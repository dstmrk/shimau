//! Login, logout and the session gate.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::{password, ratelimit, session, token};
use crate::db::{now_unix, AdminUser, ApiToken, Capability, TOUCH_INTERVAL_SECS};
use crate::error::{ApiError, ApiResult};

use super::AppState;

/// Who is making a request, once [`require_auth`] has established it.
///
/// The two arms are not equal, and that asymmetry is the point. A session is
/// the administrator in front of a browser: everything the product can do,
/// including editing a Compose file, which is an operation a human should be
/// looking at. A token is a machine, and no capability lets it write a file
/// into a stack directory (see [`Capability`]).
#[derive(Debug, Clone)]
pub enum Principal {
    Session(AdminUser),
    Token(ApiToken),
}

impl Principal {
    /// Whether this principal may run a lifecycle action on a stack.
    pub fn may_operate(&self) -> bool {
        match self {
            Principal::Session(_) => true,
            Principal::Token(token) => token.capability.may_operate(),
        }
    }

    /// Whether this principal may replace a file in a stack directory.
    ///
    /// Only the session. A machine that can write `compose.yaml` and then
    /// start the stack can give a service `privileged: true` and a bind mount
    /// of `/`, which is root on the host in two steps. `docker compose config`
    /// would accept every line of it: it validates syntax, not intent.
    pub fn may_write_files(&self) -> bool {
        matches!(self, Principal::Session(_))
    }

    /// How this principal appears in the log. Never the credential itself.
    pub fn audit(&self) -> String {
        match self {
            Principal::Session(user) => format!("session:{}", user.username),
            Principal::Token(token) => format!("token:{}({})", token.id, token.label),
        }
    }
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct IdentityResponse {
    pub username: String,
    /// The version of the binary answering the request. It travels with the
    /// identity because the browser already fetches that on every load, and it
    /// is read from the crate rather than from a build argument so it cannot
    /// disagree with the code that is running.
    pub version: &'static str,
    /// Which credential answered. A machine client reads this to discover
    /// what it is allowed to do without having to try an action and read a
    /// 403 back.
    pub principal: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<Capability>,
}

impl IdentityResponse {
    fn for_user(username: String) -> Self {
        Self {
            username,
            version: env!("CARGO_PKG_VERSION"),
            principal: "session",
            capability: None,
        }
    }

    fn for_principal(principal: &Principal) -> Self {
        match principal {
            Principal::Session(user) => Self::for_user(user.username.clone()),
            Principal::Token(api_token) => Self {
                username: api_token.label.clone(),
                version: env!("CARGO_PKG_VERSION"),
                principal: "token",
                capability: Some(api_token.capability),
            },
        }
    }
}

/// `POST /api/auth/login`
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> ApiResult<Response> {
    let address = client_address(&state, &headers, peer);
    let limiter_key = ratelimit::key(&address, &body.username);

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

    let authenticated = credentials_ok(&user, &body).map_err(ApiError::internal)?;

    if !authenticated {
        let failures = state.limiter.record_failure(&limiter_key);
        tracing::warn!(
            username = %body.username,
            peer = %address,
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

    tracing::info!(username = %user.username, peer = %address, "login");

    let cookie = session::set_cookie(&token, ttl_secs, state.config.cookie_secure);
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(IdentityResponse::for_user(user.username)),
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
pub async fn me(principal: Principal) -> Json<IdentityResponse> {
    Json(IdentityResponse::for_principal(&principal))
}

/// Rejects unauthenticated requests and attaches the [`Principal`] to the
/// request extensions for the extractors below.
///
/// A cookie wins over a bearer header when a request somehow carries both:
/// the session is the stronger credential, so the choice cannot be used to
/// downgrade a browser request into something a capability check would let
/// through more easily.
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let principal = authenticate(&state, request.headers())
        .await?
        .ok_or(ApiError::Unauthorized)?;

    if let Principal::Token(api_token) = &principal {
        // The row we just read already says when it was last recorded, so the
        // staleness test costs nothing here. Without it every request from a
        // polling client would queue a second write behind the one serialised
        // SQLite connection to change a field by a few seconds. `touch_token`
        // repeats the test in its `WHERE` clause, which is what makes two
        // concurrent requests safe; this only keeps them from asking.
        let stale = api_token
            .last_used_at
            .is_none_or(|last| now_unix() - last >= TOUCH_INTERVAL_SECS);
        if stale {
            if let Err(error) = state.db.touch_token(api_token.id).await {
                // Losing the bookkeeping write must not fail the request it
                // was recording: `last_used_at` is an aid to revoking, not a
                // control.
                tracing::warn!(%error, token = api_token.id, "could not record token use");
            }
        }
    }

    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
}

async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<Principal>, ApiError> {
    if let Some(cookie) = token_from_headers(headers) {
        let user = state
            .db
            .session_user(session::token_hash(cookie))
            .await
            .map_err(ApiError::internal)?;
        if let Some(user) = user {
            return Ok(Some(Principal::Session(user)));
        }
    }

    if let Some(presented) = bearer_from_headers(headers) {
        let api_token = state
            .db
            .token_by_hash(token::hash(presented))
            .await
            .map_err(ApiError::internal)?;
        if let Some(api_token) = api_token {
            return Ok(Some(Principal::Token(api_token)));
        }
    }

    Ok(None)
}

/// Extractor admitting any authenticated principal.
///
/// Absent extensions mean the route was wired without [`require_auth`], which
/// is a bug in the router rather than something a client did. It answers as an
/// internal error, and it fails closed either way.
impl<S: Send + Sync> FromRequestParts<S> for Principal {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Principal>()
            .cloned()
            .ok_or_else(|| ApiError::internal("a route ran without require_auth"))
    }
}

/// Extractor admitting only a principal allowed to run lifecycle actions:
/// the session, or a token with [`Capability::Operate`].
pub struct Operator;

impl<S: Send + Sync> FromRequestParts<S> for Operator {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let principal = Principal::from_request_parts(parts, state).await?;
        if !principal.may_operate() {
            return Err(ApiError::Forbidden(
                "this token is read-only; a token with the operate capability is required".into(),
            ));
        }
        Ok(Operator)
    }
}

/// Extractor admitting only the browser session.
///
/// It guards the two things a machine credential is deliberately not given:
/// writing a file into a stack directory, and minting or revoking tokens. The
/// second matters as much as the first, because a token that could create
/// another token would be its own escalation path out of its capability.
pub struct SessionOnly;

impl<S: Send + Sync> FromRequestParts<S> for SessionOnly {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let principal = Principal::from_request_parts(parts, state).await?;
        if !principal.may_write_files() {
            return Err(ApiError::Forbidden(
                "an API token cannot do this; sign in as the administrator".into(),
            ));
        }
        Ok(SessionOnly)
    }
}

