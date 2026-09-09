//! End-to-end tests over the real router.
//!
//! Everything here goes through the HTTP surface, so the authentication gate,
//! the path checks and the file-editing contract are exercised the way a
//! browser would hit them.
//!
//! The tests that need `docker compose config` (Compose validation) are
//! skipped when the CLI is absent, and run in CI where it is installed.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use http_body_util::BodyExt;
use shimau::api::AppState;
use shimau::auth::password;
use shimau::auth::ratelimit::LoginLimiter;
use shimau::config::Config;
use shimau::db::Db;
use shimau::ops::OperationRegistry;
use tower::ServiceExt;

const PASSWORD: &str = "a-very-long-test-password";

struct Harness {
    router: Router,
    stacks_dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    harness_with(&[]).await
}

async fn harness_with(overrides: &[(&str, &str)]) -> Harness {
    let stacks_dir = tempfile::tempdir().unwrap();
    stack(stacks_dir.path(), "octotracker", "compose.yaml", true);
    stack(stacks_dir.path(), "grafana", "docker-compose.yml", false);

    let config = Config::from_source({
        let dir = stacks_dir.path().to_string_lossy().into_owned();
        let overrides: std::collections::HashMap<String, String> = overrides
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |key| {
            overrides.get(key).cloned().or_else(|| match key {
                "SHIMAU_STACKS_DIR" => Some(dir.clone()),
                "SHIMAU_COOKIE_SECURE" => Some("false".into()),
                _ => None,
            })
        }
    })
    .unwrap();

    let db = Db::open_in_memory().unwrap();
    db.create_admin("admin".into(), password::hash(PASSWORD).unwrap())
        .await
        .unwrap();

    let state = AppState {
        config: Arc::new(config),
        db,
        limiter: Arc::new(LoginLimiter::new()),
        operations: Arc::new(OperationRegistry::new()),
    };

    Harness {
        router: shimau::api::router(state),
        stacks_dir,
    }
}

fn stack(root: &Path, name: &str, compose_file: &str, with_env: bool) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(compose_file),
        "services:\n  web:\n    image: nginx:alpine\n",
    )
    .unwrap();
    if with_env {
        std::fs::write(dir.join(".env"), "TOKEN=secret\n").unwrap();
    }
}

fn request(method: &str, uri: &str) -> axum::http::request::Builder {
    Request::builder().method(method).uri(uri)
}

async fn send_raw(router: &Router, mut request: Request<Body>) -> Response {
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40000))));
    router.clone().oneshot(request).await.unwrap()
}

/// Status and headers, for the assertions that are about the envelope rather
/// than the body.
async fn send_for_headers(router: &Router, request: Request<Body>) -> (StatusCode, HeaderMap) {
    let response = send_raw(router, request).await;
    (response.status(), response.headers().clone())
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Vec<u8>, Vec<String>) {
    let response = send_raw(router, request).await;
    let status = response.status();
    let cookies = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_string))
        .collect();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, body.to_vec(), cookies)
}

