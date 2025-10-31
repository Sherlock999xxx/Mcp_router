use std::{sync::Arc, time::Instant};

use anyhow::{anyhow, Result};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use futures::{stream::FuturesUnordered, StreamExt};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::{error, instrument};
use uuid::Uuid;

use crate::{
    auth::{AuthConfig, AuthLayer, BearerToken},
    config::Config,
    jsonrpc::{self, ErrorObject, Id, Request, Response},
    metrics,
    sse::{RouterEvent, SseHub},
    subs::{EnforcementError, SubscriptionStore},
    upstream::{UpstreamRegistration, UpstreamRegistry},
    util,
};

#[derive(Clone)]
pub struct RouterState {
    pub registry: UpstreamRegistry,
    pub store: SubscriptionStore,
    pub auth: AuthLayer,
    pub sse: SseHub,
    info: Arc<RwLock<Value>>,
}

impl RouterState {
    pub fn new(store: SubscriptionStore, auth: AuthLayer, sse: SseHub) -> Self {
        let info = Arc::new(RwLock::new(json!({
            "capabilities": {
                "tools": true,
                "prompts": true,
                "resources": true,
            },
            "upstreams": [],
        })));
        Self {
            registry: UpstreamRegistry::new(),
            store,
            auth,
            sse,
            info,
        }
    }

    pub async fn bootstrap(&self, config: &Config) -> Result<()> {
        for (name, upstream) in &config.upstreams {
            let registration = UpstreamRegistration {
                name: name.clone(),
                kind: upstream.kind.to_string(),
                command: upstream.command.clone(),
                args: upstream.args.clone(),
                url: upstream.url.clone(),
                bearer: upstream.bearer.clone(),
                provider_slug: upstream.provider_slug.clone(),
            };
            self.registry.register(registration).await?;
        }
        for record in self.store.list_upstreams().await? {
            let registration = UpstreamRegistration {
                name: record.name.clone(),
                kind: record.kind.clone(),
                command: record.command.clone(),
                args: record.args_vec(),
                url: record.url.clone(),
                bearer: record.bearer.clone(),
                provider_slug: record.provider_slug.clone(),
            };
            self.registry.register(registration).await?;
        }
        for provider in &config.providers {
            self.store
                .put_provider(&crate::subs::NewProvider {
                    slug: provider.slug.clone(),
                    display_name: provider.display_name.clone(),
                    description: provider.description.clone(),
                })
                .await?;
        }
        let upstream_info = self.registry.ensure_initialized().await;
        self.update_info(upstream_info).await;
        Ok(())
    }

    async fn update_info(&self, upstream_info: Vec<(String, Value)>) {
        let mut info = self.info.write().await;
        info["upstreams"] = Value::Array(
            upstream_info
                .into_iter()
                .map(|(name, mut value)| {
                    value["name"] = Value::String(name);
                    value
                })
                .collect(),
        );
    }

    async fn aggregate_tools(&self) -> Value {
        let mut stream = FuturesUnordered::new();
        for upstream in self.registry.list() {
            let name = upstream.name().to_string();
            stream.push(async move {
                let request = Request {
                    jsonrpc: "2.0".into(),
                    id: Id::None,
                    method: "tools/list".into(),
                    params: Value::default(),
                };
                match upstream.call(request).await {
                    Ok(response) => response.result.map(|mut value| {
                        if let Some(tools) = value.get_mut("tools").and_then(Value::as_array_mut) {
                            for tool in tools {
                                if let Some(tool_name) = tool.get_mut("name") {
                                    if let Some(inner) = tool_name.as_str() {
                                        *tool_name = Value::String(format!("{}/{}", name, inner));
                                    }
                                }
                            }
                        }
                        value
                    }),
                    Err(err) => {
                        error!(%name, ?err, "tools/list failed");
                        None
                    }
                }
            });
        }
        let mut aggregated = Vec::new();
        while let Some(result) = stream.next().await {
            if let Some(mut value) = result {
                if let Some(array) = value.get_mut("tools").and_then(Value::as_array_mut) {
                    aggregated.append(array);
                }
            }
        }
        json!({ "tools": aggregated })
    }

