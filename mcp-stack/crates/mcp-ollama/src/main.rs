#![deny(warnings)]

use axum::{extract::State, routing::post, Json, Router};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tokio::{net::TcpListener, signal};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:9002")]
    bind: String,
    #[arg(long, default_value = "http://127.0.0.1:11434/api")]
    base_url: String,
}

#[derive(Debug, Deserialize, Clone)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let cli = Cli::parse();
    let addr: SocketAddr = cli.bind.parse()?;

    let state = AppState {
        base_url: cli.base_url,
    };
    let app = Router::new().route("/mcp", post(handle)).with_state(state);

    info!(%addr, "starting mcp-ollama service");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}

#[derive(Clone)]
struct AppState {
    base_url: String,
}

async fn handle(
    State(state): State<AppState>,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    let id = request.id.clone();
    let response = match request.method.as_str() {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(json!({"capabilities": {"tools": true}})),
            error: None,
            id,
        },
        "tools/list" => JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(json!({"tools": [
                {"name": "ollama/generate", "description": "Generate text via Ollama"}
            ]})),
            error: None,
            id,
        },
        "tools/call" => JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(json!({"base_url": state.base_url, "request": request.params})),
            error: None,
            id,
        },
        _ => JsonRpcResponse {
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "unknown method".into(),
            }),
            id,
        },
    };
    Json(response)
}
