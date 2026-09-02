//! Stack discovery: the filesystem is the source of truth (spec §3.1).
//!
//! A stack is a directory directly below the stacks root that contains
//! exactly one supported Compose filename. Zero supported files → the
//! directory is not a stack and is ignored. More than one → the stack is
//! reported as ambiguous and no destructive operation runs against it.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The four filenames Compose itself looks for, in Compose's own precedence
/// order. We do not pick a winner when several exist: spec §3.1 requires the
/// ambiguity to surface instead.
pub const SUPPORTED_COMPOSE_FILENAMES: [&str; 4] = [
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];

pub const ENV_FILENAME: &str = ".env";

/// What discovery found in one directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StackKind {
    /// Exactly one Compose file.
    Valid { compose_file: String },
    /// Several Compose files: every action is refused until it is resolved.
    Ambiguous { compose_files: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredStack {
    /// Directory name. Doubles as the stack identifier (spec §4.1).
    pub name: String,
    #[serde(flatten)]
    pub kind: StackKind,
    pub has_env_file: bool,
    #[serde(skip)]
    pub path: PathBuf,
}

impl DiscoveredStack {
    /// The single Compose filename, when the stack is unambiguous.
    pub fn compose_file(&self) -> Option<&str> {
        match &self.kind {
            StackKind::Valid { compose_file } => Some(compose_file),
            StackKind::Ambiguous { .. } => None,
        }
    }
}

/// Scans `root` and returns the stacks it contains, sorted by name.
///
/// Unreadable entries are skipped rather than failing the whole scan: one
/// bad directory must not hide every other stack.
pub fn scan(root: &Path) -> std::io::Result<Vec<DiscoveredStack>> {
    let mut stacks = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let Ok(entry) = entry else { continue };
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        // Same rule as `paths::validate_name`: a directory the API could
        // never address must not appear in the listing either.
        if super::paths::validate_name(&name).is_err() {
            continue;
        }
        let path = entry.path();
        // `metadata()` follows symlinks, so a symlinked stack directory is
        // discovered here — and then rejected by `paths::resolve` if it
        // points outside the root. Both checks are deliberate.
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        if let Some(stack) = inspect(&path, name) {
            stacks.push(stack);
        }
    }
    stacks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(stacks)
}

/// Classifies a single directory. Returns `None` when it holds no Compose file.
pub fn inspect(path: &Path, name: String) -> Option<DiscoveredStack> {
    let compose_files: Vec<String> = SUPPORTED_COMPOSE_FILENAMES
        .iter()
        .filter(|candidate| is_regular_file(&path.join(candidate)))
        .map(|candidate| (*candidate).to_string())
        .collect();

    let kind = match compose_files.len() {
        0 => return None,
        1 => StackKind::Valid {
            compose_file: compose_files.into_iter().next().expect("length checked"),
        },
        _ => StackKind::Ambiguous { compose_files },
    };

    Some(DiscoveredStack {
        name,
        kind,
        has_env_file: is_regular_file(&path.join(ENV_FILENAME)),
        path: path.to_path_buf(),
    })
}

fn is_regular_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn stack_dir(root: &Path, name: &str, files: &[(&str, &str)]) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        for (filename, contents) in files {
            fs::write(dir.join(filename), contents).unwrap();
        }
    }

    #[test]
    fn detects_all_four_compose_filenames() {
        let root = tempfile::tempdir().unwrap();
        for (index, filename) in SUPPORTED_COMPOSE_FILENAMES.iter().enumerate() {
            stack_dir(
                root.path(),
                &format!("stack{index}"),
                &[(filename, "services: {}")],
            );
        }
        let stacks = scan(root.path()).unwrap();
        assert_eq!(stacks.len(), 4);
        for (index, filename) in SUPPORTED_COMPOSE_FILENAMES.iter().enumerate() {
            assert_eq!(stacks[index].compose_file(), Some(*filename));
        }
    }

    #[test]
    fn ignores_directories_without_a_compose_file() {
        let root = tempfile::tempdir().unwrap();
        stack_dir(root.path(), "notastack", &[("readme.txt", "hi")]);
        stack_dir(root.path(), "empty", &[]);
        assert!(scan(root.path()).unwrap().is_empty());
    }

    #[test]
    fn reports_duplicate_compose_files_as_ambiguous() {
        let root = tempfile::tempdir().unwrap();
        stack_dir(
            root.path(),
            "confused",
            &[
                ("compose.yaml", "services: {}"),
                ("docker-compose.yml", "services: {}"),
            ],
        );
        let stacks = scan(root.path()).unwrap();
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].compose_file(), None);
        match &stacks[0].kind {
            StackKind::Ambiguous { compose_files } => {
                assert_eq!(compose_files, &["compose.yaml", "docker-compose.yml"]);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn detects_the_optional_env_file() {
        let root = tempfile::tempdir().unwrap();
        stack_dir(
            root.path(),
            "with",
            &[("compose.yaml", ""), (".env", "A=1")],
        );
        stack_dir(root.path(), "without", &[("compose.yaml", "")]);
        let stacks = scan(root.path()).unwrap();
        assert!(stacks[0].has_env_file);
        assert!(!stacks[1].has_env_file);
    }

    #[test]
    fn skips_files_and_dotdirs_at_the_root() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("compose.yaml"), "").unwrap();
        stack_dir(root.path(), ".hidden", &[("compose.yaml", "")]);
        stack_dir(root.path(), "real", &[("compose.yaml", "")]);
        let stacks = scan(root.path()).unwrap();
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].name, "real");
    }

    #[test]
    fn a_directory_named_like_a_compose_file_is_not_a_compose_file() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("weird/compose.yaml")).unwrap();
        assert!(scan(root.path()).unwrap().is_empty());
    }

    #[test]
    fn results_are_sorted_by_name() {
        let root = tempfile::tempdir().unwrap();
        for name in ["zulu", "alpha", "mike"] {
            stack_dir(root.path(), name, &[("compose.yaml", "")]);
        }
        let names: Vec<_> = scan(root.path())
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["alpha", "mike", "zulu"]);
    }
}
