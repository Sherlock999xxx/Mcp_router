use std::{net::SocketAddr, sync::Arc};

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
    auth::AuthConfig,
    config::Config,
    crypto::Encryptor,
    metrics,
    providers::ProviderStore,
    router::{admin_router, handle_rpc, healthz, sse_stream, RouterState},
    subs::SubscriptionStore,
};

#[derive(Parser, Debug)]
#[command(author, version, about = "MCP router service")]
struct Args {
    #[arg(long, default_value = "config/router.toml")]
    config: String,
    #[arg(long)]
    database_url: Option<String>,
}

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

    let args = Args::parse();
    let config = Config::load_from(&args.config).unwrap_or_default();
    let db_url = args
        .database_url
        .as_deref()
        .unwrap_or(&config.database.path)
        .to_string();

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(10)
        .connect(&db_url)
        .await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;

    let subscriptions = SubscriptionStore::new(pool.clone());
    let encryptor = Arc::new(Encryptor::from_env()?);
    let providers = ProviderStore::new(pool.clone(), encryptor);
    let auth_config = Arc::new(AuthConfig::new(config.server.auth_bearer.clone()));

    let bind_addr = config.server.bind.clone();
    let state =
        RouterState::from_config(config, subscriptions, providers, auth_config.clone()).await?;

    let cors = build_cors(state.allowed_origins().as_ref());

    let metrics_handle = handle.clone();
    let static_service = ServeDir::new("gui").append_index_html_on_directories(true);
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/mcp", post(handle_rpc))
        .route("/mcp/stream", get(sse_stream))
        .route(
            "/metrics",
            get(move || async move { metrics_handle.render() }),
        )
        .nest("/api", admin_router())
        .fallback_service(static_service)
        .with_state(state.clone())
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let addr: SocketAddr = bind_addr.parse().context("parse bind address")?;
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

fn build_cors(origins: &[String]) -> CorsLayer {
    use axum::http::{HeaderValue, Method};

    let allow_origin = AllowOrigin::list(
        origins
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin).ok()),
    );
    CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_methods(AllowMethods::list([
            Method::GET,
            Method::POST,
            Method::OPTIONS,
        ]))
        .allow_headers(AllowHeaders::any())
}