/// Checks a login attempt against the stored account.
///
/// The password is verified *even when the username does not match*, and the
/// two answers are combined with a non-short-circuiting `&`. Written as
/// `username == attempt && verify(...)`, a wrong username returns before
/// Argon2id ever runs, and the microseconds that takes against the tens of
/// milliseconds of a real verification tell an unauthenticated caller which
/// username the administrator uses. The response bodies are identical; the
/// clock was not.
fn credentials_ok(
    user: &AdminUser,
    attempt: &LoginRequest,
) -> Result<bool, password::PasswordError> {
    let password_ok = password::verify(&attempt.password, &user.password_hash)?;
    let username_ok = user.username == attempt.username;
    Ok(username_ok & password_ok)
}

fn token_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(session::token_from_cookie_header)
}

fn bearer_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(token::from_authorization_header)
}

/// The address the login limiter counts failures against.
///
/// Behind a reverse proxy every request arrives from the proxy, which
/// collapses the per-address half of the key: six failures against `admin`
/// from anyone would hold the real administrator out for as long as they kept
/// trying. `SHIMAU_TRUSTED_PROXY_HEADER` names the header carrying the real
/// address, and is unset by default because anyone who can reach shimau
/// directly can also write that header and pick their own key.
///
/// `X-Forwarded-For` accumulates a list as it crosses proxies, and the client
/// is the first entry.
fn client_address(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> String {
    let Some(name) = state.config.trusted_proxy_header.as_deref() else {
        return peer.ip().to_string();
    };
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| peer.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn attempt(username: &str, password: &str) -> LoginRequest {
        LoginRequest {
            username: username.to_string(),
            password: password.to_string(),
        }
    }

    fn account(password_hash: &str) -> AdminUser {
        AdminUser {
            id: 1,
            username: "admin".to_string(),
            password_hash: password_hash.to_string(),
        }
    }

    /// The timing property, asserted without a stopwatch: an unparseable
    /// stored hash makes `verify` return an error, so seeing that error on an
    /// attempt whose *username* is already wrong proves the verification ran.
    /// A short-circuiting check would have returned `Ok(false)` untouched.
    #[test]
    fn the_password_is_verified_even_when_the_username_is_wrong() {
        let user = account("not-a-phc-string");
        assert!(credentials_ok(&user, &attempt("someone-else", "whatever")).is_err());
    }

    #[test]
    fn only_the_right_pair_authenticates() {
        let user = account(&password::hash("correct horse battery staple").unwrap());
        assert!(credentials_ok(&user, &attempt("admin", "correct horse battery staple")).unwrap());
        assert!(!credentials_ok(&user, &attempt("admin", "wrong")).unwrap());
        assert!(!credentials_ok(&user, &attempt("root", "correct horse battery staple")).unwrap());
    }

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
