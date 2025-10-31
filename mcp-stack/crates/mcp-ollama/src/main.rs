use std::io::{self, BufRead, Write};

use anyhow::Context;
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:11434/api")]
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
            result: Some(json!({"capabilities": {"tools": true}})),
            error: None,
        },
        "tools/list" => Response {
            jsonrpc: "2.0",
            id: request.id,
            result: Some(json!({"tools": [
                {"name": "ollama/generate", "description": "Generate text from Ollama"}
            ]})),
            error: None,
        },
        "tools/call" => {
            let model = request
                .params
                .get("arguments")
                .and_then(|v| v.get("model"))
                .and_then(|v| v.as_str())
                .unwrap_or("llama2");
            let prompt = request
                .params
                .get("arguments")
                .and_then(|v| v.get("prompt"))
                .and_then(|v| v.as_str())
                .unwrap_or("Hello");
            match client
                .post(format!("{}/generate", args.endpoint))
                .json(&json!({"model": model, "prompt": prompt}))
                .send()
                .await
            {
                Ok(resp) => match resp.text().await {
                    Ok(text) => Response {
                        jsonrpc: "2.0",
                        id: request.id,
                        result: Some(json!({"output": text})),
                        error: None,
                    },
                    Err(err) => error_response(request.id, err),
                },
                Err(err) => error_response(request.id, err),
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

fn error_response(id: serde_json::Value, err: impl std::fmt::Display) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(json!({"code": -32030, "message": err.to_string()})),
    }
}
