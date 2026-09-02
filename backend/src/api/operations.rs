//! Reading a running (or just-finished) operation.

use std::convert::Infallible;

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_util::stream::Stream;
use tokio::sync::broadcast::error::RecvError;

use crate::error::{ApiError, ApiResult};
use crate::ops::{OperationEvent, OperationSnapshot, OperationStatus};

use super::AppState;

/// `GET /api/operations/{id}` — full transcript, for a client that would
/// rather poll than stream.
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<OperationSnapshot>> {
    let operation = state
        .operations
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("operation {id} is not known")))?;
    Ok(Json(operation.snapshot()))
}

/// `GET /api/operations/{id}/stream` — SSE (spec §5.2).
///
/// The buffered output is replayed first, then live events follow, so a
/// browser that reloads mid-`update` sees the whole transcript. An operation
/// that already finished yields its transcript and closes immediately.
pub async fn stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let operation = state
        .operations
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("operation {id} is not known")))?;

    let (snapshot, mut receiver) = operation.subscribe();
    let already_finished = snapshot.status != OperationStatus::Running;

    let events = async_stream::stream! {
        for line in &snapshot.lines {
            if let Ok(event) = Event::default().event("line").json_data(line) {
                yield Ok(event);
            }
        }

        if already_finished {
            let payload = OperationEvent::Finished {
                status: snapshot.status,
                exit_code: snapshot.exit_code,
            };
            if let Ok(event) = Event::default().event("finished").json_data(payload) {
                yield Ok(event);
            }
            return;
        }

        loop {
            match receiver.recv().await {
                Ok(OperationEvent::Line(line)) => {
                    if let Ok(event) = Event::default().event("line").json_data(&line) {
                        yield Ok(event);
                    }
                }
                Ok(finished @ OperationEvent::Finished { .. }) => {
                    if let Ok(event) = Event::default().event("finished").json_data(&finished) {
                        yield Ok(event);
                    }
                    break;
                }
                // The buffer overflowed: the client missed lines but the
                // operation is still live, so keep following.
                Err(RecvError::Lagged(skipped)) => {
                    let payload = serde_json::json!({ "skipped": skipped });
                    if let Ok(event) = Event::default().event("lagged").json_data(payload) {
                        yield Ok(event);
                    }
                }
                Err(RecvError::Closed) => break,
            }
        }
    };

    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}
