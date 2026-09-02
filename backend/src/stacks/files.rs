//! Atomic file replacement for the Compose and `.env` editors (spec §12).
//!
//! The contract: the file on disk is either the old content or the new one,
//! never a truncated write and never absent. Everything is staged next to
//! the target so the final step is a `rename(2)` within the same filesystem.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

/// Permissions for a file that may contain secrets (`.env` and its backup).
#[cfg(unix)]
pub const SECRET_MODE: u32 = 0o600;
/// Permissions for a new Compose file when there is no existing one to copy.
#[cfg(unix)]
pub const DEFAULT_MODE: u32 = 0o644;

/// Largest editable file accepted from the browser. A Compose file or an
/// `.env` is kilobytes; the cap exists so a stray upload cannot fill the
/// stacks volume.
pub const MAX_EDITABLE_BYTES: usize = 1024 * 1024;

/// A candidate file staged next to its target, not yet visible under the
/// target name. Dropping it without [`AtomicWrite::commit`] removes the
/// staged file, so a failed validation leaves nothing behind.
#[derive(Debug)]
pub struct AtomicWrite {
    target: PathBuf,
    staged: PathBuf,
    committed: bool,
}

impl AtomicWrite {
    /// Writes `content` to a staging file in the target's own directory.
    ///
    /// `mode` applies on Unix; on other platforms it is ignored.
    pub fn stage(target: &Path, content: &str, mode: u32) -> std::io::Result<Self> {
        let dir = target.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "target has no parent directory",
            )
        })?;
        let filename = target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let staged = dir.join(format!(
            ".shimau-staged-{}-{}-{filename}",
            std::process::id(),
            unique_suffix()
        ));

        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(mode);
        #[cfg(not(unix))]
        let _ = mode;

        let mut file = options.open(&staged)?;
        file.write_all(content.as_bytes())?;
        // fsync before the rename: a rename is atomic in the directory, but
        // without this the new inode's data may still be in flight if the
        // host loses power right after.
        file.sync_all()?;

        Ok(Self {
            target: target.to_path_buf(),
            staged,
            committed: false,
        })
    }

    /// Path of the staged file, for validators that need to read it.
    pub fn staged_path(&self) -> &Path {
        &self.staged
    }

    /// Replaces the target with the staged file.
    ///
    /// When `backup_suffix` is set and the target exists, the current content
    /// is *copied* aside first — a copy rather than a rename, so the target is
    /// never momentarily missing. The backup inherits the staged file's mode,
    /// which matters for `.env` (spec §12).
    pub fn commit(mut self, backup_suffix: Option<&str>, backup_mode: u32) -> std::io::Result<()> {
        if let Some(suffix) = backup_suffix {
            if self.target.exists() {
                let backup = with_suffix(&self.target, suffix);
                fs::copy(&self.target, &backup)?;
                #[cfg(unix)]
                fs::set_permissions(&backup, fs::Permissions::from_mode(backup_mode))?;
                #[cfg(not(unix))]
                let _ = backup_mode;
            }
        }
        fs::rename(&self.staged, &self.target)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for AtomicWrite {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.staged);
        }
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    name.push_str(suffix);
    path.with_file_name(name)
}

/// Mode to give a replacement file: the existing file's, or `fallback`.
pub fn mode_of(path: &Path, fallback: u32) -> u32 {
    #[cfg(unix)]
    {
        fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o777)
            .unwrap_or(fallback)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        fallback
    }
}

fn unique_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_replaces_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("compose.yaml");
        fs::write(&target, "old").unwrap();

        let write = AtomicWrite::stage(&target, "new", 0o644).unwrap();
        write.commit(None, 0o644).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn dropping_without_commit_leaves_the_original_and_no_staged_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("compose.yaml");
        fs::write(&target, "old").unwrap();

        {
            let _write = AtomicWrite::stage(&target, "new", 0o644).unwrap();
        }

        assert_eq!(fs::read_to_string(&target).unwrap(), "old");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".shimau-staged-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staged files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn commit_writes_a_backup_of_the_previous_content() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("compose.yaml");
        fs::write(&target, "old").unwrap();

        let write = AtomicWrite::stage(&target, "new", 0o644).unwrap();
        write.commit(Some(".bak"), 0o644).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(
            fs::read_to_string(dir.path().join("compose.yaml.bak")).unwrap(),
            "old"
        );
    }

    #[test]
    fn no_backup_is_written_when_the_target_is_new() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(".env");

        let write = AtomicWrite::stage(&target, "A=1", 0o600).unwrap();
        write.commit(Some(".bak"), 0o600).unwrap();

        assert!(!dir.path().join(".env.bak").exists());
    }

    #[cfg(unix)]
    #[test]
    fn secret_files_and_their_backups_are_not_world_readable() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(".env");
        fs::write(&target, "TOKEN=old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();

        let write = AtomicWrite::stage(&target, "TOKEN=new", SECRET_MODE).unwrap();
        write.commit(Some(".bak"), SECRET_MODE).unwrap();

        for path in [target.clone(), dir.path().join(".env.bak")] {
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{path:?} has mode {mode:o}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn mode_of_reads_the_existing_file_and_falls_back_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("compose.yaml");
        fs::write(&existing, "").unwrap();
        fs::set_permissions(&existing, fs::Permissions::from_mode(0o640)).unwrap();

        assert_eq!(mode_of(&existing, DEFAULT_MODE), 0o640);
        assert_eq!(
            mode_of(&dir.path().join("missing"), DEFAULT_MODE),
            DEFAULT_MODE
        );
    }

    #[test]
    fn staged_file_is_visible_to_a_validator_before_commit() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("compose.yaml");

        let write = AtomicWrite::stage(&target, "services: {}", 0o644).unwrap();
        assert_eq!(
            fs::read_to_string(write.staged_path()).unwrap(),
            "services: {}"
        );
        assert!(!target.exists());
    }
}
