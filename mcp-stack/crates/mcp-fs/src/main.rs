use std::{
    fs,
    io::{self, BufRead, Write},
    path::PathBuf,
};

use anyhow::Context;
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = ".")]
    root: PathBuf,
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

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line).context("invalid request")?;
        let response = handle_request(&args, request);
        let payload = serde_json::to_string(&response)?;
        writeln!(stdout, "{}", payload)?;
        stdout.flush()?;
    }
    Ok(())
}

fn handle_request(args: &Args, request: Request) -> Response {
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
            result: Some(json!({"capabilities": {"resources": true}})),
            error: None,
        },
        "resources/list" => {
            let mut entries = Vec::new();
            for entry in WalkDir::new(&args.root).max_depth(2).into_iter().flatten() {
                if entry.file_type().is_file() {
                    if let Some(path_str) = entry
                        .path()
                        .strip_prefix(&args.root)
                        .ok()
                        .and_then(|p| p.to_str())
                    {
                        let encoded = STANDARD.encode(entry.path().display().to_string());
                        entries.push(json!({
                            "name": path_str,
                            "uri": format!("mcp+fs://{}", encoded),
                        }));
                    }
                }
            }
            Response {
                jsonrpc: "2.0",
                id: request.id,
                result: Some(json!({"resources": entries})),
                error: None,
            }
        }
        "resources/read" => {
            let uri = request
                .params
                .get("uri")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let decoded = uri
                .strip_prefix("mcp+fs://")
                .and_then(|rest| STANDARD.decode(rest).ok())
                .unwrap_or_default();
            let path = String::from_utf8_lossy(&decoded);
            let data = fs::read_to_string(args.root.join(path.to_string()));
            match data {
                Ok(text) => Response {
                    jsonrpc: "2.0",
                    id: request.id,
                    result: Some(json!({"data": text})),
                    error: None,
                },
                Err(err) => Response {
                    jsonrpc: "2.0",
                    id: request.id,
                    result: None,
                    error: Some(json!({"code": -32010, "message": err.to_string()})),
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
