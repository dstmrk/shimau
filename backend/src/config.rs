//! Process configuration, read once at startup from the environment.
//!
//! Every knob is an environment variable so the manager stays configurable
//! from its own `compose.yaml` without a config file to mount.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Fatal misconfiguration. Reported once, at startup, before the server binds.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0} is required but not set")]
    Missing(&'static str),
    #[error("{var} = {value:?} is not a valid {expected}")]
    Invalid {
        var: &'static str,
        value: String,
        expected: &'static str,
    },
    #[error("{var} = {value:?} does not resolve to an existing directory: {source}")]
    UnreadableDir {
        var: &'static str,
        value: String,
        source: std::io::Error,
    },
    #[error("{var} = {value:?} is not a directory")]
    NotADir { var: &'static str, value: String },
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Root of the managed stacks. Canonical, so every stack path can be
    /// checked against it with a prefix test (see `stacks::paths`).
    pub stacks_dir: PathBuf,
    /// Where `shimau.db` lives. Manager-owned metadata only.
    pub data_dir: PathBuf,
    /// Built frontend assets.
    pub static_dir: PathBuf,
    pub bind: SocketAddr,
    /// Admin bootstrap. Applied only when the users table is empty.
    pub admin_username: String,
    pub admin_password: Option<String>,
    /// `Secure` attribute on the session cookie. Off only for plain-HTTP LAN
    /// installs — a browser drops a `Secure` cookie sent over http://.
    pub cookie_secure: bool,
    pub session_ttl_hours: i64,
    /// Default number of log lines returned before following.
    pub log_tail: u32,
}

impl Config {
    /// Reads the configuration from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_source(|key| std::env::var(key).ok())
    }

    /// Same as [`Config::from_env`] with an injectable source, so the parsing
    /// rules are testable without mutating the process environment.
    pub fn from_source<F>(get: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let stacks_raw =
            get("SHIMAU_STACKS_DIR").ok_or(ConfigError::Missing("SHIMAU_STACKS_DIR"))?;
        let stacks_dir = canonical_dir("SHIMAU_STACKS_DIR", &stacks_raw)?;

        let data_dir = PathBuf::from(get("SHIMAU_DATA_DIR").unwrap_or_else(|| "/app/data".into()));
        let static_dir =
            PathBuf::from(get("SHIMAU_STATIC_DIR").unwrap_or_else(|| "/app/static".into()));

        let bind_raw = get("SHIMAU_BIND").unwrap_or_else(|| "0.0.0.0:8080".into());
        let bind = bind_raw
            .parse::<SocketAddr>()
            .map_err(|_| ConfigError::Invalid {
                var: "SHIMAU_BIND",
                value: bind_raw.clone(),
                expected: "host:port socket address",
            })?;

        let admin_username = get("SHIMAU_ADMIN_USERNAME").unwrap_or_else(|| "admin".into());
        // An empty password is treated as absent: a blank value in the compose
        // file must not silently become a credential.
        let admin_password = get("SHIMAU_ADMIN_PASSWORD").filter(|p| !p.is_empty());

        let cookie_secure = parse_bool("SHIMAU_COOKIE_SECURE", get("SHIMAU_COOKIE_SECURE"), true)?;
        let session_ttl_hours = parse_num::<i64>(
            "SHIMAU_SESSION_TTL_HOURS",
            get("SHIMAU_SESSION_TTL_HOURS"),
            168,
        )?;
        let log_tail = parse_num::<u32>("SHIMAU_LOG_TAIL", get("SHIMAU_LOG_TAIL"), 200)?;

        Ok(Self {
            stacks_dir,
            data_dir,
            static_dir,
            bind,
            admin_username,
            admin_password,
            cookie_secure,
            session_ttl_hours,
            log_tail,
        })
    }

    /// Path of the manager-owned SQLite database.
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("shimau.db")
    }
}

fn canonical_dir(var: &'static str, value: &str) -> Result<PathBuf, ConfigError> {
    let path = Path::new(value);
    let canonical = path
        .canonicalize()
        .map_err(|source| ConfigError::UnreadableDir {
            var,
            value: value.to_string(),
            source,
        })?;
    if !canonical.is_dir() {
        return Err(ConfigError::NotADir {
            var,
            value: value.to_string(),
        });
    }
    Ok(canonical)
}

fn parse_bool(
    var: &'static str,
    value: Option<String>,
    default: bool,
) -> Result<bool, ConfigError> {
    match value.as_deref() {
        None | Some("") => Ok(default),
        Some("1") | Some("true") | Some("TRUE") | Some("True") => Ok(true),
        Some("0") | Some("false") | Some("FALSE") | Some("False") => Ok(false),
        Some(other) => Err(ConfigError::Invalid {
            var,
            value: other.to_string(),
            expected: "boolean (true/false)",
        }),
    }
}

fn parse_num<T>(var: &'static str, value: Option<String>, default: T) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
{
    match value.as_deref() {
        None | Some("") => Ok(default),
        Some(raw) => raw.parse::<T>().map_err(|_| ConfigError::Invalid {
            var,
            value: raw.to_string(),
            expected: "positive integer",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn source(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn stacks_dir_is_required() {
        let err = Config::from_source(source(&[])).unwrap_err();
        assert!(matches!(err, ConfigError::Missing("SHIMAU_STACKS_DIR")));
    }

    #[test]
    fn stacks_dir_must_exist() {
        let err = Config::from_source(source(&[("SHIMAU_STACKS_DIR", "/nope/does/not/exist")]))
            .unwrap_err();
        assert!(matches!(err, ConfigError::UnreadableDir { .. }));
    }

    #[test]
    fn defaults_are_applied() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::from_source(source(&[(
            "SHIMAU_STACKS_DIR",
            dir.path().to_str().unwrap(),
        )]))
        .unwrap();
        assert_eq!(cfg.bind.to_string(), "0.0.0.0:8080");
        assert_eq!(cfg.admin_username, "admin");
        assert!(cfg.admin_password.is_none());
        assert!(cfg.cookie_secure);
        assert_eq!(cfg.session_ttl_hours, 168);
        assert_eq!(cfg.log_tail, 200);
        assert_eq!(cfg.database_path(), PathBuf::from("/app/data/shimau.db"));
    }

    #[test]
    fn empty_admin_password_counts_as_unset() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::from_source(source(&[
            ("SHIMAU_STACKS_DIR", dir.path().to_str().unwrap()),
            ("SHIMAU_ADMIN_PASSWORD", ""),
        ]))
        .unwrap();
        assert!(cfg.admin_password.is_none());
    }

    #[test]
    fn invalid_bind_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = Config::from_source(source(&[
            ("SHIMAU_STACKS_DIR", dir.path().to_str().unwrap()),
            ("SHIMAU_BIND", "not-an-address"),
        ]))
        .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                var: "SHIMAU_BIND",
                ..
            }
        ));
    }

    #[test]
    fn invalid_bool_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = Config::from_source(source(&[
            ("SHIMAU_STACKS_DIR", dir.path().to_str().unwrap()),
            ("SHIMAU_COOKIE_SECURE", "maybe"),
        ]))
        .unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));
    }
}