    async fn aggregate_prompts(&self) -> Value {
        let mut stream = FuturesUnordered::new();
        for upstream in self.registry.list() {
            let name = upstream.name().to_string();
            stream.push(async move {
                let request = Request {
                    jsonrpc: "2.0".into(),
                    id: Id::None,
                    method: "prompts/list".into(),
                    params: Value::default(),
                };
                match upstream.call(request).await {
                    Ok(response) => response.result.map(|mut value| {
                        if let Some(prompts) =
                            value.get_mut("prompts").and_then(Value::as_array_mut)
                        {
                            for prompt in prompts {
                                if let Some(prompt_name) = prompt.get_mut("name") {
                                    if let Some(inner) = prompt_name.as_str() {
                                        *prompt_name = Value::String(format!("{}/{}", name, inner));
                                    }
                                }
                            }
                        }
                        value
                    }),
                    Err(err) => {
                        error!(%name, ?err, "prompts/list failed");
                        None
                    }
                }
            });
        }
        let mut aggregated = Vec::new();
        while let Some(result) = stream.next().await {
            if let Some(mut value) = result {
                if let Some(array) = value.get_mut("prompts").and_then(Value::as_array_mut) {
                    aggregated.append(array);
                }
            }
        }
        json!({ "prompts": aggregated })
    }

    async fn aggregate_resources(&self) -> Value {
        let mut stream = FuturesUnordered::new();
        for upstream in self.registry.list() {
            let name = upstream.name().to_string();
            stream.push(async move {
                let request = Request {
                    jsonrpc: "2.0".into(),
                    id: Id::None,
                    method: "resources/list".into(),
                    params: Value::default(),
                };
                match upstream.call(request).await {
                    Ok(response) => response.result.map(|mut value| {
                        if let Some(resources) =
                            value.get_mut("resources").and_then(Value::as_array_mut)
                        {
                            for resource in resources {
                                if let Some(uri) = resource.get_mut("uri") {
                                    if let Some(inner) = uri.as_str() {
                                        *uri =
                                            Value::String(util::encode_resource_uri(&name, inner));
                                    }
                                }
                            }
                        }
                        value
                    }),
                    Err(err) => {
                        error!(%name, ?err, "resources/list failed");
                        None
                    }
                }
            });
        }
        let mut aggregated = Vec::new();
        while let Some(result) = stream.next().await {
            if let Some(mut value) = result {
                if let Some(array) = value.get_mut("resources").and_then(Value::as_array_mut) {
                    aggregated.append(array);
                }
            }
        }
        json!({ "resources": aggregated })
    }

    async fn read_resource(&self, uri: &str) -> Result<(String, Response)> {
        let decoded =
            util::decode_resource_uri(uri).ok_or_else(|| anyhow!("invalid resource URI"))?;
        let request = Request {
            jsonrpc: "2.0".into(),
            id: Id::None,
            method: "resources/read".into(),
            params: json!({ "uri": decoded.1 }),
        };
        let response = self.registry.call(&decoded.0, request).await?;
        Ok((decoded.0, response))
    }

    async fn enforce_subscription(
        &self,
        user: &str,
        tokens: i64,
    ) -> std::result::Result<(), EnforcementError> {
        if let Some(record) = self
            .store
            .get_subscription(user)
            .await
            .map_err(|_| EnforcementError::NoSubscription)?
        {
            record.check_quota(tokens)
        } else {
            Err(EnforcementError::NoSubscription)
        }
    }

    fn extract_user(params: &Value) -> Option<String> {
        if let Some(user) = params.get("user").and_then(Value::as_str) {
            return Some(user.to_string());
        }
        if let Some(account) = params.get("account").and_then(Value::as_object) {
            if let Some(user) = account.get("user_id").and_then(Value::as_str) {
                return Some(user.to_string());
            }
        }
        None
    }

