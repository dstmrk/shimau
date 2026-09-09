//! API token management.
//!
//! Every route here takes [`SessionOnly`], so a token can never mint or revoke
//! a token. Without that, `read` would be one call away from `operate` and the
//! capability would be a suggestion.
//!
//! The token itself is returned exactly once, by [`create`]. Nothing else in
//! the application can produce it again: only its SHA-256 was stored.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::token;
use crate::db::{ApiToken, Capability};
use crate::error::{ApiError, ApiResult};

use super::auth::SessionOnly;
use super::AppState;

#[derive(Deserialize)]
pub struct CreateTokenRequest {
    pub label: String,
    pub capability: String,
}

/// The one response that carries a token. The field is named so that it is
/// obvious in a log or a screenshot what has just been exposed.
#[derive(Serialize)]
pub struct CreatedToken {
    #[serde(flatten)]
    pub token: ApiToken,
    /// Shown once. shimau cannot display it again.
    pub secret: String,
}

/// `GET /api/tokens`
pub async fn list(_: SessionOnly, State(state): State<AppState>) -> ApiResult<Json<Vec<ApiToken>>> {
    let tokens = state.db.list_tokens().await.map_err(ApiError::internal)?;
    Ok(Json(tokens))
}

/// `POST /api/tokens`
pub async fn create(
    _: SessionOnly,
    State(state): State<AppState>,
    Json(body): Json<CreateTokenRequest>,
) -> ApiResult<Response> {
    let label = token::normalise_label(&body.label).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "a label is required, at most {} characters, with no control characters",
            token::MAX_LABEL_LEN
        ))
    })?;

    let capability = Capability::parse(&body.capability).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "unknown capability {:?}; expected \"read\" or \"operate\"",
            body.capability
        ))
    })?;

    let secret = token::generate().map_err(ApiError::internal)?;
    let stored = state
        .db
        .create_token(token::hash(&secret), label, capability)
        .await
        .map_err(ApiError::internal)?;

    // The label and the capability, never the token.
    tracing::info!(
        token = stored.id,
        label = %stored.label,
        capability = capability.as_str(),
        "api token created"
    );

    Ok((
        StatusCode::CREATED,
        Json(CreatedToken {
            token: stored,
            secret,
        }),
    )
        .into_response())
}

/// `DELETE /api/tokens/{id}`
///
/// The id is taken as a string and parsed here rather than as `Path<i64>`.
/// Axum's own path rejection answers in plain text, and every other error
/// under `/api` is the JSON body in `crate::error` — a client that parses one
/// shape should not meet a second one because it sent a typo.
pub async fn revoke(
    _: SessionOnly,
    State(state): State<AppState>,
    Path(raw): Path<String>,
) -> ApiResult<StatusCode> {
    let id: i64 = raw
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("{raw:?} is not a token id")))?;
    let removed = state
        .db
        .delete_token(id)
        .await
        .map_err(ApiError::internal)?;
    if !removed {
        return Err(ApiError::NotFound(format!("no token with id {id}")));
    }
    tracing::info!(token = id, "api token revoked");
    Ok(StatusCode::NO_CONTENT)
}
