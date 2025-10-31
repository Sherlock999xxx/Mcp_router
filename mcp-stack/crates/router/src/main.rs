use std::net::SocketAddr;

use anyhow::Context;
use axum::{
    http::HeaderValue,
    routing::{get, post},
    Router,
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tokio::{net::TcpListener, signal};
use tower_http::{
    cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use mcp_router::{
    auth::{AuthConfig, AuthLayer},
    config::Config,
    metrics,
    router::{handle_rpc, healthz, RouterState},
    subs::SubscriptionStore,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    color_eyre::install().ok();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = fmt::layer().with_target(false).with_level(true);
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();

    metrics::init_metrics();
    let handle = setup_metrics()?;

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/router.toml".into());
    let config = Config::load_from(&config_path).unwrap_or_default();

    let db_url = "sqlite://mcp-router.db";
    let subscriptions = SubscriptionStore::new(db_url).await?;
    let auth_layer = AuthLayer::new(AuthConfig::new(config.server.auth_bearer.clone()));

    let state = RouterState::new(subscriptions, auth_layer.clone()).await;
    state.install_from_config(&config).await?;

    let cors = if config.server.allow_origins.is_empty() {
        CorsLayer::permissive()
    } else {
        let origins: Vec<HeaderValue> = config
            .server
            .allow_origins
            .iter()
            .filter_map(|origin| origin.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods(AllowMethods::any())
            .allow_headers(AllowHeaders::any())
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/mcp", post(handle_rpc))
        .route("/metrics", get(move || async move { handle.render() }))
        .nest_service("/", ServeDir::new("gui"))
        .with_state(state.clone())
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let addr: SocketAddr = config.server.bind.parse().context("parse bind address")?;
    info!(%addr, "starting router");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn setup_metrics() -> anyhow::Result<PrometheusHandle> {
    let builder = PrometheusBuilder::new();
    Ok(builder.install_recorder()?)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install CTRL+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