    fn estimate_tokens(params: &Value) -> i64 {
        params
            .get("usage")
            .and_then(|usage| usage.get("expected_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    }

    async fn publish_stream_completion(&self, stream_id: String, result: Result<Response>) {
        match result {
            Ok(response) => {
                if let Some(result) = response.result.clone() {
                    self.sse.publish(RouterEvent {
                        id: stream_id.clone(),
                        event: "stream-complete".into(),
                        payload: json!({ "result": result }),
                    });
                }
            }
            Err(err) => {
                self.sse.publish(RouterEvent {
                    id: stream_id.clone(),
                    event: "stream-error".into(),
                    payload: json!({ "error": err.to_string() }),
                });
            }
        }
    }

    async fn tools_call(&self, request: &Request) -> Response {
        let target = request
            .params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (server, tool) = target
            .split_once('/')
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .unwrap_or_else(|| ("default".into(), target.to_string()));
        let stream_enabled = request
            .params
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let user = Self::extract_user(&request.params).unwrap_or_else(|| "anonymous".into());
        let estimated_tokens = Self::estimate_tokens(&request.params);
        if let Err(err) = self.enforce_subscription(&user, estimated_tokens).await {
            return Response::error(
                request.id.clone(),
                ErrorObject::custom(
                    -32050,
                    "subscription violation",
                    Some(json!({ "reason": err.to_string() })),
                ),
            );
        }
        let mut upstream_request = request.clone();
        upstream_request.params["name"] = Value::String(tool.clone());
        if stream_enabled {
            let stream_id = Uuid::new_v4().to_string();
            let state = self.clone();
            let upstream = self.registry.clone();
            let req = upstream_request.clone();
            let server_name = server.clone();
            let stream_id_for_task = stream_id.clone();
            self.sse.publish(RouterEvent {
                id: stream_id.clone(),
                event: "stream-start".into(),
                payload: json!({
                    "name": format!("{}/{}", server_name, tool),
                    "user": user,
                }),
            });
            tokio::spawn(async move {
                let result = upstream.call(&server_name, req).await;
                state
                    .publish_stream_completion(stream_id_for_task, result)
                    .await;
            });
            Response::result(request.id.clone(), json!({ "stream": { "id": stream_id } }))
        } else {
            match self.registry.call(&server, upstream_request).await {
                Ok(response) => {
                    if let Some(result) = &response.result {
                        let tokens = result
                            .get("usage")
                            .and_then(|usage| usage.get("total_tokens"))
                            .and_then(Value::as_i64)
                            .unwrap_or(estimated_tokens);
                        if tokens > 0 {
                            let _ = self.store.record_usage(&user, tokens, &server).await;
                            metrics::record_provider_usage(&server, tokens, "ok");
                        }
                    }
                    response
                }
                Err(err) => {
                    metrics::record_provider_usage(&server, estimated_tokens, "error");
                    Response::error(
                        request.id.clone(),
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

    pub async fn handle_jsonrpc(&self, request: Request) -> Response {
        let method = request.method.as_str().to_owned();
        let started = Instant::now();
        let bytes_in = serde_json::to_vec(&request)
            .map(|b| b.len())
            .unwrap_or_default();
        let response = match method.as_str() {
            "initialize" => {
                let info = self.info.read().await.clone();
                Response::result(request.id.clone(), info)
            }
            "tools/list" => Response::result(request.id.clone(), self.aggregate_tools().await),
            "tools/call" => self.tools_call(&request).await,
            "prompts/list" => Response::result(request.id.clone(), self.aggregate_prompts().await),
            "prompts/get" => self.prompt_get(&request).await,
            "resources/list" => {
                Response::result(request.id.clone(), self.aggregate_resources().await)
            }
            "resources/read" => match self
                .read_resource(
                    request
                        .params
                        .get("uri")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
                .await
            {
                Ok((_, response)) => response,
                Err(err) => Response::error(
                    request.id.clone(),
                    ErrorObject::custom(
                        -32010,
                        "resource read failed",
                        Some(json!({ "error": err.to_string() })),
                    ),
                ),
            },
            _ => jsonrpc::method_not_found(&method),
        };
        let bytes_out = serde_json::to_vec(&response)
            .map(|b| b.len())
            .unwrap_or_default();
        let status = if response.error.is_some() {
            "error"
        } else {
            "ok"
        };
        metrics::record_rpc(&method, status, started.elapsed(), bytes_in, bytes_out);
        response
    }

    async fn prompt_get(&self, request: &Request) -> Response {
        let target = request
            .params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (server, prompt) = target
            .split_once('/')
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .unwrap_or_else(|| ("default".into(), target.to_string()));
        let mut upstream_request = request.clone();
        upstream_request.params["name"] = Value::String(prompt);
        match self.registry.call(&server, upstream_request).await {
            Ok(response) => response,
            Err(err) => Response::error(
                request.id.clone(),
                ErrorObject::custom(
                    -32002,
                    "prompt get failed",
                    Some(json!({ "error": err.to_string() })),
                ),
            ),
        }
    }
}

#[instrument(skip(state, payload))]
pub async fn handle_rpc(
    State(state): State<RouterState>,
    BearerToken(_token): BearerToken,
    Json(payload): Json<Request>,
) -> impl IntoResponse {
    Json(state.handle_jsonrpc(payload).await)
}

pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

impl crate::auth::FromRef<RouterState> for Arc<AuthConfig> {
    fn from_ref(input: &RouterState) -> Arc<AuthConfig> {
        input.auth.config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crypto::KeyManager,
        sse::SseHub,
        upstream::{DynUpstream, Upstream},
    };
    use anyhow::{anyhow, Result};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::Arc;

    #[tokio::test]
    async fn aggregates_namespaces_and_resources() -> Result<()> {
        let manager = Arc::new(KeyManager::from_bytes(&[5u8; 32])?);
        let store = SubscriptionStore::new("sqlite::memory:?cache=shared", manager).await?;
        let auth = AuthLayer::new(AuthConfig::new(None));
        let sse = SseHub::new();
        let state = RouterState::new(store, auth, sse);

        let upstream: DynUpstream = Arc::new(DummyUpstream::new());
        state
            .registry
            .register_test("alpha", upstream, Some("demo".into()));

        let tools = state.aggregate_tools().await;
        let tool_names: Vec<_> = tools
            .get("tools")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect();
        assert!(tool_names.contains(&"alpha/echo"));

        let prompts = state.aggregate_prompts().await;
        let prompt_name = prompts["prompts"][0]["name"].as_str().unwrap();
        assert_eq!(prompt_name, "alpha/example");

        let resources = state.aggregate_resources().await;
        let resource_uri = resources["resources"][0]["uri"].as_str().unwrap();
        assert!(resource_uri.starts_with("mcp+router://alpha/"));

        let (server, response) = state.read_resource(resource_uri).await?;
        assert_eq!(server, "alpha");
        let data = response
            .result
            .as_ref()
            .and_then(|value| value.get("data"))
            .and_then(Value::as_str)
            .unwrap();
        assert_eq!(data, "resource:file:///alpha/doc.txt");

        Ok(())
    }

    #[derive(Clone)]
    struct DummyUpstream;

    impl DummyUpstream {
        fn new() -> Self {
            Self
        }
    }

    #[async_trait]
    impl Upstream for DummyUpstream {
        async fn call(&self, request: Request) -> Result<Response> {
            let Request {
                jsonrpc,
                id,
                method,
                params,
            } = request;
            if jsonrpc != "2.0" {
                return Err(anyhow!("invalid jsonrpc version"));
            }
            match method.as_str() {
                "initialize" => Ok(Response::result(
                    id,
                    json!({
                        "capabilities": {
                            "tools": true,
                            "prompts": true,
                            "resources": true,
                        }
                    }),
                )),
                "tools/list" => Ok(Response::result(
                    id,
                    json!({ "tools": [ { "name": "echo", "description": "Echo input" } ] }),
                )),
                "prompts/list" => Ok(Response::result(
                    id,
                    json!({ "prompts": [ { "name": "example", "description": "Sample prompt" } ] }),
                )),
                "resources/list" => Ok(Response::result(
                    id,
                    json!({ "resources": [ { "name": "doc", "uri": "file:///alpha/doc.txt" } ] }),
                )),
                "resources/read" => {
                    let uri = params
                        .get("uri")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    Ok(Response::result(
                        id,
                        json!({ "data": format!("resource:{uri}") }),
                    ))
                }
                other => Err(anyhow!("unexpected method: {other}")),
            }
        }
    }
}
