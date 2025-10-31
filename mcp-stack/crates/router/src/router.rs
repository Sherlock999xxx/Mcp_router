use std::{sync::Arc, time::Instant};

use anyhow::{anyhow, Result as AnyResult};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Map, Value};
use tokio::sync::RwLock;
use tracing::{error, instrument, warn};

use crate::{
    auth::AuthLayer,
    config::Config,
    jsonrpc::{self, ErrorObject, Id, Request, Response},
    metrics,
    subs::{EnforcementError, SubscriptionStore},
    upstream::{DynUpstream, StubUpstream, UpstreamRegistry},
};

#[derive(Clone)]
pub struct RouterState {
    pub registry: Arc<UpstreamRegistry>,
    pub subscriptions: SubscriptionStore,
    pub auth: AuthLayer,
    pub info: Arc<RwLock<Value>>,
}

impl RouterState {
    pub async fn new(subscriptions: SubscriptionStore, auth: AuthLayer) -> Self {
        let registry = Arc::new(UpstreamRegistry::new());
        let info = Arc::new(RwLock::new(json!({
            "capabilities": {
                "tools": true,
                "prompts": true,
                "resources": true,
            },
            "upstreams": [],
        })));
        Self {
            registry,
            subscriptions,
            auth,
            info,
        }
    }

    pub async fn install_stub_upstreams(&self) {
        let stub = Arc::new(StubUpstream {
            name: "stub".into(),
        }) as DynUpstream;
        self.registry.insert("stub", stub).await;
    }

    pub async fn install_from_config(&self, config: &Config) -> AnyResult<()> {
        self.registry.load_from_config(config).await?;
        if self.registry.list_names().await.is_empty() {
            self.install_stub_upstreams().await;
        }
        let names = self.registry.list_names().await;
        let mut info = self.info.write().await;
        *info = json!({
            "capabilities": {
                "tools": true,
                "prompts": true,
                "resources": true,
            },
            "upstreams": names,
        });
        Ok(())
    }

    async fn call_upstream(&self, server: &str, method: &str, params: Value) -> AnyResult<Value> {
        let request = Request {
            jsonrpc: "2.0".to_string(),
            id: Id::None,
            method: method.to_string(),
            params,
        };
        let response = self.registry.call(server, request).await?;
        if let Some(error) = response.error {
            return Err(anyhow!(
                "{}:{} returned error {} {}",
                server,
                method,
                error.code,
                error.message
            ));
        }
        response
            .result
            .ok_or_else(|| anyhow!("{}:{} returned no result", server, method))
    }

    async fn collect_tools(&self) -> AnyResult<Value> {
        let mut tools = Vec::new();
        let upstreams = self.registry.list_names().await;
        for server in upstreams {
            match self
                .call_upstream(&server, "tools/list", Value::Object(Map::new()))
                .await
            {
                Ok(result) => {
                    if let Some(list) = result.get("tools").and_then(Value::as_array) {
                        for tool in list {
                            let mut tool = tool.clone();
                            if let Some(name) = tool.get("name").and_then(Value::as_str) {
                                let namespaced = if name.contains('/') {
                                    name.to_string()
                                } else {
                                    format!("{}/{}", server, name)
                                };
                                tool["name"] = Value::String(namespaced);
                            }
                            tools.push(tool);
                        }
                    }
                }
                Err(err) => {
                    warn!(server = %server, ?err, "failed to collect tools");
                }
            }
        }
        Ok(json!({ "tools": tools }))
    }

    async fn collect_prompts(&self) -> AnyResult<Value> {
        let mut prompts = Vec::new();
        let upstreams = self.registry.list_names().await;
        for server in upstreams {
            match self
                .call_upstream(&server, "prompts/list", Value::Object(Map::new()))
                .await
            {
                Ok(result) => {
                    if let Some(list) = result.get("prompts").and_then(Value::as_array) {
                        for prompt in list {
                            let mut prompt = prompt.clone();
                            if let Some(name) = prompt.get("name").and_then(Value::as_str) {
                                let namespaced = if name.contains('/') {
                                    name.to_string()
                                } else {
                                    format!("{}/{}", server, name)
                                };
                                prompt["name"] = Value::String(namespaced);
                            }
                            prompts.push(prompt);
                        }
                    }
                }
                Err(err) => warn!(server = %server, ?err, "failed to collect prompts"),
            }
        }
        Ok(json!({ "prompts": prompts }))
    }

