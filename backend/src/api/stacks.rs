//! Stack listing, actions and file editing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::compose::status::{self, ServiceStatus, StackStatus};
use crate::compose::{self, Action};
use crate::error::{ApiError, ApiResult};
use crate::stacks::discovery::{self, DiscoveredStack, StackKind, ENV_FILENAME};
use crate::stacks::files::{self, AtomicWrite, DEFAULT_MODE, MAX_EDITABLE_BYTES, SECRET_MODE};
use crate::stacks::paths::{self, PathError};

use super::AppState;

/// Ceiling on a single `docker compose ps`, so one wedged stack cannot hang
/// the whole listing.
const STATUS_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Serialize)]
pub struct StackSummary {
    #[serde(flatten)]
    pub stack: DiscoveredStack,
    pub status: StackStatus,
    /// Set when an action is currently running on this stack, so a reloaded
    /// page can re-attach to the live output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_operation_id: Option<String>,
}

#[derive(Serialize)]
pub struct StackDetail {
    #[serde(flatten)]
    pub summary: StackSummary,
    pub services: Vec<ServiceStatus>,
}

#[derive(Serialize)]
pub struct FileResponse {
    pub filename: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct EnvResponse {
    pub filename: String,
    pub exists: bool,
    pub content: String,
}

#[derive(Deserialize)]
pub struct FileUpdate {
    pub content: String,
}

#[derive(Serialize)]
pub struct OperationAccepted {
    pub operation_id: String,
}

#[derive(Deserialize)]
pub struct LogsQuery {
    pub tail: Option<u32>,
}

#[derive(Serialize)]
pub struct LogsResponse {
    pub lines: Vec<compose::OutputLine>,
}

/// `GET /api/stacks` — rescans the stacks directory on every call (spec §4.1).
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<StackSummary>>> {
    let discovered = discovery::scan(&state.config.stacks_dir).map_err(|error| {
        ApiError::internal(format!("could not scan the stacks directory: {error}"))
    })?;

    let summaries = futures_util::future::join_all(
        discovered
            .into_iter()
            .map(|stack| summarize(state.clone(), stack)),
    )
    .await;

    Ok(Json(summaries))
}

/// `GET /api/stacks/{stack}`
pub async fn detail(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> ApiResult<Json<StackDetail>> {
    let stack = resolve_stack(&state, &name)?;
    let reported = match stack.compose_file() {
        Some(compose_file) => service_statuses(&stack.path, compose_file).await,
        None => None,
    };
    let status = match &reported {
        Some(services) => status::derive(services),
        None => StackStatus::Unknown,
    };
    let active_operation_id = state.operations.active_for(&stack.name);

    Ok(Json(StackDetail {
        summary: StackSummary {
            stack,
            status,
            active_operation_id,
        },
        services: reported.unwrap_or_default(),
    }))
}

pub async fn start(state: State<AppState>, name: AxumPath<String>) -> ApiResult<Response> {
    act(state, name, Action::Start).await
}

pub async fn stop(state: State<AppState>, name: AxumPath<String>) -> ApiResult<Response> {
    act(state, name, Action::Stop).await
}

pub async fn restart(state: State<AppState>, name: AxumPath<String>) -> ApiResult<Response> {
    act(state, name, Action::Restart).await
}

pub async fn update(state: State<AppState>, name: AxumPath<String>) -> ApiResult<Response> {
    act(state, name, Action::Update).await
}

async fn act(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    action: Action,
) -> ApiResult<Response> {
    let stack = resolve_stack(&state, &name)?;
    let compose_file = require_unambiguous(&stack)?.to_string();

    let operation = state
        .operations
        .start(stack.name.clone(), stack.path.clone(), compose_file, action)
        .map_err(|busy| {
            ApiError::Conflict(format!(
                "an operation ({}) is already running on {}",
                busy.operation_id, stack.name
            ))
        })?;

    tracing::info!(stack = %stack.name, action = action.as_str(), operation = operation.id(), "action started");

    Ok((
        StatusCode::ACCEPTED,
        Json(OperationAccepted {
            operation_id: operation.id().to_string(),
        }),
    )
        .into_response())
}

/// `GET /api/stacks/{stack}/compose`
pub async fn read_compose(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> ApiResult<Json<FileResponse>> {
    let stack = resolve_stack(&state, &name)?;
    let compose_file = require_unambiguous(&stack)?.to_string();
    let path = resolve_in_stack(&stack.path, &compose_file)?;
    let content = read_text(&path).await?;
    Ok(Json(FileResponse {
        filename: compose_file,
        content,
    }))
}

/// `PUT /api/stacks/{stack}/compose`
///
/// Spec §4.5 and §12: validate the candidate first, replace atomically, keep
/// the original when validation fails, and never rename the file.
pub async fn write_compose(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<FileUpdate>,
) -> ApiResult<Json<FileResponse>> {
    let stack = resolve_stack(&state, &name)?;
    let compose_file = require_unambiguous(&stack)?.to_string();
    check_size(&body.content)?;
    let path = resolve_in_stack(&stack.path, &compose_file)?;

    let mode = files::mode_of(&path, DEFAULT_MODE);
    let staged = AtomicWrite::stage(&path, &body.content, mode)
        .map_err(|error| ApiError::internal(format!("could not stage the new file: {error}")))?;

    let staged_name = staged
        .staged_path()
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ApiError::internal("staged file has no name"))?
        .to_string();

    let outcome = compose::run(compose::command(
        &stack.path,
        &staged_name,
        &["config", "--quiet"],
    ))
    .await
    .map_err(|error| ApiError::internal(format!("could not run docker compose config: {error}")))?;

    if !outcome.success() {
        // `staged` is dropped here, which removes the candidate. The file on
        // disk is untouched.
        return Err(ApiError::ValidationFailed {
            message: format!("{compose_file} was not saved: docker compose config rejected it"),
            details: sanitize_compose_output(
                &outcome.failure_details(),
                &staged_name,
                &compose_file,
            ),
        });
    }

    staged.commit(Some(".bak"), mode).map_err(|error| {
        ApiError::internal(format!("could not replace {compose_file}: {error}"))
    })?;

    tracing::info!(stack = %stack.name, file = %compose_file, "compose file saved");

    Ok(Json(FileResponse {
        filename: compose_file,
        content: body.content,
    }))
}

/// `GET /api/stacks/{stack}/env`
pub async fn read_env(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> ApiResult<Json<EnvResponse>> {
    let stack = resolve_stack(&state, &name)?;
    if !stack.has_env_file {
        return Err(ApiError::NotFound(format!(
            "{} has no .env file",
            stack.name
        )));
    }
    let path = resolve_in_stack(&stack.path, ENV_FILENAME)?;
    let content = read_text(&path).await?;
    Ok(Json(EnvResponse {
        filename: ENV_FILENAME.to_string(),
        exists: true,
        content,
    }))
}

/// `PUT /api/stacks/{stack}/env`
///
/// The content is written verbatim: `.env` is a text file to this application
/// and is never reinterpreted or normalised (spec §4.6). Nothing about it is
/// logged.
pub async fn write_env(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<FileUpdate>,
) -> ApiResult<Json<EnvResponse>> {
    let stack = resolve_stack(&state, &name)?;
    if !stack.has_env_file {
        return Err(ApiError::NotFound(format!(
            "{} has no .env file",
            stack.name
        )));
    }
    check_size(&body.content)?;
    let path = resolve_in_stack(&stack.path, ENV_FILENAME)?;

    let staged = AtomicWrite::stage(&path, &body.content, SECRET_MODE)
        .map_err(|error| ApiError::internal(format!("could not stage the new .env: {error}")))?;
    staged
        .commit(Some(".bak"), SECRET_MODE)
        .map_err(|error| ApiError::internal(format!("could not replace .env: {error}")))?;

    tracing::info!(stack = %stack.name, "env file saved");

    Ok(Json(EnvResponse {
        filename: ENV_FILENAME.to_string(),
        exists: true,
        content: body.content,
    }))
}

/// `GET /api/stacks/{stack}/logs`
pub async fn logs(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Query(query): Query<LogsQuery>,
) -> ApiResult<Json<LogsResponse>> {
    let stack = resolve_stack(&state, &name)?;
    let compose_file = require_unambiguous(&stack)?.to_string();
    let tail = clamp_tail(query.tail, state.config.log_tail);

    let owned = compose::logs_args(tail, false);
    let args: Vec<&str> = owned.iter().map(String::as_str).collect();

    let outcome = compose::run(compose::command(&stack.path, &compose_file, &args))
        .await
        .map_err(|error| {
            ApiError::internal(format!("could not run docker compose logs: {error}"))
        })?;

    if !outcome.success() {
        return Err(ApiError::ComposeFailed {
            message: format!("could not read the logs of {}", stack.name),
            details: outcome.failure_details(),
        });
    }

    let lines = outcome
        .stdout
        .lines()
        .map(|text| compose::OutputLine {
            stream: compose::StreamKind::Stdout,
            text: text.to_string(),
        })
        .collect();

    Ok(Json(LogsResponse { lines }))
}

/// `GET /api/stacks/{stack}/logs/stream` — SSE (spec §4.4).
///
/// A stopped stack is not an error here: `docker compose logs --follow`
/// returns what exists and the stream simply ends with a `finished` event.
pub async fn logs_stream(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Query(query): Query<LogsQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>> {
    let stack = resolve_stack(&state, &name)?;
    let compose_file = require_unambiguous(&stack)?.to_string();
    let tail = clamp_tail(query.tail, state.config.log_tail);

    let owned = compose::logs_args(tail, true);
    let args: Vec<&str> = owned.iter().map(String::as_str).collect();
    let command = compose::command(&stack.path, &compose_file, &args);
    let mut stream = compose::spawn_lines(command)
        .map_err(|error| ApiError::internal(format!("could not follow the logs: {error}")))?;

    let events = async_stream::stream! {
        while let Some(line) = stream.next_line().await {
            match Event::default().event("line").json_data(&line) {
                Ok(event) => yield Ok(event),
                Err(error) => {
                    tracing::warn!(%error, "could not encode a log line");
                }
            }
        }
        let code = stream.wait().await.ok().flatten();
        let payload = serde_json::json!({ "exit_code": code });
        if let Ok(event) = Event::default().event("finished").json_data(payload) {
            yield Ok(event);
        }
    };

    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

// --- helpers ---------------------------------------------------------------

async fn summarize(state: AppState, stack: DiscoveredStack) -> StackSummary {
    let status = match stack.compose_file() {
        Some(compose_file) => match service_statuses(&stack.path, compose_file).await {
            Some(services) => status::derive(&services),
            None => StackStatus::Unknown,
        },
        None => StackStatus::Unknown,
    };
    let active_operation_id = state.operations.active_for(&stack.name);
    StackSummary {
        stack,
        status,
        active_operation_id,
    }
}

/// `docker compose ps` for one stack.
///
/// `None` means the question could not be answered — an unreachable Docker
/// daemon, a timeout, unparseable output. That is deliberately distinct from
/// `Some(vec![])`, which means Compose answered and there are no containers:
/// a broken socket must read as `Unknown`, never as "nothing created"
/// (spec §4.2). One stack failing never fails the whole listing.
async fn service_statuses(stack_dir: &Path, compose_file: &str) -> Option<Vec<ServiceStatus>> {
    let command = compose::command(
        stack_dir,
        compose_file,
        &["ps", "--all", "--format", "json"],
    );
    let outcome = match tokio::time::timeout(STATUS_TIMEOUT, compose::run(command)).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            tracing::warn!(%error, dir = %stack_dir.display(), "docker compose ps could not run");
            return None;
        }
        Err(_) => {
            tracing::warn!(dir = %stack_dir.display(), "docker compose ps timed out");
            return None;
        }
    };
    if !outcome.success() {
        tracing::warn!(
            dir = %stack_dir.display(),
            details = %outcome.failure_details(),
            "docker compose ps failed"
        );
        return None;
    }
    match status::parse_ps(&outcome.stdout) {
        Ok(services) => Some(services),
        Err(error) => {
            tracing::warn!(%error, "could not parse docker compose ps output");
            None
        }
    }
}

fn resolve_stack(state: &AppState, name: &str) -> ApiResult<DiscoveredStack> {
    let dir = paths::resolve(&state.config.stacks_dir, name).map_err(map_path_error)?;
    discovery::inspect(&dir, name.to_string())
        .ok_or_else(|| ApiError::NotFound(format!("{name} is not a stack")))
}

fn require_unambiguous(stack: &DiscoveredStack) -> ApiResult<&str> {
    match &stack.kind {
        StackKind::Valid { compose_file } => Ok(compose_file),
        StackKind::Ambiguous { compose_files } => Err(ApiError::Conflict(format!(
            "{} contains several Compose files ({}). Leave exactly one and rescan.",
            stack.name,
            compose_files.join(", ")
        ))),
    }
}

fn resolve_in_stack(stack_dir: &Path, filename: &str) -> ApiResult<PathBuf> {
    paths::resolve_file(stack_dir, filename).map_err(map_path_error)
}

fn map_path_error(error: PathError) -> ApiError {
    match error {
        PathError::NotFound | PathError::NotADirectory => {
            ApiError::NotFound("stack not found".into())
        }
        // A traversal attempt is answered as a bad request, never with a hint
        // about what does or does not exist outside the stacks directory.
        other => ApiError::BadRequest(other.to_string()),
    }
}

async fn read_text(path: &Path) -> ApiResult<String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| ApiError::internal(format!("could not stat the file: {error}")))?;
    if metadata.len() as usize > MAX_EDITABLE_BYTES {
        return Err(ApiError::BadRequest(
            "the file is too large to edit from the browser".into(),
        ));
    }
    tokio::fs::read_to_string(path)
        .await
        .map_err(|error| ApiError::internal(format!("could not read the file: {error}")))
}

fn check_size(content: &str) -> ApiResult<()> {
    if content.len() > MAX_EDITABLE_BYTES {
        return Err(ApiError::BadRequest(format!(
            "content exceeds the {MAX_EDITABLE_BYTES} byte limit"
        )));
    }
    Ok(())
}

fn clamp_tail(requested: Option<u32>, default: u32) -> u32 {
    requested.unwrap_or(default).clamp(1, 5000)
}

/// Rewrites the staging filename out of validator output.
///
/// `docker compose config` names the file it was given, which is the internal
/// staging name; showing it would leak an implementation detail into an error
/// the user is meant to act on.
fn sanitize_compose_output(details: &str, staged_name: &str, real_name: &str) -> String {
    details.replace(staged_name, real_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_is_clamped_to_a_sane_range() {
        assert_eq!(clamp_tail(None, 200), 200);
        assert_eq!(clamp_tail(Some(0), 200), 1);
        assert_eq!(clamp_tail(Some(50), 200), 50);
        assert_eq!(clamp_tail(Some(999_999), 200), 5000);
    }

    #[test]
    fn oversized_content_is_rejected() {
        let big = "x".repeat(MAX_EDITABLE_BYTES + 1);
        assert!(check_size(&big).is_err());
        assert!(check_size("services: {}").is_ok());
    }

    #[test]
    fn a_traversal_attempt_is_a_bad_request_not_a_404() {
        let error = map_path_error(PathError::IllegalCharacter);
        assert!(matches!(error, ApiError::BadRequest(_)));
        let error = map_path_error(PathError::Escapes);
        assert!(matches!(error, ApiError::BadRequest(_)));
    }

    #[test]
    fn a_missing_stack_is_a_404() {
        assert!(matches!(
            map_path_error(PathError::NotFound),
            ApiError::NotFound(_)
        ));
    }

    #[test]
    fn ambiguous_stacks_refuse_every_action() {
        let stack = DiscoveredStack {
            name: "confused".into(),
            kind: StackKind::Ambiguous {
                compose_files: vec!["compose.yaml".into(), "compose.yml".into()],
            },
            has_env_file: false,
            path: PathBuf::from("/tmp/confused"),
        };
        let error = require_unambiguous(&stack).unwrap_err();
        assert!(matches!(error, ApiError::Conflict(_)));
    }

    #[test]
    fn the_staging_filename_never_reaches_the_user() {
        let details = "validating .shimau-staged-1-2-compose.yaml: bad indentation";
        let clean =
            sanitize_compose_output(details, ".shimau-staged-1-2-compose.yaml", "compose.yaml");
        assert!(!clean.contains("shimau-staged"));
        assert!(clean.contains("compose.yaml"));
    }
}
