use std::io::{self, BufRead, Write};

use anyhow::Context;
use clap::Parser;
use reqwest::{header::HeaderMap, Client};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, env = "ANTHROPIC_API_KEY")]
    api_key: Option<String>,
    #[arg(long, default_value = "https://api.anthropic.com/v1")]
    endpoint: String,
}

#[derive(Debug, Deserialize)]
struct Request {
    jsonrpc: String,
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = Client::builder().build()?;
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line).context("invalid request")?;
        let response = handle_request(&client, &args, request).await;
        let payload = serde_json::to_string(&response)?;
        writeln!(stdout, "{}", payload)?;
        stdout.flush()?;
    }
    Ok(())
}

async fn handle_request(client: &Client, args: &Args, request: Request) -> Response {
    if request.jsonrpc.trim() != "2.0" {
        return Response {
            jsonrpc: "2.0",
            id: request.id,
            result: None,
            error: Some(json!({
                "code": -32600,
                "message": "invalid jsonrpc version",
            })),
        };
    }
    match request.method.as_str() {
        "initialize" => Response {
            jsonrpc: "2.0",
            id: request.id,
            result: Some(json!({"capabilities": {"tools": true}})),
            error: None,
        },
        "tools/list" => Response {
            jsonrpc: "2.0",
            id: request.id,
            result: Some(json!({"tools": [
                {"name": "claude/messages_create", "description": "Call Claude Messages API"}
            ]})),
            error: None,
        },
        "tools/call" => proxy_request(client, args, request, "messages").await,
        _ => Response {
            jsonrpc: "2.0",
            id: request.id,
            result: None,
            error: Some(json!({"code": -32601, "message": "method not found"})),
        },
    }
}

async fn proxy_request(client: &Client, args: &Args, request: Request, path: &str) -> Response {
    let body = request
        .params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    if let Some(key) = args.api_key.as_ref() {
        headers.insert("x-api-key", key.parse().unwrap());
    } else {
        return Response {
            jsonrpc: "2.0",
            id: request.id,
            result: None,
            error: Some(json!({"code": -32051, "message": "missing API key"})),
        };
    }
    headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
    match client
        .post(format!("{}/{}", args.endpoint.trim_end_matches('/'), path))
        .headers(headers)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(json) => Response {
                jsonrpc: "2.0",
                id: request.id,
                result: Some(json!({"response": json})),
                error: None,
            },
            Err(err) => error_response(request.id, err),
        },
        Err(err) => error_response(request.id, err),
    }
}

fn error_response(id: serde_json::Value, err: impl std::fmt::Display) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(json!({"code": -32050, "message": err.to_string()})),
    }
}