    async fn collect_resources(&self) -> AnyResult<Value> {
        let mut resources = Vec::new();
        let upstreams = self.registry.list_names().await;
        for server in upstreams {
            match self
                .call_upstream(&server, "resources/list", Value::Object(Map::new()))
                .await
            {
                Ok(result) => {
                    if let Some(list) = result.get("resources").and_then(Value::as_array) {
                        for resource in list {
                            if let Some(uri) = resource.get("uri").and_then(Value::as_str) {
                                let encoded = STANDARD.encode(uri.as_bytes());
                                let router_uri = format!("mcp+router://{}/{}", server, encoded);
                                let mut resource = resource.clone();
                                resource["uri"] = Value::String(router_uri);
                                resources.push(resource);
                            }
                        }
                    }
                }
                Err(err) => warn!(server = %server, ?err, "failed to collect resources"),
            }
        }
        Ok(json!({ "resources": resources }))
    }

    async fn read_resource(&self, uri: &str) -> AnyResult<Value> {
        let (server, raw_uri) = decode_router_uri(uri)?;
        self.call_upstream(&server, "resources/read", json!({ "uri": raw_uri }))
            .await
    }

    async fn fetch_prompt(&self, namespaced: &str) -> AnyResult<Value> {
        let (server, name) = split_namespaced(namespaced)?;
        let mut value = self
            .call_upstream(&server, "prompts/get", json!({ "name": name }))
            .await?;
        if let Some(obj) = value.as_object_mut() {
            obj.entry("name")
                .or_insert_with(|| Value::String(format!("{}/{}", server, name)));
        }
        Ok(value)
    }

    async fn enforce_subscription(&self, user: &str, tokens: i64) -> Result<(), EnforcementError> {
        if let Some(record) = self
            .subscriptions
            .get_subscription(user)
            .await
            .unwrap_or(None)
        {
            record.check_quota(tokens)
        } else {
            Err(EnforcementError::NoSubscription)
        }
    }

    pub async fn handle_jsonrpc(&self, request: Request) -> Response {
        let method = request.method.clone();
        let started = Instant::now();
        let bytes_in = serde_json::to_vec(&request)
            .map(|buf| buf.len())
            .unwrap_or(0);
        let response = match method.as_str() {
            "initialize" => {
                let info = self.info.read().await.clone();
                Response::result(request.id, info)
            }
            "tools/list" => match self.collect_tools().await {
                Ok(value) => Response::result(request.id, value),
                Err(err) => {
                    error!(?err, "failed to aggregate tools");
                    Response::error(
                        request.id,
                        ErrorObject::custom(
                            -32020,
                            "failed to aggregate tools",
                            Some(json!({ "error": err.to_string() })),
                        ),
                    )
                }
            },
            "tools/call" => self.forward_tool_call(request).await,
            "prompts/list" => match self.collect_prompts().await {
                Ok(value) => Response::result(request.id, value),
                Err(err) => {
                    error!(?err, "failed to aggregate prompts");
                    Response::error(
                        request.id,
                        ErrorObject::custom(
                            -32021,
                            "failed to aggregate prompts",
                            Some(json!({ "error": err.to_string() })),
                        ),
                    )
                }
            },
            "prompts/get" => {
                let requested = request
                    .params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match self.fetch_prompt(requested).await {
                    Ok(value) => Response::result(request.id, value),
                    Err(err) => {
                        error!(?err, prompt = requested, "failed to fetch prompt");
                        Response::error(
                            request.id,
                            ErrorObject::custom(
                                -32022,
                                "failed to fetch prompt",
                                Some(json!({ "error": err.to_string() })),
                            ),
                        )
                    }
                }
            }
            "resources/list" => match self.collect_resources().await {
                Ok(value) => Response::result(request.id, value),
                Err(err) => {
                    error!(?err, "failed to aggregate resources");
                    Response::error(
                        request.id,
                        ErrorObject::custom(
                            -32023,
                            "failed to aggregate resources",
                            Some(json!({ "error": err.to_string() })),
                        ),
                    )
                }
            },
            "resources/read" => {
                let uri = request
                    .params
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match self.read_resource(uri).await {
                    Ok(value) => Response::result(request.id, value),
                    Err(err) => Response::error(
                        request.id,
                        ErrorObject::custom(
                            -32024,
                            "resource read failed",
                            Some(json!({ "error": err.to_string() })),
                        ),
                    ),
                }
            }
            _ => jsonrpc::method_not_found(&method),
        };
        let status = if response.error.is_some() {
            "error"
        } else {
            "ok"
        };
        let bytes_out = serde_json::to_vec(&response)
            .map(|buf| buf.len())
            .unwrap_or(0);
        metrics::record_rpc(&method, status, started.elapsed(), bytes_in, bytes_out);
        response
    }

