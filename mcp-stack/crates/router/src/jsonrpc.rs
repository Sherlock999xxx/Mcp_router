use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Id {
    Str(String),
    Int(i64),
    None,
}

impl Default for Id {
    fn default() -> Self {
        Id::None
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Request {
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Id,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

fn default_jsonrpc() -> String {
    "2.0".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Response {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl Response {
    pub fn result(id: Id, value: Value) -> Self {
        Self {
            jsonrpc: default_jsonrpc(),
            id,
            result: Some(value),
            error: None,
        }
    }

    pub fn error(id: Id, error: ErrorObject) -> Self {
        Self {
            jsonrpc: default_jsonrpc(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ErrorObject {
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
            data: None,
        }
    }

    pub fn custom(code: i32, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }
}

#[derive(Debug, Error)]
pub enum JsonRpcError {
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("upstream error: {0}")]
    Upstream(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Batch(pub Vec<Request>);

impl Batch {
    pub fn into_response(self, responses: Vec<Response>) -> Value {
        Value::Array(
            responses
                .into_iter()
                .map(|r| serde_json::to_value(r).unwrap())
                .collect(),
        )
    }
}

pub fn ok() -> Response {
    Response::result(Id::None, json!({"ok": true}))
}

pub fn method_not_found(method: &str) -> Response {
    Response::error(
        Id::None,
        ErrorObject::custom(-32601, format!("unknown method: {}", method), None),
    )
}

pub fn invalid_params(msg: impl Into<String>) -> Response {
    Response::error(Id::None, ErrorObject::invalid_params(msg))
}

pub fn parse_headers(headers: &HashMap<String, String>) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect()
}

impl Request {
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: default_jsonrpc(),
            id: Id::Int(0),
            method: method.into(),
            params,
        }
    }
}
