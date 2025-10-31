#![deny(warnings)]

use std::{fs, path::PathBuf};

use base64::{engine::general_purpose, Engine as _};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::error;

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long, default_value = ".")]
    root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
    #[serde(default)]
    id: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(err) => {
                error!(?err, "invalid request");
                continue;
            }
        };
        let response = handle_request(&cli.root, request);
        let serialized = serde_json::to_string(&response)?;
        println!("{}", serialized);
    }

    Ok(())
}

fn handle_request(root: &PathBuf, request: JsonRpcRequest) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(json!({"capabilities": {"resources": true}})),
            error: None,
            id: request.id,
        },
        "resources/list" => {
            let entries = match list_resources(root) {
                Ok(list) => list,
                Err(err) => {
                    return JsonRpcResponse {
                        jsonrpc: "2.0",
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32000,
                            message: err.to_string(),
                        }),
                        id: request.id,
                    };
                }
            };
            JsonRpcResponse {
                jsonrpc: "2.0",
                result: Some(json!({"resources": entries})),
                error: None,
                id: request.id,
            }
        }
        "resources/read" => {
            let uri = request
                .params
                .get("uri")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let decoded = uri.split('/').last().unwrap_or("");
            let path = match general_purpose::STANDARD.decode(decoded) {
                Ok(bytes) => root.join(String::from_utf8_lossy(&bytes).to_string()),
                Err(_) => root.clone(),
            };
            match fs::read_to_string(path) {
                Ok(content) => JsonRpcResponse {
                    jsonrpc: "2.0",
                    result: Some(
                        json!({"contents": [{"mimeType": "text/plain", "text": content}]}),
                    ),
                    error: None,
                    id: request.id,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0",
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32001,
                        message: err.to_string(),
                    }),
                    id: request.id,
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "unknown method".into(),
            }),
            id: request.id,
        },
    }
}

fn list_resources(root: &PathBuf) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut resources = Vec::new();
    for entry in walkdir::WalkDir::new(root).max_depth(1) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let path = entry.path();
            let name = path.file_name().unwrap().to_string_lossy();
            let encoded = general_purpose::STANDARD.encode(path.to_string_lossy().as_bytes());
            resources.push(json!({
                "name": format!("mcp+fs://{}", encoded),
                "description": name,
            }));
        }
    }
    Ok(resources)
}
