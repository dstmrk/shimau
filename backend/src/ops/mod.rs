//! In-flight Compose operations and their live output (spec §5.2).
//!
//! An operation is a short-lived, in-memory object: an id, the lines the
//! Compose commands have printed so far, and a broadcast channel that late
//! subscribers join after replaying the buffer. Nothing is persisted — a
//! restart of the manager loses the transcript, not the containers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::broadcast;

use crate::compose::{self, Action, OutputLine};
use crate::db::now_unix;

/// Lines kept per operation. A `pull` of a large stack is chatty; the buffer
/// is what a reconnecting browser replays, not an archive.
const MAX_BUFFERED_LINES: usize = 2000;
/// Finished operations kept for late readers before being dropped.
const MAX_RETAINED_OPERATIONS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Running,
    Succeeded,
    Failed,
}

/// What the SSE stream carries.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationEvent {
    Line(OutputLine),
    Finished {
        status: OperationStatus,
        exit_code: Option<i32>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationSnapshot {
    pub id: String,
    pub stack: String,
    pub action: Action,
    pub status: OperationStatus,
    pub exit_code: Option<i32>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub lines: Vec<OutputLine>,
    /// True when older lines were dropped from the buffer.
    pub truncated: bool,
}

#[derive(Debug)]
struct OperationState {
    status: OperationStatus,
    exit_code: Option<i32>,
    finished_at: Option<i64>,
    lines: Vec<OutputLine>,
    truncated: bool,
}

#[derive(Debug)]
pub struct Operation {
    id: String,
    stack: String,
    action: Action,
    started_at: i64,
    state: Mutex<OperationState>,
    tx: broadcast::Sender<OperationEvent>,
}

impl Operation {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Snapshot plus a live receiver, taken under one lock so no line can
    /// slip between the replayed buffer and the live stream.
    pub fn subscribe(&self) -> (OperationSnapshot, broadcast::Receiver<OperationEvent>) {
        let state = self.state.lock().expect("operation mutex poisoned");
        let receiver = self.tx.subscribe();
        (self.snapshot_locked(&state), receiver)
    }

    pub fn snapshot(&self) -> OperationSnapshot {
        let state = self.state.lock().expect("operation mutex poisoned");
        self.snapshot_locked(&state)
    }

    fn snapshot_locked(&self, state: &OperationState) -> OperationSnapshot {
        OperationSnapshot {
            id: self.id.clone(),
            stack: self.stack.clone(),
            action: self.action,
            status: state.status,
            exit_code: state.exit_code,
            started_at: self.started_at,
            finished_at: state.finished_at,
            lines: state.lines.clone(),
            truncated: state.truncated,
        }
    }

    fn push_line(&self, line: OutputLine) {
        {
            let mut state = self.state.lock().expect("operation mutex poisoned");
            if state.lines.len() >= MAX_BUFFERED_LINES {
                state.lines.remove(0);
                state.truncated = true;
            }
            state.lines.push(line.clone());
        }
        // A send with no subscribers is not an error: the browser may simply
        // not be watching this operation.
        let _ = self.tx.send(OperationEvent::Line(line));
    }

    fn finish(&self, status: OperationStatus, exit_code: Option<i32>) {
        {
            let mut state = self.state.lock().expect("operation mutex poisoned");
            state.status = status;
            state.exit_code = exit_code;
            state.finished_at = Some(now_unix());
        }
        let _ = self.tx.send(OperationEvent::Finished { status, exit_code });
    }

    pub fn is_running(&self) -> bool {
        self.state.lock().expect("operation mutex poisoned").status == OperationStatus::Running
    }
}

#[derive(Default)]
struct Registry {
    operations: HashMap<String, Arc<Operation>>,
    /// Stack name → id of the operation currently running on it.
    active: HashMap<String, String>,
    /// Insertion order, for eviction of finished operations.
    order: Vec<String>,
}

#[derive(Default)]
pub struct OperationRegistry {
    inner: Mutex<Registry>,
    counter: Mutex<u64>,
}

/// Refused because the stack already has something running (spec §5.2).
#[derive(Debug, thiserror::Error)]
#[error("an operation is already running on this stack")]
pub struct OperationInProgress {
    pub operation_id: String,
}

impl OperationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Id of the operation currently running on `stack`, if any.
    pub fn active_for(&self, stack: &str) -> Option<String> {
        let inner = self.inner.lock().expect("registry mutex poisoned");
        inner.active.get(stack).cloned()
    }

    pub fn get(&self, id: &str) -> Option<Arc<Operation>> {
        let inner = self.inner.lock().expect("registry mutex poisoned");
        inner.operations.get(id).cloned()
    }

    /// Registers a new operation for `stack`, refusing if one is in flight.
    fn create(&self, stack: &str, action: Action) -> Result<Arc<Operation>, OperationInProgress> {
        let mut inner = self.inner.lock().expect("registry mutex poisoned");
        if let Some(existing) = inner.active.get(stack) {
            return Err(OperationInProgress {
                operation_id: existing.clone(),
            });
        }
        let id = self.next_id();
        let (tx, _) = broadcast::channel(256);
        let operation = Arc::new(Operation {
            id: id.clone(),
            stack: stack.to_string(),
            action,
            started_at: now_unix(),
            state: Mutex::new(OperationState {
                status: OperationStatus::Running,
                exit_code: None,
                finished_at: None,
                lines: Vec::new(),
                truncated: false,
            }),
            tx,
        });
        inner.operations.insert(id.clone(), Arc::clone(&operation));
        inner.active.insert(stack.to_string(), id.clone());
        inner.order.push(id);
        Self::evict_finished(&mut inner);
        Ok(operation)
    }

    fn release(&self, stack: &str, id: &str) {
        let mut inner = self.inner.lock().expect("registry mutex poisoned");
        if inner.active.get(stack).map(String::as_str) == Some(id) {
            inner.active.remove(stack);
        }
    }

    fn next_id(&self) -> String {
        let mut counter = self.counter.lock().expect("counter mutex poisoned");
        *counter += 1;
        format!("op-{}-{}", now_unix(), counter)
    }

    fn evict_finished(inner: &mut Registry) {
        while inner.order.len() > MAX_RETAINED_OPERATIONS {
            let Some(position) = inner.order.iter().position(|id| {
                inner
                    .operations
                    .get(id)
                    .map(|op| !op.is_running())
                    .unwrap_or(true)
            }) else {
                // Every retained operation is still running; nothing to evict.
                break;
            };
            let id = inner.order.remove(position);
            inner.operations.remove(&id);
        }
    }

    /// Starts `action` on the stack and returns immediately.
    ///
    /// The steps run sequentially and stop at the first non-zero exit: an
    /// `update` whose `pull` fails must not go on to recreate containers with
    /// the old image and report success.
    pub fn start(
        self: &Arc<Self>,
        stack: String,
        stack_dir: PathBuf,
        compose_file: String,
        action: Action,
    ) -> Result<Arc<Operation>, OperationInProgress> {
        let operation = self.create(&stack, action)?;
        let registry = Arc::clone(self);
        let task_operation = Arc::clone(&operation);
        tokio::spawn(async move {
            let outcome = run_steps(&task_operation, &stack_dir, &compose_file, action).await;
            match outcome {
                Ok(()) => task_operation.finish(OperationStatus::Succeeded, Some(0)),
                Err(code) => task_operation.finish(OperationStatus::Failed, code),
            }
            registry.release(&stack, task_operation.id());
        });
        Ok(operation)
    }
}

