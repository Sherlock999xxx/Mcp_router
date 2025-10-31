use std::{collections::HashMap, convert::Infallible, sync::Arc, time::Duration};

use anyhow::{anyhow, Context};
use axum::{
    extract::{FromRef, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::{error, instrument};

use crate::{
    auth::{AuthConfig, BearerToken},
    config::{Config, SubscriptionPreset, UpstreamCommand, UpstreamKind},
    jsonrpc::{ErrorObject, Request, Response},
    metrics,
    providers::{ProviderRequest, ProviderStore},
    subs::{EnforcementError, SubscriptionStore, Tier},
    upstream::{UpstreamRegistry, UpstreamSummary},
};

#[derive(Clone)]
pub struct RouterState {
    pub config: Arc<RwLock<Config>>,
    pub registry: Arc<UpstreamRegistry>,
    pub subscriptions: SubscriptionStore,
    pub providers: ProviderStore,
    pub auth: Arc<AuthConfig>,
    allow_origins: Arc<Vec<String>>,
}

impl RouterState {
    pub async fn from_config(
        config: Config,
        subscriptions: SubscriptionStore,
        providers: ProviderStore,
        auth: Arc<AuthConfig>,
    ) -> anyhow::Result<Self> {
        let registry = Arc::new(UpstreamRegistry::new());
        for (name, upstream) in &config.upstreams {
            registry.register(name, upstream.clone()).await?;
        }
        Ok(Self {
            allow_origins: Arc::new(config.server.allow_origins.clone()),
            config: Arc::new(RwLock::new(config)),
            registry,
            subscriptions,
            providers,
            auth,
        })
    }

    pub fn allowed_origins(&self) -> Arc<Vec<String>> {
        self.allow_origins.clone()
    }

    pub async fn add_upstream(
        &self,
        name: String,
        command: UpstreamCommand,
    ) -> anyhow::Result<UpstreamSummary> {
        self.registry.register(&name, command.clone()).await?;
        self.config
            .write()
            .await
            .upstreams
            .insert(name.clone(), command.clone());
        Ok(UpstreamSummary {
            name,
            kind: command.kind,
            command: command.command,
            args: command.args,
            url: command.url,
            bearer: command.bearer.is_some(),
        })
    }

    async fn aggregate_tools(&self) -> Value {
        let mut tools = Vec::new();
        for (server, result) in self.registry.broadcast("tools/list", json!({})).await {
            match result {
                Ok(response) => {
                    if let Some(mut object) =
                        response.result.clone().and_then(|v| v.as_object().cloned())
                    {
                        if let Some(Value::Array(items)) = object.remove("tools") {
                            for item in items {
                                match item {
                                    Value::Object(mut map) => {
                                        let original = map
                                            .get("name")
                                            .and_then(Value::as_str)
                                            .unwrap_or_default();
                                        let local = original
                                            .split_once('/')
                                            .map(|(_, tail)| tail)
                                            .unwrap_or(original);
                                        map.insert(
                                            "name".into(),
                                            Value::String(format!("{}/{}", server, local)),
                                        );
                                        tools.push(Value::Object(map));
                                    }
                                    other => tools.push(other),
                                }
                            }
                        }
                    }
                }
                Err(err) => error!(upstream = %server, ?err, "tools/list aggregation failed"),
            }
        }
        json!({ "tools": tools })
    }

    async fn aggregate_prompts(&self) -> Value {
        let mut prompts = Vec::new();
        for (server, result) in self.registry.broadcast("prompts/list", json!({})).await {
            match result {
                Ok(response) => {
                    if let Some(mut object) =
                        response.result.clone().and_then(|v| v.as_object().cloned())
                    {
                        if let Some(Value::Array(items)) = object.remove("prompts") {
                            for item in items {
                                match item {
                                    Value::Object(mut map) => {
                                        let original = map
                                            .get("name")
                                            .and_then(Value::as_str)
                                            .unwrap_or_default();
                                        let local = original
                                            .split_once('/')
                                            .map(|(_, tail)| tail)
                                            .unwrap_or(original);
                                        map.insert(
                                            "name".into(),
                                            Value::String(format!("{}/{}", server, local)),
                                        );
                                        prompts.push(Value::Object(map));
                                    }
                                    other => prompts.push(other),
                                }
                            }
                        }
                    }
                }
                Err(err) => error!(upstream = %server, ?err, "prompts/list aggregation failed"),
            }
        }
        json!({ "prompts": prompts })
    }

    async fn aggregate_resources(&self) -> Value {
        let mut resources = Vec::new();
        for (server, result) in self.registry.broadcast("resources/list", json!({})).await {
            match result {
                Ok(response) => {
                    if let Some(mut object) =
                        response.result.clone().and_then(|v| v.as_object().cloned())
                    {
                        if let Some(Value::Array(items)) = object.remove("resources") {
                            for item in items {
                                match item {
                                    Value::Object(mut map) => {
                                        if let Some(uri) = map.get("uri").and_then(Value::as_str) {
                                            let encoded = STANDARD.encode(uri);
                                            map.insert(
                                                "uri".into(),
                                                Value::String(format!(
                                                    "mcp+router://{}/{}",
                                                    server, encoded
                                                )),
                                            );
                                        }
                                        resources.push(Value::Object(map));
                                    }
                                    other => resources.push(other),
                                }
                            }
                        }
                    }
                }
                Err(err) => error!(upstream = %server, ?err, "resources/list aggregation failed"),
            }
        }
        json!({ "resources": resources })
    }

    async fn read_resource(&self, uri: &str) -> anyhow::Result<Value> {
        let (server, payload) = uri
            .strip_prefix("mcp+router://")
            .and_then(|rest| rest.split_once('/'))
            .ok_or_else(|| anyhow!("invalid router resource uri"))?;
        let decoded = STANDARD
            .decode(payload)
            .context("decode upstream resource handle")?;
        let original = String::from_utf8(decoded).context("resource uri not utf8")?;
        let response = self
            .registry
            .call(
                server,
                Request::new("resources/read", json!({ "uri": original })),
            )
            .await?;
        response
            .result
            .ok_or_else(|| anyhow!("upstream returned empty result"))
    }

    async fn enforce_subscription(
        &self,
        params: &Value,
        estimated_tokens: i64,
    ) -> Result<Option<String>, EnforcementError> {
        let user_id = params
            .get("user_id")
            .and_then(Value::as_str)
            .map(|value| value.to_string());
        if let Some(user) = &user_id {
            if let Some(record) = self
                .subscriptions
                .get_subscription(user)
                .await
                .map_err(|_| EnforcementError::NoSubscription)?
            {
                record.check_quota(estimated_tokens)?;
                return Ok(user_id);
            }
            return Err(EnforcementError::NoSubscription);
        }
        Ok(None)
    }

    pub async fn handle_jsonrpc(&self, request: Request) -> Response {
        let method = request.method.clone();
        let bytes_in = serde_json::to_vec(&request)
            .map(|v| v.len())
            .unwrap_or_default();
        let started = std::time::Instant::now();
        let response = match method.as_str() {
            "initialize" => {
                let presets = SubscriptionPreset::defaults();
                Response::result(
                    request.id,
                    json!({
                        "capabilities": {
                            "tools": true,
                            "prompts": true,
                            "resources": true,
                        },
                        "subscription_tiers": presets,
                    }),
                )
            }
            "tools/list" => Response::result(request.id, self.aggregate_tools().await),
            "prompts/list" => Response::result(request.id, self.aggregate_prompts().await),
            "resources/list" => Response::result(request.id, self.aggregate_resources().await),
            "prompts/get" => {
                let name = request
                    .params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match split_namespace(name) {
                    Some((server, prompt)) => match self
                        .registry
                        .call(
                            server,
                            Request::new("prompts/get", json!({ "name": prompt })),
                        )
                        .await
                    {
                        Ok(response) => response,
                        Err(err) => Response::error(
                            request.id,
                            ErrorObject::custom(
                                -32011,
                                "upstream prompt fetch failed",
                                Some(json!({"error": err.to_string()})),
                            ),
                        ),
                    },
                    None => Response::error(
                        request.id,
                        ErrorObject::invalid_params("prompt name must be namespaced"),
                    ),
                }
            }
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
                            -32012,
                            "resource read failed",
                            Some(json!({"error": err.to_string()})),
                        ),
                    ),
                }
            }
            "tools/call" => self.handle_tool_call(request).await,
            _ => Response::error(
                request.id,
                ErrorObject::custom(-32601, format!("unknown method: {}", method), None),
            ),
        };
        let bytes_out = serde_json::to_vec(&response)
            .map(|v| v.len())
            .unwrap_or_default();
        let status = if response.error.is_some() {
            "error"
        } else {
            "ok"
        };
        metrics::record_rpc(&method, status, started.elapsed(), bytes_in, bytes_out);
        response
    }

    async fn handle_tool_call(&self, request: Request) -> Response {
        let target = request
            .params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let estimated_tokens = request
            .params
            .get("usage")
            .and_then(|usage| usage.get("tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let user_id = match self
            .enforce_subscription(&request.params, estimated_tokens)
            .await
        {
            Ok(user) => user,
            Err(err) => {
                return Response::error(
                    request.id,
                    ErrorObject::custom(-32020, err.to_string(), None),
                )
            }
        };
        match split_namespace(target) {
            Some((server, name)) => {
                let mut params = request.params.clone();
                params["name"] = Value::String(name.to_string());
                match self
                    .registry
                    .call(server, Request::new("tools/call", params))
                    .await
                {
                    Ok(response) => {
                        if let Some(ref user) = user_id {
                            let tokens = response
                                .result
                                .as_ref()
                                .and_then(|value| value.get("usage"))
                                .and_then(|usage| usage.get("tokens"))
                                .and_then(Value::as_i64)
                                .unwrap_or(estimated_tokens);
                            if let Err(err) =
                                self.subscriptions.record_usage(user, server, tokens).await
                            {
                                error!(?err, user, provider=%server, "failed to record usage");
                            }
                            metrics::record_provider_usage(server, tokens, "ok");
                        }
                        response
                    }
                    Err(err) => {
                        metrics::record_provider_usage(server, 0, "error");
                        Response::error(
                            request.id,
                            ErrorObject::custom(
                                -32001,
                                "upstream error",
                                Some(json!({"error": err.to_string()})),
                            ),
                        )
                    }
                }
            }
            None => Response::error(
                request.id,
                ErrorObject::invalid_params("tool name must be namespaced"),
            ),
        }
    }
}

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn handle_rpc(
    State(state): State<RouterState>,
    BearerToken(_token): BearerToken,
    Json(payload): Json<Request>,
) -> impl IntoResponse {
    Json(state.handle_jsonrpc(payload).await)
}

#[derive(Debug, Deserialize)]
pub struct StreamParams {
    server: String,
    #[serde(flatten)]
    rest: HashMap<String, String>,
}

pub async fn sse_stream(
    State(state): State<RouterState>,
    BearerToken(_token): BearerToken,
    Query(params): Query<StreamParams>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let StreamParams { server, rest } = params;
    let source = state
        .registry
        .event_stream(&server, &rest)
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, err.to_string()))?;
    let stream = source.map(|event| match event {
        Ok(reqwest_eventsource::Event::Open) => Ok(Event::default().comment("stream open")),
        Ok(reqwest_eventsource::Event::Message(msg)) => {
            let mut ev = Event::default();
            if !msg.event.is_empty() {
                ev = ev.event(msg.event.clone());
            }
            if !msg.id.is_empty() {
                ev = ev.id(msg.id.clone());
            }
            ev = ev.data(msg.data);
            Ok(ev)
        }
        Err(err) => Ok(Event::default().event("error").data(err.to_string())),
    });
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep"),
    ))
}

