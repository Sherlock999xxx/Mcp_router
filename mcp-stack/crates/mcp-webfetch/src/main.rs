use std::io::{self, BufRead, Write};

use anyhow::Context;
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "Mozilla/5.0 (MCP-Webfetch)")]
    user_agent: String,
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
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let client = Client::builder().user_agent(args.user_agent).build()?;
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line).context("invalid request")?;
        let response = handle_request(&client, request).await;
        let payload = serde_json::to_string(&response)?;
        writeln!(stdout, "{}", payload)?;
        stdout.flush()?;
    }
    Ok(())
}

async fn handle_request(client: &Client, request: Request) -> Response {
    if request.jsonrpc != "2.0" {
        return Response {
            jsonrpc: "2.0",
            id: request.id,
            result: None,
            error: Some(json!({
                "code": -32600,
                "message": "invalid jsonrpc version"
            })),
        };
    }
    match request.method.as_str() {
        "initialize" => Response {
            jsonrpc: "2.0",
            id: request.id,
            result: Some(json!({"capabilities": {"tools": true, "prompts": true}})),
            error: None,
        },
        "tools/list" => Response {
            jsonrpc: "2.0",
            id: request.id,
            result: Some(json!({"tools": [
                {"name": "webfetch/http_get", "description": "HTTP GET request"},
                {"name": "webfetch/http_post_json", "description": "HTTP POST with JSON body"}
            ]})),
            error: None,
        },
        "prompts/list" => Response {
            jsonrpc: "2.0",
            id: request.id,
            result: Some(json!({"prompts": [
                {"name": "webfetch/example", "description": "Example prompt"}
            ]})),
            error: None,
        },
        "prompts/get" => Response {
            jsonrpc: "2.0",
            id: request.id,
            result: Some(json!({
                "name": "webfetch/example",
                "prompt": "Use webfetch/http_get to retrieve content."
            })),
            error: None,
        },
        "tools/call" => {
            let name = request
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            match name {
                "webfetch/http_get" => {
                    let url = request
                        .params
                        .get("arguments")
                        .and_then(|v| v.get("url"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("https://example.com");
                    match client.get(url).send().await {
                        Ok(resp) => match resp.text().await {
                            Ok(text) => Response {
                                jsonrpc: "2.0",
                                id: request.id,
                                result: Some(json!({"status": "ok", "body": text})),
                                error: None,
                            },
                            Err(err) => http_error(request.id, err),
                        },
                        Err(err) => http_error(request.id, err),
                    }
                }
                "webfetch/http_post_json" => {
                    let url = request
                        .params
                        .get("arguments")
                        .and_then(|v| v.get("url"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("https://example.com");
                    let body = request
                        .params
                        .get("arguments")
                        .and_then(|v| v.get("body"))
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    match client.post(url).json(&body).send().await {
                        Ok(resp) => match resp.text().await {
                            Ok(text) => Response {
                                jsonrpc: "2.0",
                                id: request.id,
                                result: Some(json!({"status": "ok", "body": text})),
                                error: None,
                            },
                            Err(err) => http_error(request.id, err),
                        },
                        Err(err) => http_error(request.id, err),
                    }
                }
                _ => Response {
                    jsonrpc: "2.0",
                    id: request.id,
                    result: None,
                    error: Some(json!({"code": -32601, "message": "unknown tool"})),
                },
            }
        }
        _ => Response {
            jsonrpc: "2.0",
            id: request.id,
            result: None,
            error: Some(json!({"code": -32601, "message": "method not found"})),
        },
    }
}

fn http_error(id: serde_json::Value, err: impl std::fmt::Display) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(json!({"code": -32020, "message": err.to_string()})),
    }
}
