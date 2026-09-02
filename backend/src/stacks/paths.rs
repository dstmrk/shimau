//! Resolution of a stack name into a filesystem path.
//!
//! This is the only place where a browser-supplied string becomes a path.
//! Spec §7.3: the resolved path must stay inside the configured stacks
//! directory, and the browser must never be able to name an arbitrary path.
//!
//! Two independent gates, because either one alone has a known hole:
//!
//! 1. a character allowlist on the name (rejects `..`, `/`, NUL, absolute
//!    paths, and anything URL-decoded into a separator);
//! 2. canonicalisation of the joined path, checked against the canonical
//!    root (rejects a symlink inside the root that escapes it).

use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("stack name is empty")]
    Empty,
    #[error("stack name is too long")]
    TooLong,
    #[error("stack name contains characters that are not allowed")]
    IllegalCharacter,
    #[error("stack name is a relative path segment")]
    RelativeSegment,
    #[error("stack not found")]
    NotFound,
    #[error("stack path escapes the configured stacks directory")]
    Escapes,
    #[error("stack path is not a directory")]
    NotADirectory,
}

/// Longest stack directory name accepted. Compose project names derive from
/// the directory name and get truncated far below this.
const MAX_NAME_LEN: usize = 128;

/// Validates a stack *name* in isolation, without touching the filesystem.
pub fn validate_name(name: &str) -> Result<(), PathError> {
    if name.is_empty() {
        return Err(PathError::Empty);
    }
    if name.len() > MAX_NAME_LEN {
        return Err(PathError::TooLong);
    }
    if name == "." || name == ".." {
        return Err(PathError::RelativeSegment);
    }
    // A leading dot would expose `.git`, `.ssh` and friends sitting next to
    // the stacks; discovery ignores them too, so accepting one here would
    // create a path reachable by name but absent from the listing.
    if name.starts_with('.') {
        return Err(PathError::IllegalCharacter);
    }
    let legal = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if !legal {
        return Err(PathError::IllegalCharacter);
    }
    Ok(())
}

/// Resolves `name` against `root` and returns the canonical stack directory.
///
/// `root` must already be canonical (see `Config::from_env`).
pub fn resolve(root: &Path, name: &str) -> Result<PathBuf, PathError> {
    validate_name(name)?;
    let joined = root.join(name);
    let canonical = joined.canonicalize().map_err(|_| PathError::NotFound)?;
    if !canonical.starts_with(root) {
        return Err(PathError::Escapes);
    }
    if !canonical.is_dir() {
        return Err(PathError::NotADirectory);
    }
    Ok(canonical)
}

/// Resolves a file *inside* an already-resolved stack directory.
///
/// Used for the Compose file and `.env`: the caller supplies a filename from
/// a fixed set, never from the request body, but the canonical check is
/// repeated so a symlinked `compose.yaml` pointing at `/etc/shadow` cannot be
/// read or written through the editor.
pub fn resolve_file(stack_dir: &Path, filename: &str) -> Result<PathBuf, PathError> {
    if filename.is_empty() {
        return Err(PathError::Empty);
    }
    if filename.contains('/') || filename.contains('\\') || filename.contains('\0') {
        return Err(PathError::IllegalCharacter);
    }
    if filename == "." || filename == ".." {
        return Err(PathError::RelativeSegment);
    }
    let joined = stack_dir.join(filename);
    match joined.canonicalize() {
        // The file exists: it must resolve inside the stack directory.
        Ok(canonical) => {
            if !canonical.starts_with(stack_dir) {
                return Err(PathError::Escapes);
            }
            Ok(canonical)
        }
        // The file does not exist yet (creating a `.env`, writing a backup).
        // The parent is already canonical, so the join is safe.
        Err(_) => Ok(joined),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("octotracker")).unwrap();
        dir
    }

    #[test]
    fn accepts_ordinary_names() {
        for name in ["octotracker", "uptime-kuma", "grafana_2", "app.v2"] {
            assert!(validate_name(name).is_ok(), "{name} should be accepted");
        }
    }

    #[test]
    fn rejects_traversal_and_separators() {
        for name in [
            "..",
            ".",
            "../etc",
            "../../etc/passwd",
            "/etc/passwd",
            "foo/bar",
            "foo\\bar",
            "foo\0bar",
            "",
        ] {
            assert!(
                validate_name(name).is_err(),
                "{name:?} should have been rejected"
            );
        }
    }

    #[test]
    fn rejects_dotfiles() {
        assert_eq!(validate_name(".git"), Err(PathError::IllegalCharacter));
        assert_eq!(validate_name(".env"), Err(PathError::IllegalCharacter));
    }

    #[test]
    fn rejects_overlong_names() {
        assert_eq!(validate_name(&"a".repeat(129)), Err(PathError::TooLong));
    }

    #[test]
    fn resolve_returns_the_canonical_stack_dir() {
        let dir = root();
        let canonical_root = dir.path().canonicalize().unwrap();
        let resolved = resolve(&canonical_root, "octotracker").unwrap();
        assert_eq!(resolved, canonical_root.join("octotracker"));
    }

    #[test]
    fn resolve_rejects_traversal_before_touching_disk() {
        let dir = root();
        let canonical_root = dir.path().canonicalize().unwrap();
        assert_eq!(
            resolve(&canonical_root, "../../etc/passwd"),
            Err(PathError::IllegalCharacter)
        );
    }

    #[test]
    fn resolve_rejects_missing_stack() {
        let dir = root();
        let canonical_root = dir.path().canonicalize().unwrap();
        assert_eq!(
            resolve(&canonical_root, "nonexistent"),
            Err(PathError::NotFound)
        );
    }

    #[test]
    fn resolve_rejects_a_file_masquerading_as_a_stack() {
        let dir = root();
        let canonical_root = dir.path().canonicalize().unwrap();
        fs::write(canonical_root.join("notadir"), "x").unwrap();
        assert_eq!(
            resolve(&canonical_root, "notadir"),
            Err(PathError::NotADirectory)
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_a_symlink_escaping_the_root() {
        let outside = tempfile::tempdir().unwrap();
        let dir = root();
        let canonical_root = dir.path().canonicalize().unwrap();
        std::os::unix::fs::symlink(outside.path(), canonical_root.join("escape")).unwrap();
        assert_eq!(resolve(&canonical_root, "escape"), Err(PathError::Escapes));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_file_rejects_a_symlinked_compose_file() {
        let dir = root();
        let canonical_root = dir.path().canonicalize().unwrap();
        let stack = canonical_root.join("octotracker");
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), stack.join("compose.yaml")).unwrap();
        assert_eq!(
            resolve_file(&stack, "compose.yaml"),
            Err(PathError::Escapes)
        );
    }

    #[test]
    fn resolve_file_allows_a_file_that_does_not_exist_yet() {
        let dir = root();
        let stack = dir.path().canonicalize().unwrap().join("octotracker");
        assert_eq!(resolve_file(&stack, ".env").unwrap(), stack.join(".env"));
    }

    #[test]
    fn resolve_file_rejects_separators() {
        let dir = root();
        let stack = dir.path().canonicalize().unwrap().join("octotracker");
        assert_eq!(
            resolve_file(&stack, "../compose.yaml"),
            Err(PathError::IllegalCharacter)
        );
    }
}