pub fn admin_router() -> Router<RouterState> {
    Router::new()
        .route("/upstreams", get(list_upstreams).post(add_upstream))
        .route("/providers", get(list_providers).post(create_provider))
        .route(
            "/subscriptions",
            get(list_subscriptions).post(set_subscription),
        )
        .route("/users", get(list_users).post(create_user))
}

#[instrument(skip(state))]
async fn list_upstreams(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
) -> impl IntoResponse {
    let upstreams = state.registry.list().await;
    Json(upstreams)
}

#[derive(Debug, Deserialize)]
struct UpstreamCreateRequest {
    name: String,
    kind: UpstreamKind,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    bearer: Option<String>,
}

#[instrument(skip(state, payload))]
async fn add_upstream(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
    Json(payload): Json<UpstreamCreateRequest>,
) -> impl IntoResponse {
    let command = UpstreamCommand {
        kind: payload.kind,
        command: payload.command,
        args: payload.args.unwrap_or_default(),
        url: payload.url,
        bearer: payload.bearer,
    };
    match state.add_upstream(payload.name, command).await {
        Ok(summary) => (StatusCode::CREATED, Json(summary)).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[instrument(skip(state))]
async fn list_providers(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
) -> impl IntoResponse {
    match state.providers.list().await {
        Ok(providers) => Json(providers).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": err.to_string()})),
        )
            .into_response(),
    }
}

#[instrument(skip(state, payload))]
async fn create_provider(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
    Json(payload): Json<ProviderRequest>,
) -> impl IntoResponse {
    match state.providers.upsert(payload).await {
        Ok(record) => (StatusCode::CREATED, Json(record)).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": err.to_string()})),
        )
            .into_response(),
    }
}