/// Runs each Compose step, streaming its output into the operation.
///
/// `Err(exit_code)` on the first failing step.
async fn run_steps(
    operation: &Operation,
    stack_dir: &std::path::Path,
    compose_file: &str,
    action: Action,
) -> Result<(), Option<i32>> {
    for step in action.steps() {
        operation.push_line(OutputLine {
            stream: compose::StreamKind::Stdout,
            text: format!("$ docker {}", compose::argv(compose_file, step).join(" ")),
        });

        let command = compose::command(stack_dir, compose_file, step);
        let mut stream = match compose::spawn_lines(command) {
            Ok(stream) => stream,
            Err(error) => {
                operation.push_line(OutputLine {
                    stream: compose::StreamKind::Stderr,
                    text: format!("failed to run docker compose: {error}"),
                });
                return Err(None);
            }
        };

        while let Some(line) = stream.next_line().await {
            operation.push_line(line);
        }

        match stream.wait().await {
            Ok(Some(0)) => {}
            Ok(code) => {
                operation.push_line(OutputLine {
                    stream: compose::StreamKind::Stderr,
                    text: format!(
                        "docker compose {} exited with code {}",
                        step.join(" "),
                        code.map(|c| c.to_string())
                            .unwrap_or_else(|| "signal".into())
                    ),
                });
                return Err(code);
            }
            Err(error) => {
                operation.push_line(OutputLine {
                    stream: compose::StreamKind::Stderr,
                    text: format!("could not wait for docker compose: {error}"),
                });
                return Err(None);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Arc<OperationRegistry> {
        Arc::new(OperationRegistry::new())
    }

    #[test]
    fn a_stack_can_only_have_one_operation_in_flight() {
        let registry = registry();
        let first = registry.create("octotracker", Action::Start).unwrap();
        let second = registry.create("octotracker", Action::Stop).unwrap_err();
        assert_eq!(second.operation_id, first.id());
    }

    #[test]
    fn other_stacks_are_not_blocked() {
        let registry = registry();
        registry.create("a", Action::Start).unwrap();
        assert!(registry.create("b", Action::Start).is_ok());
    }

    #[test]
    fn releasing_lets_the_next_operation_start() {
        let registry = registry();
        let first = registry.create("a", Action::Start).unwrap();
        first.finish(OperationStatus::Succeeded, Some(0));
        registry.release("a", first.id());
        assert!(registry.create("a", Action::Stop).is_ok());
    }

    #[test]
    fn active_for_reports_the_running_operation() {
        let registry = registry();
        assert_eq!(registry.active_for("a"), None);
        let op = registry.create("a", Action::Update).unwrap();
        assert_eq!(registry.active_for("a").as_deref(), Some(op.id()));
    }

    #[test]
    fn a_snapshot_carries_the_buffered_output() {
        let registry = registry();
        let op = registry.create("a", Action::Start).unwrap();
        op.push_line(OutputLine {
            stream: compose::StreamKind::Stdout,
            text: "Pulling".into(),
        });
        op.finish(OperationStatus::Succeeded, Some(0));

        let snapshot = op.snapshot();
        assert_eq!(snapshot.status, OperationStatus::Succeeded);
        assert_eq!(snapshot.exit_code, Some(0));
        assert_eq!(snapshot.lines.len(), 1);
        assert_eq!(snapshot.lines[0].text, "Pulling");
        assert!(snapshot.finished_at.is_some());
    }

    #[test]
    fn the_line_buffer_is_bounded_and_says_so() {
        let registry = registry();
        let op = registry.create("a", Action::Start).unwrap();
        for index in 0..MAX_BUFFERED_LINES + 10 {
            op.push_line(OutputLine {
                stream: compose::StreamKind::Stdout,
                text: format!("line {index}"),
            });
        }
        let snapshot = op.snapshot();
        assert_eq!(snapshot.lines.len(), MAX_BUFFERED_LINES);
        assert!(snapshot.truncated);
        assert_eq!(snapshot.lines[0].text, "line 10");
    }

    #[tokio::test]
    async fn subscribers_receive_live_lines_and_the_finish_event() {
        let registry = registry();
        let op = registry.create("a", Action::Restart).unwrap();
        let (snapshot, mut receiver) = op.subscribe();
        assert!(snapshot.lines.is_empty());

        op.push_line(OutputLine {
            stream: compose::StreamKind::Stdout,
            text: "Restarting".into(),
        });
        op.finish(OperationStatus::Failed, Some(1));

        match receiver.recv().await.unwrap() {
            OperationEvent::Line(line) => assert_eq!(line.text, "Restarting"),
            other => panic!("expected a line, got {other:?}"),
        }
        match receiver.recv().await.unwrap() {
            OperationEvent::Finished { status, exit_code } => {
                assert_eq!(status, OperationStatus::Failed);
                assert_eq!(exit_code, Some(1));
            }
            other => panic!("expected the finish event, got {other:?}"),
        }
    }

    #[test]
    fn finished_operations_are_evicted_but_running_ones_are_kept() {
        let registry = registry();
        for index in 0..MAX_RETAINED_OPERATIONS + 5 {
            let stack = format!("stack{index}");
            let op = registry.create(&stack, Action::Start).unwrap();
            op.finish(OperationStatus::Succeeded, Some(0));
            registry.release(&stack, op.id());
        }
        let inner = registry.inner.lock().unwrap();
        assert!(inner.operations.len() <= MAX_RETAINED_OPERATIONS);
    }

    #[test]
    fn operation_ids_are_unique() {
        let registry = registry();
        let a = registry.create("a", Action::Start).unwrap();
        let b = registry.create("b", Action::Start).unwrap();
        assert_ne!(a.id(), b.id());
    }
}
