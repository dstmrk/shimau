//! Manager-owned metadata (spec §8).
//!
//! One small SQLite file, `shimau.db`, holding the administrator account, its
//! sessions and the API tokens issued to machine clients. It is deliberately
//! not a mirror of anything Docker or the filesystem already knows: no stacks,
//! no statuses, no Compose content.
//!
//! The schema changes through [`MIGRATIONS`], keyed on `PRAGMA user_version`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

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

/// Schema versions, in order. Append only: the index is the version number,
/// so reordering or removing one silently reinterprets existing databases.
const MIGRATIONS: &[&str] = &[
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
    "
    CREATE TABLE IF NOT EXISTS api_tokens (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        token_hash   TEXT    NOT NULL UNIQUE,
        label        TEXT    NOT NULL,
        capability   TEXT    NOT NULL,
        created_at   INTEGER NOT NULL,
        last_used_at INTEGER
    );
    ",
];

/// How stale `last_used_at` is allowed to get before a request rewrites it.
pub const TOUCH_INTERVAL_SECS: i64 = 60;

#[derive(Debug, Clone)]
pub struct AdminUser {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
}

/// What an API token is allowed to do.
///
/// Deliberately two, and deliberately neither of them writes a file. A token
/// that could save a Compose file could give a service `privileged: true` and
/// a bind mount of `/`, and then start it: `docker compose config` validates
/// syntax, not intent. Editing a Compose file stays with the browser session,
/// where a human is looking at the diff. A third capability for it would be a
/// new decision, not a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Read stacks, status, logs, stats, Compose files and operations.
    Read,
    /// Everything `Read` can do, plus start, stop, restart and update.
    Operate,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::Read => "read",
            Capability::Operate => "operate",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "read" => Some(Capability::Read),
            "operate" => Some(Capability::Operate),
            _ => None,
        }
    }

    /// Whether this capability may run a lifecycle action.
    pub fn may_operate(self) -> bool {
        matches!(self, Capability::Operate)
    }
}

/// A stored API token. The token itself is not here: only its hash was ever
/// written, and this struct never carries it.
#[derive(Debug, Clone, Serialize)]
pub struct ApiToken {
    pub id: i64,
    pub label: String,
    pub capability: Capability,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
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

    /// Applies every migration the database has not seen yet.
    ///
    /// The position in [`MIGRATIONS`] is the schema version, tracked in
    /// `PRAGMA user_version`. Step 0 is the original two-table schema: a
    /// database created before this migrator existed reports version 0 and
    /// replays it, which is why every statement is written `IF NOT EXISTS`.
    ///
    /// That idempotence is also the crash story. A migration and the version
    /// bump that follows it are two statements, not one transaction, so a
    /// process killed between them replays the migration on the next start.
    /// Replaying must be a no-op, not an error.
    fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
        let applied: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        for (index, statement) in MIGRATIONS.iter().enumerate() {
            if (index as i64) < applied {
                continue;
            }
            conn.execute_batch(statement)?;
            conn.pragma_update(None, "user_version", index as i64 + 1)?;
        }
        Ok(())
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

