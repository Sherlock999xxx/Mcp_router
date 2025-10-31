#![deny(warnings)]
#![allow(clippy::too_many_arguments)]

mod auth;
mod config;
mod jsonrpc;
mod metrics;
mod router;
mod subs;
mod upstream;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::Context;
use axum::{
    extract::Path,
    http::StatusCode,
    response::Response,
    routing::{get, post},
    Router,
};
use clap::Parser;
use tokio::{fs, net::TcpListener, signal};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{
    config::RouterConfig, metrics::MetricsHandle, router::McpRouter, upstream::UpstreamRegistry,
};

#[derive(Parser, Debug)]
#[command(author, version, about = "MCP Router", long_about = None)]
struct Cli {
    /// Path to router configuration file
    #[arg(long, default_value = "config/router.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    color_eyre::install().ok();
    let cli = Cli::parse();

    init_tracing();

    let config = RouterConfig::load_from_path(&cli.config)
        .with_context(|| format!("failed to load config from {}", cli.config))?;

    let metrics = MetricsHandle::new();
    let db = subs::Database::connect(&config.database.path).await?;
    db.run_migrations().await?;

    let registry = UpstreamRegistry::from_config(&config, metrics.clone()).await?;
    let router = Arc::new(McpRouter::new(
        config.clone(),
        registry.clone(),
        db.clone(),
        metrics.clone(),
    ));

    let metrics_clone = metrics.clone();
    let mut app = Router::new()
        .route("/mcp", post(router::mcp_handler))
        .route("/mcp/stream", get(router::mcp_stream))
        .route("/healthz", get(|| async { axum::response::Html("ok") }))
        .route(
            "/metrics",
            get(move || {
                let metrics = metrics_clone.clone();
                async move { metrics.render() }
            }),
        )
        .route(
            "/api/upstreams",
            get(router::http_list_upstreams).post(router::http_create_upstream),
        )
        .route(
            "/api/providers",
            get(router::http_list_providers).post(router::http_create_provider),
        )
        .route(
            "/api/subscriptions",
            get(router::http_list_subscriptions).post(router::http_create_subscription),
        )
        .route(
            "/api/users",
            get(router::http_list_users).post(router::http_create_user),
        )
        .route("/static/*path", get(serve_static))
        .route("/", get(serve_index))
        .with_state(router.clone());

    if !config.server.allow_origins.is_empty() {
        let mut cors = CorsLayer::new().allow_headers(Any).allow_methods(Any);
        for origin in &config.server.allow_origins {
            if let Ok(origin) = origin.parse::<axum::http::HeaderValue>() {
                cors = cors.allow_origin(origin);
            }
        }
        app = app.layer(cors);
    }

    let app = auth::apply_auth(app, config.server.auth_bearer.clone());

    let addr: SocketAddr = config.server.bind.parse().context("invalid bind address")?;
    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "starting MCP router");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer())
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install terminate handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("shutdown signal received");
}

async fn serve_index() -> Result<Response, (StatusCode, String)> {
    serve_file(PathBuf::from("gui/index.html")).await
}

async fn serve_static(Path(path): Path<String>) -> Result<Response, (StatusCode, String)> {
    let mut full = PathBuf::from("gui");
    let safe_path = path.trim_start_matches('/');
    if safe_path.contains("..") {
        return Err((StatusCode::BAD_REQUEST, "invalid path".into()));
    }
    full.push(safe_path);
    if full.is_dir() {
        full.push("index.html");
    }
    serve_file(full).await
}

async fn serve_file(path: PathBuf) -> Result<Response, (StatusCode, String)> {
    match fs::read(&path).await {
        Ok(bytes) => Ok(Response::builder()
            .status(StatusCode::OK)
            .body(axum::body::Body::from(bytes))
            .unwrap()),
        Err(err) => Err((StatusCode::NOT_FOUND, err.to_string())),
    }
}
