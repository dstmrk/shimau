//! Manager-owned metadata (spec §8).
//!
//! One small SQLite file, `shimau.db`, holding the administrator account and
//! its sessions. It is deliberately not a mirror of anything Docker or the
//! filesystem already knows: no stacks, no statuses, no Compose content.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("could not create the data directory {path}: {source}")]
    DataDir {
        path: String,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone)]
pub struct AdminUser {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
}

/// Handle to the SQLite file. Cheap to clone.
///
/// Access is serialised behind a mutex and every call hops onto a blocking
/// thread: `rusqlite` is synchronous, and the query volume here (a session
/// lookup per request) does not justify a connection pool.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Opens (creating if needed) the database and applies the schema.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| DbError::DataDir {
                path: parent.display().to_string(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// In-memory database. Used by the test suites; a real deployment always
    /// goes through [`Db::open`].
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// The whole schema, idempotent. The MVP has no migration history to
    /// carry, so a single `CREATE TABLE IF NOT EXISTS` pass is the honest
    /// implementation; a versioned migrator can arrive with the second table
    /// that needs changing.
    fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS users (
                id            INTEGER PRIMARY KEY CHECK (id = 1),
                username      TEXT    NOT NULL,
                password_hash TEXT    NOT NULL,
                created_at    INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                token_hash TEXT    PRIMARY KEY,
                user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS sessions_expires_at ON sessions (expires_at);
            ",
        )
    }

    async fn call<T, F>(&self, f: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, rusqlite::Error> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().expect("database mutex poisoned");
            f(&guard)
        })
        .await?
        .map_err(DbError::from)
    }

    pub async fn admin_user(&self) -> Result<Option<AdminUser>, DbError> {
        self.call(|conn| {
            conn.query_row(
                "SELECT id, username, password_hash FROM users WHERE id = 1",
                [],
                |row| {
                    Ok(AdminUser {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                    })
                },
            )
            .optional()
        })
        .await
    }

    /// Creates the administrator. Fails if one already exists — the bootstrap
    /// path must never silently rewrite a password.
    pub async fn create_admin(
        &self,
        username: String,
        password_hash: String,
    ) -> Result<(), DbError> {
        let now = now_unix();
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO users (id, username, password_hash, created_at) VALUES (1, ?1, ?2, ?3)",
                rusqlite::params![username, password_hash, now],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn insert_session(
        &self,
        token_hash: String,
        user_id: i64,
        expires_at: i64,
    ) -> Result<(), DbError> {
        let now = now_unix();
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO sessions (token_hash, user_id, created_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![token_hash, user_id, now, expires_at],
            )?;
            Ok(())
        })
        .await
    }

    /// Returns the session's user when the token is known and unexpired.
    pub async fn session_user(&self, token_hash: String) -> Result<Option<AdminUser>, DbError> {
        let now = now_unix();
        self.call(move |conn| {
            conn.query_row(
                "SELECT u.id, u.username, u.password_hash
                   FROM sessions s
                   JOIN users u ON u.id = s.user_id
                  WHERE s.token_hash = ?1 AND s.expires_at > ?2",
                rusqlite::params![token_hash, now],
                |row| {
                    Ok(AdminUser {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                    })
                },
            )
            .optional()
        })
        .await
    }

    pub async fn delete_session(&self, token_hash: String) -> Result<(), DbError> {
        self.call(move |conn| {
            conn.execute(
                "DELETE FROM sessions WHERE token_hash = ?1",
                rusqlite::params![token_hash],
            )?;
            Ok(())
        })
        .await
    }

    /// Drops expired rows. Called on startup and after each login.
    pub async fn purge_expired_sessions(&self) -> Result<usize, DbError> {
        let now = now_unix();
        self.call(move |conn| {
            conn.execute(
                "DELETE FROM sessions WHERE expires_at <= ?1",
                rusqlite::params![now],
            )
        })
        .await
    }
}

/// Seconds since the Unix epoch.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.create_admin("admin".into(), "$argon2id$fake".into())
            .await
            .unwrap();
        db
    }

    #[tokio::test]
    async fn admin_is_absent_on_a_fresh_database() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.admin_user().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn create_admin_is_not_idempotent() {
        let db = seeded().await;
        let second = db.create_admin("other".into(), "hash".into()).await;
        assert!(second.is_err(), "a second admin must be rejected");
        assert_eq!(db.admin_user().await.unwrap().unwrap().username, "admin");
    }

    #[tokio::test]
    async fn a_session_resolves_to_its_user() {
        let db = seeded().await;
        db.insert_session("hash-a".into(), 1, now_unix() + 3600)
            .await
            .unwrap();
        let user = db.session_user("hash-a".into()).await.unwrap();
        assert_eq!(user.unwrap().username, "admin");
    }

    #[tokio::test]
    async fn an_expired_session_does_not_authenticate() {
        let db = seeded().await;
        db.insert_session("hash-b".into(), 1, now_unix() - 1)
            .await
            .unwrap();
        assert!(db.session_user("hash-b".into()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn an_unknown_token_does_not_authenticate() {
        let db = seeded().await;
        assert!(db.session_user("nope".into()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn logout_removes_the_session() {
        let db = seeded().await;
        db.insert_session("hash-c".into(), 1, now_unix() + 3600)
            .await
            .unwrap();
        db.delete_session("hash-c".into()).await.unwrap();
        assert!(db.session_user("hash-c".into()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn purge_removes_only_expired_sessions() {
        let db = seeded().await;
        db.insert_session("live".into(), 1, now_unix() + 3600)
            .await
            .unwrap();
        db.insert_session("dead".into(), 1, now_unix() - 10)
            .await
            .unwrap();
        assert_eq!(db.purge_expired_sessions().await.unwrap(), 1);
        assert!(db.session_user("live".into()).await.unwrap().is_some());
    }
}
