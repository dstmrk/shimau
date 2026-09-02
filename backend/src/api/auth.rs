//! Login, logout and the session gate.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use crate::auth::{password, ratelimit, session};
use crate::db::{now_unix, AdminUser};
use crate::error::{ApiError, ApiResult};

use super::AppState;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct IdentityResponse {
    pub username: String,
}

/// `POST /api/auth/login`
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginRequest>,
) -> ApiResult<Response> {
    let limiter_key = ratelimit::key(&peer.ip().to_string(), &body.username);

    if let Some(retry_after_secs) = state.limiter.retry_after(&limiter_key) {
        return Err(ApiError::TooManyRequests { retry_after_secs });
    }

    let user = state
        .db
        .admin_user()
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::Internal("no administrator account exists; check the bootstrap".into())
        })?;

    // The username check runs through the same failure path as a wrong
    // password: the response must not distinguish the two.
    let credentials_ok = user.username == body.username
        && password::verify(&body.password, &user.password_hash).map_err(ApiError::internal)?;

    if !credentials_ok {
        let failures = state.limiter.record_failure(&limiter_key);
        tracing::warn!(
            username = %body.username,
            peer = %peer.ip(),
            failures,
            "failed login"
        );
        return Err(ApiError::Unauthorized);
    }

    state.limiter.reset(&limiter_key);
    // Opportunistic housekeeping: the session table is tiny and this is the
    // only write path that runs often enough to matter.
    if let Err(error) = state.db.purge_expired_sessions().await {
        tracing::warn!(%error, "could not purge expired sessions");
    }

    let token = session::generate_token().map_err(ApiError::internal)?;
    let ttl_secs = state.config.session_ttl_hours * 3600;
    state
        .db
        .insert_session(session::token_hash(&token), user.id, now_unix() + ttl_secs)
        .await
        .map_err(ApiError::internal)?;

    tracing::info!(username = %user.username, peer = %peer.ip(), "login");

    let cookie = session::set_cookie(&token, ttl_secs, state.config.cookie_secure);
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(IdentityResponse {
            username: user.username,
        }),
    )
        .into_response())
}

/// `POST /api/auth/logout`
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(token) = token_from_headers(&headers) {
        state
            .db
            .delete_session(session::token_hash(token))
            .await
            .map_err(ApiError::internal)?;
    }
    let cookie = session::clear_cookie(state.config.cookie_secure);
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response())
}

/// `GET /api/auth/me`
pub async fn me(Extension(user): Extension<AdminUser>) -> Json<IdentityResponse> {
    Json(IdentityResponse {
        username: user.username,
    })
}

/// Rejects unauthenticated requests and attaches the administrator to the
/// request extensions for the handlers that want it.
pub async fn require_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = token_from_headers(request.headers()).ok_or(ApiError::Unauthorized)?;
    let user = state
        .db
        .session_user(session::token_hash(token))
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::Unauthorized)?;

    request.extensions_mut().insert(user);
    Ok(next.run(request).await)
}

fn token_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(session::token_from_cookie_header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn the_token_is_read_from_the_cookie_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; shimau_session=abc"),
        );
        assert_eq!(token_from_headers(&headers), Some("abc"));
    }

    #[test]
    fn a_request_without_a_cookie_has_no_token() {
        assert_eq!(token_from_headers(&HeaderMap::new()), None);
    }

    #[test]
    fn a_bearer_header_is_not_accepted_as_a_session() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc"),
        );
        assert_eq!(token_from_headers(&headers), None);
    }
}
