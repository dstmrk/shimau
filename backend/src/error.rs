//! One error type for every API handler.
//!
//! Errors carry an operation-specific message because a bare "operation
//! failed" is useless when the underlying cause is `docker compose pull`
//! exiting 1 (spec §11).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("authentication required")]
    Unauthorized,

    #[error("too many failed login attempts, retry in {retry_after_secs}s")]
    TooManyRequests { retry_after_secs: u64 },

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Conflict(String),

    /// Candidate file content rejected by `docker compose config`. `details`
    /// carries the validator output verbatim so the UI can show it.
    #[error("{message}")]
    ValidationFailed { message: String, details: String },

    /// A Compose command ran and exited non-zero.
    #[error("{message}")]
    ComposeFailed { message: String, details: String },

    #[error("{0}")]
    Internal(String),
}

/// Wire format. `code` is stable and machine-readable; `message` is meant for
/// humans; `details` carries command output when there is any.
#[derive(Serialize)]
pub struct ApiErrorBody {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
}

impl ApiError {
    pub fn internal(context: impl std::fmt::Display) -> Self {
        ApiError::Internal(context.to_string())
    }

    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::TooManyRequests { .. } => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ApiError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            ApiError::ValidationFailed { .. } => {
                (StatusCode::UNPROCESSABLE_ENTITY, "validation_failed")
            }
            ApiError::ComposeFailed { .. } => (StatusCode::BAD_GATEWAY, "compose_failed"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.parts();
        // Internal failures are logged with their context but never echoed to
        // the client: the context can name filesystem paths.
        if let ApiError::Internal(context) = &self {
            tracing::error!(error = %context, "internal error");
        }
        let (message, details) = match &self {
            ApiError::Internal(_) => ("internal error".to_string(), None),
            ApiError::ValidationFailed { message, details }
            | ApiError::ComposeFailed { message, details } => {
                (message.clone(), Some(details.clone()))
            }
            other => (other.to_string(), None),
        };
        let retry_after_secs = match &self {
            ApiError::TooManyRequests { retry_after_secs } => Some(*retry_after_secs),
            _ => None,
        };
        let body = ApiErrorBody {
            code,
            message,
            details,
            retry_after_secs,
        };
        (status, Json(body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_match_variants() {
        assert_eq!(ApiError::Unauthorized.parts().0, StatusCode::UNAUTHORIZED);
        assert_eq!(
            ApiError::NotFound("x".into()).parts().0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            ApiError::Conflict("x".into()).parts().0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            ApiError::ValidationFailed {
                message: "m".into(),
                details: "d".into()
            }
            .parts()
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            ApiError::ComposeFailed {
                message: "m".into(),
                details: "d".into()
            }
            .parts()
            .0,
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn internal_details_never_reach_the_client() {
        let body = ApiError::Internal("/srv/secret/path exploded".into()).into_response();
        assert_eq!(body.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