async fn login(router: &Router) -> String {
    let body = serde_json::json!({ "username": "admin", "password": PASSWORD });
    let (status, _, cookies) = send(
        router,
        request("POST", "/api/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    cookies
        .first()
        .and_then(|c| c.split(';').next())
        .expect("login must set a session cookie")
        .to_string()
}

/// Mints an API token through the real endpoint and returns the one and only
/// copy of the secret.
async fn mint_token(router: &Router, cookie: &str, capability: &str) -> String {
    let body =
        serde_json::json!({ "label": format!("{capability} client"), "capability": capability });
    let (status, body, _) = send(
        router,
        request("POST", "/api/tokens")
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    json(&body)["secret"]
        .as_str()
        .expect("creation must return the token once")
        .to_string()
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// One directive out of a policy, by name. `script-src-elem` must not answer
/// for `script-src`, so the name is matched whole.
fn directive<'a>(policy: &'a str, name: &str) -> Option<&'a str> {
    policy.split(';').map(str::trim).find_map(|entry| {
        let (key, value) = entry.split_once(' ')?;
        (key == name).then(|| value.trim())
    })
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).expect("response should be JSON")
}

fn docker_compose_available() -> bool {
    std::process::Command::new("docker")
        .args(["compose", "version", "--short"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn health_is_public() {
    let h = harness().await;
    let (status, body, _) = send(
        &h.router,
        request("GET", "/api/health").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["status"], "ok");
}

#[tokio::test]
async fn every_stack_endpoint_requires_a_session() {
    let h = harness().await;
    for (method, uri) in [
        ("GET", "/api/stacks"),
        ("GET", "/api/stacks/octotracker"),
        ("POST", "/api/stacks/octotracker/start"),
        ("POST", "/api/stacks/octotracker/stop"),
        ("POST", "/api/stacks/octotracker/restart"),
        ("POST", "/api/stacks/octotracker/update"),
        ("GET", "/api/stacks/octotracker/compose"),
        ("GET", "/api/stacks/octotracker/env"),
        ("GET", "/api/stacks/octotracker/logs"),
        ("GET", "/api/stacks/octotracker/stats"),
        ("GET", "/api/auth/me"),
        ("GET", "/api/tokens"),
        ("POST", "/api/tokens"),
        ("DELETE", "/api/tokens/1"),
    ] {
        let (status, _, _) =
            send(&h.router, request(method, uri).body(Body::empty()).unwrap()).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{method} {uri} was not gated"
        );
    }
}

#[tokio::test]
async fn a_wrong_password_is_rejected() {
    let h = harness().await;
    let body = serde_json::json!({ "username": "admin", "password": "wrong" });
    let (status, _, cookies) = send(
        &h.router,
        request("POST", "/api/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(cookies.is_empty(), "a failed login must not set a cookie");
}

#[tokio::test]
async fn repeated_failures_are_throttled() {
    let h = harness().await;
    let body = serde_json::json!({ "username": "admin", "password": "wrong" });
    let mut throttled = None;
    for _ in 0..10 {
        let (status, payload, _) = send(
            &h.router,
            request("POST", "/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        if status == StatusCode::TOO_MANY_REQUESTS {
            throttled = Some(json(&payload));
            break;
        }
    }
    let payload = throttled.expect("the limiter should kick in within ten attempts");
    assert_eq!(payload["code"], "rate_limited");
    assert!(payload["retry_after_secs"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn logging_in_lists_the_discovered_stacks() {
    let h = harness().await;
    let cookie = login(&h.router).await;

    let (status, body, _) = send(
        &h.router,
        request("GET", "/api/stacks")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let stacks = json(&body);
    let stacks = stacks.as_array().unwrap();
    assert_eq!(stacks.len(), 2);
    assert_eq!(stacks[0]["name"], "grafana");
    assert_eq!(stacks[0]["compose_file"], "docker-compose.yml");
    assert_eq!(stacks[0]["has_env_file"], false);
    assert_eq!(stacks[1]["name"], "octotracker");
    assert_eq!(stacks[1]["has_env_file"], true);
}

#[tokio::test]
async fn identity_carries_the_running_version() {
    // The header shows this number and every support question starts with it,
    // so it has to come from the binary that is actually serving the request.
    // Both shapes matter: the browser takes its identity from the login
    // response on sign-in and from `/me` on reload.
    let h = harness().await;

    let body = serde_json::json!({ "username": "admin", "password": PASSWORD });
    let (status, login_body, cookies) = send(
        &h.router,
        request("POST", "/api/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&login_body)["version"], env!("CARGO_PKG_VERSION"));

    let cookie = cookies
        .first()
        .and_then(|c| c.split(';').next())
        .expect("login must set a session cookie")
        .to_string();

    let (status, me_body, _) = send(
        &h.router,
        request("GET", "/api/auth/me")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let me_body = json(&me_body);
    assert_eq!(me_body["username"], "admin");
    assert_eq!(me_body["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn logout_invalidates_the_session() {
    let h = harness().await;
    let cookie = login(&h.router).await;

    let (status, _, _) = send(
        &h.router,
        request("POST", "/api/auth/logout")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _) = send(
        &h.router,
        request("GET", "/api/auth/me")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn path_traversal_is_rejected_at_the_http_boundary() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    std::fs::write(h.stacks_dir.path().join("..").join("outside.txt"), "x").ok();

    for uri in [
        "/api/stacks/..%2f..%2fetc",
        "/api/stacks/.%2e",
        "/api/stacks/.env",
        "/api/stacks/%2fetc%2fpasswd",
    ] {
        let (status, _, _) = send(
            &h.router,
            request("GET", uri)
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
            "{uri} returned {status}"
        );
    }
}

#[tokio::test]
async fn reading_the_compose_file_preserves_its_name() {
    let h = harness().await;
    let cookie = login(&h.router).await;

    let (status, body, _) = send(
        &h.router,
        request("GET", "/api/stacks/grafana/compose")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["filename"], "docker-compose.yml");
    assert!(json(&body)["content"].as_str().unwrap().contains("nginx"));
}

#[tokio::test]
async fn a_stack_without_an_env_file_reports_404() {
    let h = harness().await;
    let cookie = login(&h.router).await;

    let (status, _, _) = send(
        &h.router,
        request("GET", "/api/stacks/grafana/env")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_env_file_round_trips_and_keeps_a_backup() {
    let h = harness().await;
    let cookie = login(&h.router).await;

    let (status, body, _) = send(
        &h.router,
        request("GET", "/api/stacks/octotracker/env")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["content"], "TOKEN=secret\n");

    let update = serde_json::json!({ "content": "TOKEN=rotated\n" });
    let (status, _, _) = send(
        &h.router,
        request("PUT", "/api/stacks/octotracker/env")
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(update.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let dir = h.stacks_dir.path().join("octotracker");
    assert_eq!(
        std::fs::read_to_string(dir.join(".env")).unwrap(),
        "TOKEN=rotated\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join(".env.bak")).unwrap(),
        "TOKEN=secret\n"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for file in [".env", ".env.bak"] {
            let mode = std::fs::metadata(dir.join(file))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{file} has mode {mode:o}");
        }
    }
}

#[tokio::test]
async fn an_ambiguous_stack_refuses_actions() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    let dir = h.stacks_dir.path().join("confused");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("compose.yaml"), "services: {}\n").unwrap();
    std::fs::write(dir.join("docker-compose.yml"), "services: {}\n").unwrap();

    let (status, body, _) = send(
        &h.router,
        request("POST", "/api/stacks/confused/start")
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(json(&body)["message"]
        .as_str()
        .unwrap()
        .contains("several Compose files"));
}

#[tokio::test]
async fn an_unknown_api_route_answers_with_json() {
    let h = harness().await;
    let (status, body, _) = send(
        &h.router,
        request("GET", "/api/nope").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["code"], "not_found");
}

#[tokio::test]
async fn invalid_compose_content_cannot_overwrite_the_file() {
    if !docker_compose_available() {
        eprintln!("skipping: the docker compose CLI is not available");
        return;
    }
    let h = harness().await;
    let cookie = login(&h.router).await;
    let file = h.stacks_dir.path().join("grafana/docker-compose.yml");
    let original = std::fs::read_to_string(&file).unwrap();

    let update = serde_json::json!({ "content": "services:\n  web:\n    image: [1,2\n" });
    let (status, body, _) = send(
        &h.router,
        request("PUT", "/api/stacks/grafana/compose")
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(update.to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let payload = json(&body);
    assert_eq!(payload["code"], "validation_failed");
    assert!(!payload["details"].as_str().unwrap().is_empty());
    assert!(!payload["details"]
        .as_str()
        .unwrap()
        .contains("shimau-staged"));

    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    let leftovers: Vec<_> = std::fs::read_dir(h.stacks_dir.path().join("grafana"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".shimau-staged"))
        .collect();
    assert!(leftovers.is_empty(), "left behind {leftovers:?}");
}

#[tokio::test]
async fn valid_compose_content_is_saved_with_a_backup() {
    if !docker_compose_available() {
        eprintln!("skipping: the docker compose CLI is not available");
        return;
    }
    let h = harness().await;
    let cookie = login(&h.router).await;
    let dir = h.stacks_dir.path().join("grafana");

    let new_content = "services:\n  web:\n    image: nginx:1.27-alpine\n";
    let update = serde_json::json!({ "content": new_content });
    let (status, _, _) = send(
        &h.router,
        request("PUT", "/api/stacks/grafana/compose")
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(update.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        std::fs::read_to_string(dir.join("docker-compose.yml")).unwrap(),
        new_content
    );
    assert!(dir.join("docker-compose.yml.bak").exists());
    // The filename must not drift towards compose.yaml (spec §4.5).
    assert!(!dir.join("compose.yaml").exists());
}

#[tokio::test]
async fn a_compose_file_cannot_be_written_through_a_symlink() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    let outside = tempfile::NamedTempFile::new().unwrap();
    let dir = h.stacks_dir.path().join("linked");
    std::fs::create_dir_all(&dir).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), dir.join("compose.yaml")).unwrap();

    let (status, _, _) = send(
        &h.router,
        request("GET", "/api/stacks/linked/compose")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_responses_are_never_cached() {
    let harness = harness().await;
    let cookie = login(&harness.router).await;

    // The `.env` body is the one that matters: it is a secret answered over a
    // plain GET, and a shared cache holding on to it is a leak.
    for uri in [
        "/api/auth/me",
        "/api/stacks",
        "/api/stacks/octotracker/env",
        "/api/stacks/octotracker/compose",
    ] {
        let (status, headers) = send_for_headers(
            &harness.router,
            request("GET", uri)
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert_eq!(
            headers.get(header::CACHE_CONTROL).unwrap(),
            "no-store",
            "{uri} may not be cached"
        );
    }
}

#[tokio::test]
async fn a_throttled_login_carries_a_retry_after_header() {
    let harness = harness().await;
    let body = serde_json::json!({ "username": "admin", "password": "wrong-password" });

    let mut throttled = None;
    for _ in 0..8 {
        let (status, headers) = send_for_headers(
            &harness.router,
            request("POST", "/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        if status == StatusCode::TOO_MANY_REQUESTS {
            throttled = Some(headers);
            break;
        }
    }

    let headers = throttled.expect("repeated failures must eventually throttle");
    let seconds: u64 = headers
        .get(header::RETRY_AFTER)
        .expect("a 429 must say when to come back")
        .to_str()
        .unwrap()
        .parse()
        .expect("Retry-After is a number of seconds");
    assert!(seconds > 0);
}

#[tokio::test]
async fn every_response_carries_the_content_security_policy() {
    let harness = harness().await;

    let (status, headers) = send_for_headers(
        &harness.router,
        request("GET", "/api/health").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let policy = headers
        .get(header::CONTENT_SECURITY_POLICY)
        .expect("every response carries a policy")
        .to_str()
        .unwrap();

    // The directive worth defending. `'unsafe-inline'` is granted to styles
    // and to nothing else; `'unsafe-eval'` to nothing at all.
    assert_eq!(directive(policy, "script-src"), Some("'self'"));
    assert!(!policy.contains("unsafe-eval"), "{policy}");
    assert_eq!(directive(policy, "default-src"), Some("'self'"));
    assert_eq!(directive(policy, "object-src"), Some("'none'"));
    assert_eq!(directive(policy, "base-uri"), Some("'none'"));
    assert_eq!(directive(policy, "frame-ancestors"), Some("'none'"));
    assert_eq!(
        directive(policy, "style-src"),
        Some("'self' 'unsafe-inline'")
    );

    // The policy is applied outside the /api nest, because the document it
    // governs is served by the static side of the router.
    let (_, headers) = send_for_headers(
        &harness.router,
        request("GET", "/index.html").body(Body::empty()).unwrap(),
    )
    .await;
    assert!(headers.contains_key(header::CONTENT_SECURITY_POLICY));
}

// --- API tokens ------------------------------------------------------------
//
// The capability is the whole point of a machine credential, so each of these
// asserts one edge of it. Together they say: a token reads, `operate` also
// acts, and neither one ever writes a file or touches another token.

#[tokio::test]
async fn a_read_token_can_read_but_cannot_act() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    let token = mint_token(&h.router, &cookie, "read").await;

    let (status, body, _) = send(
        &h.router,
        request("GET", "/api/stacks")
            .header(header::AUTHORIZATION, bearer(&token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body).as_array().unwrap().len(), 2);

    for action in ["start", "stop", "restart", "update"] {
        let (status, payload, _) = send(
            &h.router,
            request("POST", &format!("/api/stacks/octotracker/{action}"))
                .header(header::AUTHORIZATION, bearer(&token))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{action} was allowed");
        assert_eq!(json(&payload)["code"], "forbidden");
    }
}

#[tokio::test]
async fn an_operate_token_can_act() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    let token = mint_token(&h.router, &cookie, "operate").await;

    let (status, body, _) = send(
        &h.router,
        request("POST", "/api/stacks/octotracker/stop")
            .header(header::AUTHORIZATION, bearer(&token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(json(&body)["operation_id"].is_string());
}

/// The reason no capability writes a file: a Compose file the machine controls
/// plus a start it can trigger is root on the host, and `docker compose config`
/// would validate every line of it.
#[tokio::test]
async fn no_token_can_write_a_stack_file() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    let update = serde_json::json!({ "content": "services:\n  x:\n    image: alpine\n" });

    for capability in ["read", "operate"] {
        let token = mint_token(&h.router, &cookie, capability).await;
        for file in ["compose", "env"] {
            let (status, payload, _) = send(
                &h.router,
                request("PUT", &format!("/api/stacks/octotracker/{file}"))
                    .header(header::AUTHORIZATION, bearer(&token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(update.to_string()))
                    .unwrap(),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "a {capability} token wrote {file}"
            );
            assert_eq!(json(&payload)["code"], "forbidden");
        }
    }

    // The file on disk is the one the administrator left there.
    let compose = h.stacks_dir.path().join("octotracker/compose.yaml");
    assert!(std::fs::read_to_string(compose).unwrap().contains("nginx"));
}

/// A token that could mint a token would be its own escalation path out of
/// its capability, so the token surface is session-only.
#[tokio::test]
async fn no_token_can_reach_the_token_surface() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    let token = mint_token(&h.router, &cookie, "operate").await;
    let body = serde_json::json!({ "label": "escalated", "capability": "operate" });

    for (method, uri, payload) in [
        ("GET", "/api/tokens", None),
        ("POST", "/api/tokens", Some(body.to_string())),
        ("DELETE", "/api/tokens/1", None),
    ] {
        let (status, _, _) = send(
            &h.router,
            request(method, uri)
                .header(header::AUTHORIZATION, bearer(&token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(payload.map(Body::from).unwrap_or_else(Body::empty))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri} was allowed");
    }
}

#[tokio::test]
async fn a_revoked_token_authenticates_nothing() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    let token = mint_token(&h.router, &cookie, "read").await;

    let (status, body, _) = send(
        &h.router,
        request("GET", "/api/tokens")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = json(&body)[0]["id"].as_i64().unwrap();

    let (status, _, _) = send(
        &h.router,
        request("DELETE", &format!("/api/tokens/{id}"))
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _) = send(
        &h.router,
        request("GET", "/api/stacks")
            .header(header::AUTHORIZATION, bearer(&token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_malformed_or_unknown_bearer_is_unauthorized() {
    let h = harness().await;
    for value in [
        "Bearer shimau_notarealtoken",
        "Bearer ",
        "Basic shimau_abc",
        "shimau_abc",
        "",
    ] {
        let (status, _, _) = send(
            &h.router,
            request("GET", "/api/stacks")
                .header(header::AUTHORIZATION, value)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{value:?} authenticated");
    }
}

/// The token is shown once, at creation. Nothing else can produce it again,
/// because only its SHA-256 was written.
#[tokio::test]
async fn the_token_is_returned_once_and_never_listed() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    let token = mint_token(&h.router, &cookie, "read").await;
    assert!(token.starts_with("shimau_"));

    let (status, body, _) = send(
        &h.router,
        request("GET", "/api/tokens")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let listed = String::from_utf8(body).unwrap();
    assert!(
        !listed.contains(&token),
        "the token came back in the listing: {listed}"
    );
    assert!(!listed.contains("secret"), "listing: {listed}");
    assert!(listed.contains("read"));
}

#[tokio::test]
async fn a_token_needs_a_label_and_a_known_capability() {
    let h = harness().await;
    let cookie = login(&h.router).await;

    for body in [
        serde_json::json!({ "label": "", "capability": "read" }),
        serde_json::json!({ "label": "   ", "capability": "read" }),
        serde_json::json!({ "label": "ci\nrunner", "capability": "read" }),
        serde_json::json!({ "label": "ok", "capability": "administer" }),
        serde_json::json!({ "label": "ok", "capability": "" }),
    ] {
        let (status, _, _) = send(
            &h.router,
            request("POST", "/api/tokens")
                .header(header::COOKIE, &cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body} was accepted");
    }
}

#[tokio::test]
async fn identity_reports_which_credential_answered() {
    let h = harness().await;
    let cookie = login(&h.router).await;

    let (_, body, _) = send(
        &h.router,
        request("GET", "/api/auth/me")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(json(&body)["principal"], "session");
    assert!(json(&body)["capability"].is_null());

    let token = mint_token(&h.router, &cookie, "read").await;
    let (_, body, _) = send(
        &h.router,
        request("GET", "/api/auth/me")
            .header(header::AUTHORIZATION, bearer(&token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(json(&body)["principal"], "token");
    assert_eq!(json(&body)["capability"], "read");
}

/// A request carrying both credentials is the administrator's browser, and it
/// must not be downgradable into the weaker one by attaching a header.
#[tokio::test]
async fn the_session_wins_when_both_credentials_are_present() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    let token = mint_token(&h.router, &cookie, "read").await;

    let (status, body, _) = send(
        &h.router,
        request("GET", "/api/auth/me")
            .header(header::COOKIE, &cookie)
            .header(header::AUTHORIZATION, bearer(&token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["principal"], "session");
}

// --- .env creation ---------------------------------------------------------

#[tokio::test]
async fn an_env_file_can_be_created_where_there_was_none() {
    let h = harness().await;
    let cookie = login(&h.router).await;
    let env = h.stacks_dir.path().join("grafana/.env");
    assert!(!env.exists());

    let update = serde_json::json!({ "content": "PORT=3000\n" });
    let (status, body, _) = send(
        &h.router,
        request("PUT", "/api/stacks/grafana/env")
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(update.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["exists"], true);
    assert_eq!(std::fs::read_to_string(&env).unwrap(), "PORT=3000\n");
    assert!(
        !h.stacks_dir.path().join("grafana/.env.bak").exists(),
        "there was nothing to back up"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&env).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "a created .env must not be world-readable");
    }

    // It now reads back, where before the write it was a 404.
    let (status, _, _) = send(
        &h.router,
        request("GET", "/api/stacks/grafana/env")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// --- the login limiter behind a proxy --------------------------------------

async fn failed_login(router: &Router, header_value: Option<(&str, &str)>) -> StatusCode {
    let body = serde_json::json!({ "username": "admin", "password": "wrong" });
    let mut builder =
        request("POST", "/api/auth/login").header(header::CONTENT_TYPE, "application/json");
    if let Some((name, value)) = header_value {
        builder = builder.header(name, value);
    }
    let (status, _, _) = send(router, builder.body(Body::from(body.to_string())).unwrap()).await;
    status
}

/// Every request behind a tunnel arrives from the same socket address, which
/// collapses the per-address half of the limiter key: one attacker guessing
/// `admin` would hold the real administrator out. With the header trusted, the
/// two callers are counted apart again.
#[tokio::test]
async fn a_trusted_proxy_header_keys_the_limiter_per_client() {
    let h = harness_with(&[("SHIMAU_TRUSTED_PROXY_HEADER", "X-Forwarded-For")]).await;

    let mut throttled = false;
    for _ in 0..10 {
        if failed_login(&h.router, Some(("X-Forwarded-For", "203.0.113.9"))).await
            == StatusCode::TOO_MANY_REQUESTS
        {
            throttled = true;
            break;
        }
    }
    assert!(throttled, "the attacker should have been throttled");

    assert_eq!(
        failed_login(&h.router, Some(("X-Forwarded-For", "198.51.100.4"))).await,
        StatusCode::UNAUTHORIZED,
        "a different client must not inherit the attacker's backoff"
    );
}

/// And the default is not to trust it, because anyone who can reach shimau
/// directly can also write the header and pick their own key.
#[tokio::test]
async fn the_proxy_header_is_ignored_unless_it_is_configured() {
    let h = harness().await;

    let mut throttled = false;
    for _ in 0..10 {
        if failed_login(&h.router, Some(("X-Forwarded-For", "203.0.113.9"))).await
            == StatusCode::TOO_MANY_REQUESTS
        {
            throttled = true;
            break;
        }
    }
    assert!(throttled);

    assert_eq!(
        failed_login(&h.router, Some(("X-Forwarded-For", "198.51.100.4"))).await,
        StatusCode::TOO_MANY_REQUESTS,
        "changing the header must not reset the counter when it is not trusted"
    );
}

/// Every error under `/api` is the same JSON body. A malformed token id must
/// not be the one place a client meets axum's plain-text path rejection.
#[tokio::test]
async fn a_malformed_token_id_answers_in_the_usual_shape() {
    let h = harness().await;
    let cookie = login(&h.router).await;

    let (status, body, _) = send(
        &h.router,
        request("DELETE", "/api/tokens/not-a-number")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["code"], "bad_request");
}
