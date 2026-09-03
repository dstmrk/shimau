//! The only place that shells out to Docker.
//!
//! Spec §6.2: the backend drives the Compose CLI instead of reimplementing
//! it, and the set of commands it can build is closed. There is no generic
//! execution path — `Action` is an enum, never a string from a request.

pub mod status;

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

/// A lifecycle action the UI can trigger on a stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Start,
    Stop,
    Restart,
    /// `pull` followed by `up -d`; see [`Action::steps`].
    Update,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
            Action::Update => "update",
        }
    }

    /// The Compose invocations this action expands to, in order.
    ///
    /// Spec §4.3: Update is exactly `pull` then `up -d` — no bespoke image
    /// update logic. Stop is `stop`, never `down`: containers, networks and
    /// volumes survive.
    pub fn steps(self) -> &'static [&'static [&'static str]] {
        match self {
            Action::Start => &[&["up", "-d"]],
            Action::Stop => &[&["stop"]],
            Action::Restart => &[&["restart"]],
            Action::Update => &[&["pull"], &["up", "-d"]],
        }
    }
}

/// Environment variables forwarded to every Compose child process.
///
/// The child's environment is cleared and rebuilt from this allowlist. Two
/// reasons, both load-bearing:
///
/// * Compose interpolates `${VAR}` in a Compose file from its own
///   environment. Inheriting ours would let a Compose file read
///   `SHIMAU_ADMIN_PASSWORD` back out through `docker compose config`.
/// * A stack's own values must come from its `.env`, not from whatever the
///   manager container happens to have set.
const FORWARDED_ENV: [&str; 7] = [
    "PATH",
    "HOME",
    "DOCKER_HOST",
    "DOCKER_CONFIG",
    "DOCKER_CERT_PATH",
    "DOCKER_TLS_VERIFY",
    "DOCKER_API_VERSION",
];

/// Global flags that precede every subcommand.
///
/// `--ansi never` and `--progress plain` keep the output line-oriented, which
/// is what the SSE stream and the log viewer consume.
fn base_args(compose_file: &str) -> Vec<String> {
    vec![
        "compose".into(),
        "--ansi".into(),
        "never".into(),
        "--progress".into(),
        "plain".into(),
        "--file".into(),
        compose_file.into(),
    ]
}

/// Full argv (without the `docker` program name) for a Compose invocation.
///
/// Split out from [`command`] so the action-to-command mapping is assertable
/// without a Docker daemon.
pub fn argv(compose_file: &str, subcommand: &[&str]) -> Vec<String> {
    let mut args = base_args(compose_file);
    args.extend(subcommand.iter().map(|s| (*s).to_string()));
    args
}

/// Builds a Compose command rooted at the stack directory.
///
/// Spec §6.3: the working directory *is* the stack directory, because Compose
/// files routinely use relative bind mounts (`./data:/data`).
pub fn command(stack_dir: &Path, compose_file: &str, subcommand: &[&str]) -> Command {
    let mut cmd = Command::new("docker");
    cmd.args(argv(compose_file, subcommand));
    cmd.current_dir(stack_dir);
    cmd.env_clear();
    for key in FORWARDED_ENV {
        if let Ok(value) = std::env::var(key) {
            cmd.env(key, value);
        }
    }
    cmd
}

/// Argv for `docker compose logs`.
pub fn logs_args(tail: u32, follow: bool) -> Vec<String> {
    let mut args = vec![
        "logs".to_string(),
        "--no-color".to_string(),
        "--tail".to_string(),
        tail.to_string(),
    ];
    if follow {
        args.push("--follow".to_string());
    }
    args
}

/// Why a Compose invocation produced no outcome.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("could not run docker compose: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("docker compose did not answer within {0} seconds")]
    TimedOut(u64),
}

/// Result of a Compose invocation that ran to completion.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub status_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    pub fn success(&self) -> bool {
        self.status_code == Some(0)
    }

    /// Human-readable reason, for the `details` field of an API error.
    pub fn failure_details(&self) -> String {
        let code = self
            .status_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        let output = if self.stderr.trim().is_empty() {
            self.stdout.trim()
        } else {
            self.stderr.trim()
        };
        if output.is_empty() {
            format!("exit code {code}")
        } else {
            format!("exit code {code}\n\n{output}")
        }
    }
}

