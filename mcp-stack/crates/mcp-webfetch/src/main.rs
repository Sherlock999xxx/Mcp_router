#![deny(warnings)]

use axum::{routing::post, Json, Router};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tokio::{net::TcpListener, signal};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:9001")]
    bind: String,
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

    let app = Router::new().route("/mcp", post(handle));

    info!(%addr, "starting mcp-webfetch service");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            signal::ctrl_c().await.ok();
        })
        .await?;

    Ok(())
}

async fn handle(Json(request): Json<JsonRpcRequest>) -> Json<JsonRpcResponse> {
    let id = request.id.clone();
    let response = match request.method.as_str() {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(json!({"capabilities": {"tools": true, "prompts": true}})),
            error: None,
            id,
        },
        "tools/list" => JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(json!({"tools": [
                {"name": "webfetch/http_get", "description": "Perform HTTP GET"},
                {"name": "webfetch/http_post_json", "description": "Perform HTTP POST with JSON"}
            ]})),
            error: None,
            id,
        },
        "tools/call" => JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(json!({"status": "queued", "echo": request.params})),
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
