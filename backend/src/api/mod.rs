//! HTTP surface.
//!
//! Deliberately small and operation-specific (spec §10). There is no endpoint
//! that takes a command, an image name, or a filesystem path from the client:
//! the only client-supplied identifier is a stack name, and that goes through
//! `stacks::paths`.

pub mod auth;
pub mod operations;
pub mod stacks;

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{middleware, Json, Router};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::auth::ratelimit::LoginLimiter;
use crate::config::Config;
use crate::db::Db;
use crate::ops::OperationRegistry;
use crate::stacks::files::MAX_EDITABLE_BYTES;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Db,
    pub limiter: Arc<LoginLimiter>,
    pub operations: Arc<OperationRegistry>,
}

/// Builds the whole application: API under `/api`, built frontend everywhere
/// else, with unknown non-API paths falling back to `index.html` so the SPA
/// can own its routes.
pub fn router(state: AppState) -> Router {
    let index = state.config.static_dir.join("index.html");
    let assets = ServeDir::new(&state.config.static_dir)
        .append_index_html_on_directories(true)
        .fallback(ServeFile::new(index));

    Router::new()
        .nest("/api", api_router(state.clone()))
        .fallback_service(assets)
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(TraceLayer::new_for_http())
}

fn api_router(state: AppState) -> Router {
    let public = Router::new()
        .route("/health", get(health))
        .route("/auth/login", post(auth::login))
        .with_state(state.clone());

    let protected = Router::new()
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/stacks", get(stacks::list))
        .route("/stacks/{stack}", get(stacks::detail))
        .route("/stacks/{stack}/start", post(stacks::start))
        .route("/stacks/{stack}/stop", post(stacks::stop))
        .route("/stacks/{stack}/restart", post(stacks::restart))
        .route("/stacks/{stack}/update", post(stacks::update))
        .route("/stacks/{stack}/logs", get(stacks::logs))
        .route("/stacks/{stack}/logs/stream", get(stacks::logs_stream))
        .route(
            "/stacks/{stack}/compose",
            get(stacks::read_compose).put(stacks::write_compose),
        )
        .route(
            "/stacks/{stack}/env",
            get(stacks::read_env).put(stacks::write_env),
        )
        .route("/operations/{id}", get(operations::detail))
        .route("/operations/{id}/stream", get(operations::stream))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ))
        .with_state(state);

    Router::new()
        .merge(public)
        .merge(protected)
        // The editors are the only endpoints taking a body, and both take a
        // small text file.
        .layer(DefaultBodyLimit::max(MAX_EDITABLE_BYTES + 4096))
        .fallback(unknown_endpoint)
}

/// Unauthenticated liveness probe, used by the container HEALTHCHECK.
async fn health(State(_state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

/// A 404 inside `/api` must stay JSON: the SPA fallback would otherwise hand
/// `index.html` to a fetch() and turn a typo into a confusing parse error.
async fn unknown_endpoint() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "code": "not_found", "message": "unknown endpoint" })),
    )
        .into_response()
}
