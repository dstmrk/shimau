//! shimau — a tiny, modern Docker Compose manager.
//!
//! The application is a thin, typed layer over the Compose CLI: Compose files
//! stay the source of truth, the filesystem stays the source of truth, and
//! the only state shimau owns is its own administrator account.

use std::net::SocketAddr;
use std::sync::Arc;

use tracing_subscriber::EnvFilter;

use shimau::api::{self, AppState};
use shimau::auth::password;
use shimau::auth::ratelimit::LoginLimiter;
use shimau::config::Config;
use shimau::db::Db;
use shimau::ops::OperationRegistry;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("SHIMAU_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Arc::new(Config::from_env()?);
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        stacks_dir = %config.stacks_dir.display(),
        data_dir = %config.data_dir.display(),
        "starting shimau"
    );

    report_compose_version().await;

    if !config.static_dir.join("index.html").exists() {
        tracing::warn!(
            static_dir = %config.static_dir.display(),
            "no index.html found; the API will serve but the UI will not"
        );
    }

    let db = Db::open(&config.database_path())?;
    bootstrap_admin(&db, &config).await?;
    if let Ok(purged) = db.purge_expired_sessions().await {
        if purged > 0 {
            tracing::info!(purged, "removed expired sessions");
        }
    }

    let state = AppState {
        config: Arc::clone(&config),
        db,
        limiter: Arc::new(LoginLimiter::new()),
        operations: Arc::new(OperationRegistry::new()),
    };

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(address = %config.bind, "listening");

    axum::serve(
        listener,
        api::router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

/// Creates the administrator on first boot from the environment (spec §7.1).
///
/// Once the account exists the bootstrap variables are inert: shimau will not
/// rewrite a password from the environment, because a compose file left with
/// a stale `SHIMAU_ADMIN_PASSWORD` would otherwise silently reset the account
/// on every restart.
async fn bootstrap_admin(db: &Db, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(existing) = db.admin_user().await? {
        if config.admin_password.is_some() {
            tracing::info!(
                username = %existing.username,
                "an administrator already exists; SHIMAU_ADMIN_PASSWORD is ignored"
            );
        }
        return Ok(());
    }

    let Some(plaintext) = config.admin_password.as_deref() else {
        return Err(
            "no administrator account exists yet. Set SHIMAU_ADMIN_PASSWORD (and optionally \
             SHIMAU_ADMIN_USERNAME) and start shimau again — authentication is mandatory, so \
             there is no way in without one."
                .into(),
        );
    };

    if plaintext.chars().count() < password::MIN_PASSWORD_LEN {
        return Err(format!(
            "SHIMAU_ADMIN_PASSWORD must be at least {} characters",
            password::MIN_PASSWORD_LEN
        )
        .into());
    }

    let hash = password::hash(plaintext)?;
    db.create_admin(config.admin_username.clone(), hash).await?;
    tracing::info!(username = %config.admin_username, "administrator account created");
    Ok(())
}

/// Logs the bundled Compose version, and says so loudly when the CLI is
/// missing — every action in the UI depends on it.
async fn report_compose_version() {
    let mut command = tokio::process::Command::new("docker");
    command.args(["compose", "version", "--short"]);
    match command.output().await {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            tracing::info!(version = %version, "docker compose available");
        }
        Ok(output) => {
            tracing::error!(
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "`docker compose version` failed; stack actions will not work"
            );
        }
        Err(error) => {
            tracing::error!(%error, "the docker CLI is not available; stack actions will not work");
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("shutting down");
}