    async fn forward_tool_call(&self, request: Request) -> Response {
        let Some(target) = request.params.get("name").and_then(Value::as_str) else {
            return Response::error(request.id, ErrorObject::invalid_params("missing tool name"));
        };

        let (server, tool_name) = match split_namespaced(target) {
            Ok(parts) => parts,
            Err(err) => {
                return Response::error(
                    request.id,
                    ErrorObject::custom(
                        -32602,
                        "invalid tool name",
                        Some(json!({ "error": err.to_string() })),
                    ),
                )
            }
        };

        let user = request
            .params
            .get("user")
            .and_then(Value::as_str)
            .unwrap_or("anonymous");
        let tokens = extract_token_quota(&request.params);

        if let Err(err) = self.enforce_subscription(user, tokens).await {
            return Response::error(request.id, map_enforcement_error(err));
        }

        let mut forwarded = request.clone();
        match forwarded.params.as_object_mut() {
            Some(map) => {
                map.insert("name".into(), Value::String(tool_name.clone()));
            }
            None => {
                let mut map = Map::new();
                map.insert("name".into(), Value::String(tool_name.clone()));
                forwarded.params = Value::Object(map);
            }
        }

        match self.registry.call(&server, forwarded).await {
            Ok(response) => {
                let outcome = if response.error.is_some() {
                    "error"
                } else {
                    "ok"
                };
                metrics::record_provider_usage(&server, tokens, outcome);
                if outcome == "ok" && tokens > 0 {
                    if let Err(err) = self.subscriptions.record_usage(user, tokens).await {
                        warn!(user = user, ?err, "failed to record usage");
                    }
                }
                response
            }
            Err(err) => {
                metrics::record_provider_usage(&server, tokens, "error");
                error!(server = %server, ?err, "upstream call failed");
                Response::error(
                    request.id,
                    ErrorObject::custom(
                        -32001,
                        "upstream error",
                        Some(json!({ "message": err.to_string() })),
                    ),
                )
            }
        }
    }
}

fn map_enforcement_error(err: EnforcementError) -> ErrorObject {
    match err {
        EnforcementError::NoSubscription => {
            ErrorObject::custom(-32050, "subscription required", None)
        }
        EnforcementError::Expired => ErrorObject::custom(-32051, "subscription expired", None),
        EnforcementError::RequestsExceeded => {
            ErrorObject::custom(-32052, "request quota exceeded", None)
        }
        EnforcementError::TokensExceeded => {
            ErrorObject::custom(-32053, "token quota exceeded", None)
        }
    }
}

fn extract_token_quota(params: &Value) -> i64 {
    params
        .get("tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            params
                .get("usage")
                .and_then(Value::as_object)
                .and_then(|usage| usage.get("tokens"))
                .and_then(Value::as_i64)
        })
        .unwrap_or(0)
}

fn split_namespaced(value: &str) -> AnyResult<(String, String)> {
    let (server, name) = value
        .split_once('/')
        .ok_or_else(|| anyhow!("value must include upstream prefix"))?;
    if server.is_empty() || name.is_empty() {
        return Err(anyhow!("value must include both upstream and name"));
    }
    Ok((server.to_string(), name.to_string()))
}

fn decode_router_uri(uri: &str) -> AnyResult<(String, String)> {
    let remainder = uri
        .strip_prefix("mcp+router://")
        .ok_or_else(|| anyhow!("invalid router resource uri"))?;
    let (server, encoded) = remainder
        .split_once('/')
        .ok_or_else(|| anyhow!("router resource missing upstream"))?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|err| anyhow!("invalid resource encoding: {err}"))?;
    let raw = String::from_utf8(bytes).map_err(|err| anyhow!("resource uri not utf-8: {err}"))?;
    Ok((server.to_string(), raw))
}

#[instrument(skip(state, payload))]
pub async fn handle_rpc(
    State(state): State<RouterState>,
    Json(payload): Json<Request>,
) -> impl IntoResponse {
    Json(state.handle_jsonrpc(payload).await)
}

pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}