    /// Stores a new API token and returns the row describing it.
    pub async fn create_token(
        &self,
        token_hash: String,
        label: String,
        capability: Capability,
    ) -> Result<ApiToken, DbError> {
        let now = now_unix();
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO api_tokens (token_hash, label, capability, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![token_hash, label, capability.as_str(), now],
            )?;
            Ok(ApiToken {
                id: conn.last_insert_rowid(),
                label,
                capability,
                created_at: now,
                last_used_at: None,
            })
        })
        .await
    }

    /// Every token, newest first. Never carries the token itself.
    ///
    /// A row whose stored capability does not parse is dropped rather than
    /// guessed at: it cannot authenticate anything either (see
    /// [`Db::token_by_hash`]), so listing it as some other capability would
    /// only mislead whoever is deciding what to revoke.
    pub async fn list_tokens(&self) -> Result<Vec<ApiToken>, DbError> {
        self.call(|conn| {
            let mut statement = conn.prepare(
                "SELECT id, label, capability, created_at, last_used_at
                   FROM api_tokens ORDER BY id DESC",
            )?;
            let rows = statement.query_map([], row_to_token)?;
            let mut tokens = Vec::new();
            for row in rows {
                if let Some(token) = row? {
                    tokens.push(token);
                }
            }
            Ok(tokens)
        })
        .await
    }

    /// Resolves a presented token to its row.
    ///
    /// `None` covers both "no such token" and "a row this build cannot read",
    /// because an unrecognised capability must fail closed.
    pub async fn token_by_hash(&self, token_hash: String) -> Result<Option<ApiToken>, DbError> {
        self.call(move |conn| {
            conn.query_row(
                "SELECT id, label, capability, created_at, last_used_at
                   FROM api_tokens WHERE token_hash = ?1",
                rusqlite::params![token_hash],
                row_to_token,
            )
            .optional()
            .map(Option::flatten)
        })
        .await
    }

    /// Records that a token was used, at most once per [`TOUCH_INTERVAL_SECS`].
    ///
    /// The coarseness is the point: this runs on every authenticated machine
    /// request, and the field only has to answer "is this token still in use",
    /// which does not need second-level resolution. The staleness test is in
    /// the `WHERE` clause so two concurrent requests cannot race a read
    /// against a write.
    pub async fn touch_token(&self, id: i64) -> Result<(), DbError> {
        let now = now_unix();
        self.call(move |conn| {
            conn.execute(
                "UPDATE api_tokens SET last_used_at = ?1
                  WHERE id = ?2 AND (last_used_at IS NULL OR last_used_at < ?3)",
                rusqlite::params![now, id, now - TOUCH_INTERVAL_SECS],
            )?;
            Ok(())
        })
        .await
    }

    /// Revokes a token. `false` when there was no such row.
    pub async fn delete_token(&self, id: i64) -> Result<bool, DbError> {
        self.call(move |conn| {
            let removed = conn.execute(
                "DELETE FROM api_tokens WHERE id = ?1",
                rusqlite::params![id],
            )?;
            Ok(removed > 0)
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

/// Builds an [`ApiToken`] from a row, or `None` when its stored capability is
/// not one this build knows.
fn row_to_token(row: &rusqlite::Row) -> Result<Option<ApiToken>, rusqlite::Error> {
    let raw: String = row.get(2)?;
    let Some(capability) = Capability::parse(&raw) else {
        return Ok(None);
    };
    Ok(Some(ApiToken {
        id: row.get(0)?,
        label: row.get(1)?,
        capability,
        created_at: row.get(3)?,
        last_used_at: row.get(4)?,
    }))
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
    async fn a_token_round_trips_by_its_hash() {
        let db = seeded().await;
        let created = db
            .create_token("hash-t".into(), "laptop".into(), Capability::Read)
            .await
            .unwrap();
        assert_eq!(created.capability, Capability::Read);
        assert!(created.last_used_at.is_none());

        let found = db.token_by_hash("hash-t".into()).await.unwrap().unwrap();
        assert_eq!(found.id, created.id);
        assert_eq!(found.label, "laptop");
    }

    #[tokio::test]
    async fn an_unknown_token_hash_resolves_to_nothing() {
        let db = seeded().await;
        assert!(db.token_by_hash("nope".into()).await.unwrap().is_none());
    }

    /// The stored capability is the whole authorisation decision, so a value
    /// this build cannot read has to fail closed rather than default to
    /// something. A downgrade must not turn a future capability into `read`.
    #[tokio::test]
    async fn a_token_with_an_unreadable_capability_never_authenticates() {
        let db = seeded().await;
        db.call(|conn| {
            conn.execute(
                "INSERT INTO api_tokens (token_hash, label, capability, created_at)
                 VALUES ('hash-x', 'from the future', 'administer', 0)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        assert!(db.token_by_hash("hash-x".into()).await.unwrap().is_none());
        assert!(db.list_tokens().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn revoking_a_token_stops_it_resolving() {
        let db = seeded().await;
        let token = db
            .create_token("hash-r".into(), "ci".into(), Capability::Operate)
            .await
            .unwrap();

        assert!(db.delete_token(token.id).await.unwrap());
        assert!(db.token_by_hash("hash-r".into()).await.unwrap().is_none());
        assert!(
            !db.delete_token(token.id).await.unwrap(),
            "a second revoke has nothing to remove"
        );
    }

    #[tokio::test]
    async fn tokens_are_listed_newest_first_and_never_carry_the_secret() {
        let db = seeded().await;
        db.create_token("h1".into(), "first".into(), Capability::Read)
            .await
            .unwrap();
        db.create_token("h2".into(), "second".into(), Capability::Operate)
            .await
            .unwrap();

        let listed = db.list_tokens().await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].label, "second");
        let encoded = serde_json::to_string(&listed).unwrap();
        assert!(
            !encoded.contains("h1"),
            "the hash reached the wire: {encoded}"
        );
        assert!(
            !encoded.contains("h2"),
            "the hash reached the wire: {encoded}"
        );
    }

    #[tokio::test]
    async fn using_a_token_records_that_it_was_used() {
        let db = seeded().await;
        let token = db
            .create_token("hash-u".into(), "agent".into(), Capability::Read)
            .await
            .unwrap();

        db.touch_token(token.id).await.unwrap();
        let touched = db.token_by_hash("hash-u".into()).await.unwrap().unwrap();
        assert!(touched.last_used_at.is_some());

        // A second use inside the interval must not rewrite the row.
        let first = touched.last_used_at.unwrap();
        db.touch_token(token.id).await.unwrap();
        let again = db.token_by_hash("hash-u".into()).await.unwrap().unwrap();
        assert_eq!(again.last_used_at, Some(first));
    }

    /// A database written before the migrator existed reports version 0 with
    /// its two tables already present. Replaying step 0 has to be a no-op, and
    /// the account in it has to survive.
    #[test]
    fn migrating_an_existing_database_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shimau.db");

        let legacy = Connection::open(&path).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE users (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     username TEXT NOT NULL,
                     password_hash TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE TABLE sessions (
                     token_hash TEXT PRIMARY KEY,
                     user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                     created_at INTEGER NOT NULL,
                     expires_at INTEGER NOT NULL
                 );
                 INSERT INTO users VALUES (1, 'admin', 'hash', 0);",
            )
            .unwrap();
        drop(legacy);

        let db = Db::open(&path).unwrap();
        let version: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    /// Opening twice is what a restart does, and a replayed migration must not
    /// fail on a table that is already there.
    #[test]
    fn opening_an_already_migrated_database_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shimau.db");
        drop(Db::open(&path).unwrap());
        Db::open(&path).expect("a second open must not fail");
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
