use std::net::SocketAddr;

use anyhow::Context;
use axum::{
    routing::{get, post},
    Router,
};
use clap::Parser;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tokio::signal;
use tower_http::{
    cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use mcp_router::{
    admin,
    auth::{AuthConfig, AuthLayer},
    config::Config,
    crypto, metrics,
    router::{handle_rpc, healthz, RouterState},
    sse,
};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "config/router.toml")]
    config: String,
    #[arg(
        long,
        env = "MCP_ROUTER_DATABASE_URL",
        default_value = "sqlite://mcp-router.db"
    )]
    database_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    color_eyre::install().ok();
    let args = Args::parse();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = fmt::layer().with_target(false).with_level(true);
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();

    metrics::init_metrics();
    let handle = setup_metrics()?;

    let config = Config::load_from(&args.config).context("load config")?;
    let key_manager = crypto::global_key_manager()?;
    let database_url = config
        .server
        .database_url
        .clone()
        .unwrap_or_else(|| args.database_url.clone());

    let store = mcp_router::subs::SubscriptionStore::new(&database_url, key_manager).await?;
    let auth_layer = AuthLayer::new(AuthConfig::new(config.server.auth_bearer.clone()));
    let sse_hub = sse::SseHub::new();
    let state = RouterState::new(store.clone(), auth_layer.clone(), sse_hub.clone());
    state.bootstrap(&config).await?;

    let cors = build_cors(&config);

    let api = admin::router(state.clone());

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(move || async move { handle.render() }))
        .route("/mcp", post(handle_rpc))
        .route("/mcp/stream", get(sse::stream))
        .nest("/api", api)
        .nest_service("/static", ServeDir::new("gui/static"))
        .nest_service(
            "/",
            ServeDir::new("gui").append_index_html_on_directories(true),
        )
        .with_state(state.clone())
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let addr: SocketAddr = config.server.bind.parse().context("parse bind address")?;
    info!(%addr, "starting router");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn setup_metrics() -> anyhow::Result<PrometheusHandle> {
    let builder = PrometheusBuilder::new();
    Ok(builder.install_recorder()?)
}

fn build_cors(config: &Config) -> CorsLayer {
    let origins = config
        .server
        .allow_origins
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect::<Vec<_>>();
    let allow_origin = AllowOrigin::list(origins);
    CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_methods(AllowMethods::any())
        .allow_headers(AllowHeaders::any())
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