#[instrument(skip(state))]
async fn list_users(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
) -> impl IntoResponse {
    match state.subscriptions.list_users().await {
        Ok(users) => Json(users).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": err.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct CreateUserRequest {
    email: String,
}

#[instrument(skip(state, payload))]
async fn create_user(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    match state.subscriptions.create_user(&payload.email).await {
        Ok(user) => (StatusCode::CREATED, Json(user)).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": err.to_string()})),
        )
            .into_response(),
    }
}

#[instrument(skip(state))]
async fn list_subscriptions(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
) -> impl IntoResponse {
    match state.subscriptions.list_subscriptions().await {
        Ok(subs) => Json(subs).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": err.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct SetSubscriptionRequest {
    user_id: String,
    tier: Tier,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
    max_tokens: i64,
    max_requests: i64,
    max_concurrent: i32,
}

#[instrument(skip(state, payload))]
async fn set_subscription(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
    Json(payload): Json<SetSubscriptionRequest>,
) -> impl IntoResponse {
    match state
        .subscriptions
        .set_subscription(
            &payload.user_id,
            payload.tier,
            payload.expires_at,
            payload.max_tokens,
            payload.max_requests,
            payload.max_concurrent,
        )
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": err.to_string()})),
        )
            .into_response(),
    }
}

fn split_namespace(value: &str) -> Option<(&str, &str)> {
    value.split_once('/')
}

impl FromRef<RouterState> for Arc<AuthConfig> {
    fn from_ref(state: &RouterState) -> Arc<AuthConfig> {
        state.auth.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_namespace_parses() {
        assert_eq!(split_namespace("a/b"), Some(("a", "b")));
        assert_eq!(split_namespace("a"), None);
    }

    #[test]
    fn encode_resource_uri() {
        let uri = "file:///tmp/example.txt";
        let encoded = STANDARD.encode(uri);
        assert_eq!(encoded, "ZmlsZTovLy90bXAvZXhhbXBsZS50eHQ=");
    }
}