/// Runs a Compose command to completion and captures its output, giving up
/// after `limit`.
///
/// Every command that has to answer an HTTP request goes through here, so the
/// budget cannot be forgotten at a call site. The child is spawned with
/// `kill_on_drop`, which is what makes the timeout mean something: dropping
/// the future on a lapsed budget — or when the browser hangs up mid-request —
/// kills the process instead of leaving one `docker compose ps` per poll
/// behind an unresponsive daemon.
pub async fn run_with_timeout(mut cmd: Command, limit: Duration) -> Result<Outcome, RunError> {
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::null());
    cmd.kill_on_drop(true);
    let child = cmd.spawn()?;

    let output = tokio::time::timeout(limit, child.wait_with_output())
        .await
        .map_err(|_| RunError::TimedOut(limit.as_secs()))??;

    Ok(Outcome {
        status_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Which pipe a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputLine {
    pub stream: StreamKind,
    pub text: String,
}

/// A running Compose command whose output is delivered line by line.
///
/// The child is spawned with `kill_on_drop`, so a browser that closes a log
/// stream cannot leave `docker compose logs --follow` running forever.
pub struct LineStream {
    lines: mpsc::Receiver<OutputLine>,
    child: Child,
}

impl LineStream {
    /// Next output line, or `None` once both pipes are closed.
    pub async fn next_line(&mut self) -> Option<OutputLine> {
        self.lines.recv().await
    }

    /// Waits for the process to exit and returns its exit code.
    pub async fn wait(&mut self) -> std::io::Result<Option<i32>> {
        Ok(self.child.wait().await?.code())
    }
}

/// Spawns a Compose command and streams stdout and stderr as interleaved lines.
pub fn spawn_lines(mut cmd: Command) -> std::io::Result<LineStream> {
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::null());
    cmd.kill_on_drop(true);
    let mut child = cmd.spawn()?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // Bounded so a chatty stack applies backpressure instead of growing the
    // queue without limit.
    let (tx, rx) = mpsc::channel::<OutputLine>(512);
    spawn_pipe_reader(stdout, StreamKind::Stdout, tx.clone());
    spawn_pipe_reader(stderr, StreamKind::Stderr, tx);

    Ok(LineStream { lines: rx, child })
}

fn spawn_pipe_reader<R>(pipe: R, stream: StreamKind, tx: mpsc::Sender<OutputLine>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = BufReader::new(pipe).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if tx.send(OutputLine { stream, text: line }).await.is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_is_stop_never_down() {
        // Spec §4.3 and §13: `down` must not be reachable from the UI.
        assert_eq!(Action::Stop.steps(), &[&["stop"]]);
        for action in [Action::Start, Action::Stop, Action::Restart, Action::Update] {
            for step in action.steps() {
                assert!(
                    !step.contains(&"down"),
                    "{:?} must not expand to a `down`",
                    action
                );
            }
        }
    }

    #[test]
    fn action_to_command_mapping_is_exact() {
        assert_eq!(Action::Start.steps(), &[&["up", "-d"]]);
        assert_eq!(Action::Restart.steps(), &[&["restart"]]);
        assert_eq!(
            Action::Update.steps(),
            &[&["pull"] as &[&str], &["up", "-d"]]
        );
    }

    #[test]
    fn argv_pins_the_compose_filename() {
        let args = argv("docker-compose.yml", &["up", "-d"]);
        assert_eq!(
            args,
            vec![
                "compose",
                "--ansi",
                "never",
                "--progress",
                "plain",
                "--file",
                "docker-compose.yml",
                "up",
                "-d"
            ]
        );
    }

    #[test]
    fn logs_args_respect_tail_and_follow() {
        assert_eq!(
            logs_args(50, false),
            vec!["logs", "--no-color", "--tail", "50"]
        );
        assert_eq!(
            logs_args(200, true),
            vec!["logs", "--no-color", "--tail", "200", "--follow"]
        );
    }

    #[test]
    fn command_runs_in_the_stack_directory() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = command(dir.path(), "compose.yaml", &["ps"]);
        let std_cmd = cmd.as_std();
        assert_eq!(std_cmd.get_current_dir(), Some(dir.path()));
    }

    #[test]
    fn command_does_not_forward_manager_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = command(dir.path(), "compose.yaml", &["config"]);
        let std_cmd = cmd.as_std();
        let forwarded: Vec<_> = std_cmd
            .get_envs()
            .filter_map(|(k, _)| k.to_str().map(str::to_string))
            .collect();
        for key in &forwarded {
            assert!(
                FORWARDED_ENV.contains(&key.as_str()),
                "{key} must not reach a Compose child process"
            );
        }
        assert!(!forwarded.iter().any(|k| k.starts_with("SHIMAU_")));
    }

    /// The one test in the suite that waits on wall-clock time: a process
    /// dying is only observable by looking a moment later. The margins are
    /// wide (a 200ms budget against a child that writes after a second) so a
    /// loaded runner cannot turn it red.
    #[tokio::test]
    async fn a_command_that_outlives_its_budget_is_killed() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("survived");

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(format!("sleep 1; : > {}", marker.display()));

        let error = run_with_timeout(cmd, Duration::from_millis(200))
            .await
            .unwrap_err();
        assert!(matches!(error, RunError::TimedOut(_)), "got {error:?}");

        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(
            !marker.exists(),
            "the child outlived the timeout and kept running"
        );
    }

    #[tokio::test]
    async fn a_command_inside_its_budget_returns_its_output() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("printf hello; exit 3");
        let outcome = run_with_timeout(cmd, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(outcome.status_code, Some(3));
        assert_eq!(outcome.stdout, "hello");
        assert!(!outcome.success());
    }

    #[test]
    fn failure_details_prefer_stderr() {
        let outcome = Outcome {
            status_code: Some(1),
            stdout: "noise".into(),
            stderr: "manifest unknown".into(),
        };
        assert!(!outcome.success());
        let details = outcome.failure_details();
        assert!(details.contains("exit code 1"));
        assert!(details.contains("manifest unknown"));
    }

    #[test]
    fn failure_details_fall_back_to_stdout() {
        let outcome = Outcome {
            status_code: Some(2),
            stdout: "something on stdout".into(),
            stderr: "   ".into(),
        };
        assert!(outcome.failure_details().contains("something on stdout"));
    }
}
