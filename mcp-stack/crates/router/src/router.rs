use std::{sync::Arc, time::Instant};

use axum::response::sse::{Event, Sse};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use base64::{engine::general_purpose, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::time::{self, Duration};
use tokio_stream::{wrappers::IntervalStream, StreamExt};

use crate::{
    config::{RouterConfig, UpstreamConfig},
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    metrics::MetricsHandle,
    subs::Database,
    upstream::UpstreamRegistry,
};

#[derive(Clone)]
pub struct McpRouter {
    #[allow(dead_code)]
    config: RouterConfig,
    registry: UpstreamRegistry,
    pub db: Database,
    metrics: MetricsHandle,
}

impl McpRouter {
    pub fn new(
        config: RouterConfig,
        registry: UpstreamRegistry,
        db: Database,
        metrics: MetricsHandle,
    ) -> Self {
        Self {
            config,
            registry,
            db,
            metrics,
        }
    }

    async fn handle_jsonrpc(self: Arc<Self>, request: JsonRpcRequest) -> JsonRpcResponse {
        let start = Instant::now();
        let method = request.method.clone();
        let response = match request.method.as_str() {
            "initialize" => self.handle_initialize(request.clone()).await,
            "tools/list" => self.handle_tools_list(request.clone()).await,
            "prompts/list" => self.handle_prompts_list(request.clone()).await,
            "prompts/get" => self.handle_prompts_get(request.clone()).await,
            "resources/list" => self.handle_resources_list(request.clone()).await,
            "resources/read" => self.handle_resources_read(request.clone()).await,
            "tools/call" => self.handle_tools_call(request.clone()).await,
            _ => JsonRpcResponse::error(request.id.clone(), -32601, "unknown method"),
        };
        let elapsed = start.elapsed().as_secs_f64();
        self.metrics.record_call(
            &method,
            if response.error.is_some() {
                "error"
            } else {
                "ok"
            },
        );
        self.metrics.observe_latency(&method, elapsed);
        response
    }

    async fn handle_initialize(self: &Arc<Self>, request: JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse::success(
            request.id,
            json!({
                "protocolVersion": "2.0",
                "capabilities": {
                    "tools": true,
                    "prompts": true,
                    "resources": true,
                },
                "upstreams": self.registry.list().await.into_iter().map(|(name, desc)| {
                    json!({
                        "name": name,
                        "config": desc,
                    })
                }).collect::<Vec<_>>()
            }),
        )
    }

    async fn handle_tools_list(self: &Arc<Self>, request: JsonRpcRequest) -> JsonRpcResponse {
        let mut tools = Vec::new();
        for (name, _) in self.registry.list().await {
            tools.push(json!({
                "name": format!("{}/echo", name),
                "description": "Echoes the payload back for testing",
            }));
        }
        JsonRpcResponse::success(request.id, json!({"tools": tools}))
    }

    async fn handle_prompts_list(self: &Arc<Self>, request: JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse::success(
            request.id,
            json!({"prompts": [{"name": "default", "description": "Sample prompt"}]}),
        )
    }

    async fn handle_prompts_get(self: &Arc<Self>, request: JsonRpcRequest) -> JsonRpcResponse {
        let name = request
            .params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("default");
        JsonRpcResponse::success(
            request.id,
            json!({
                "prompt": {
                    "name": name,
                    "messages": [
                        {"role": "system", "content": "You are connected to the MCP router."}
                    ]
                }
            }),
        )
    }

    async fn handle_resources_list(self: &Arc<Self>, request: JsonRpcRequest) -> JsonRpcResponse {
        let resources: Vec<Value> = self
            .registry
            .list()
            .await
            .into_iter()
            .map(|(name, _)| {
                json!({
                    "name": format!(
                        "mcp+router://{}/{}",
                        name,
                        general_purpose::STANDARD.encode("root")
                    ),
                    "description": "Router managed resource",
                })
            })
            .collect();
        JsonRpcResponse::success(request.id, json!({"resources": resources}))
    }

    async fn handle_resources_read(self: &Arc<Self>, request: JsonRpcRequest) -> JsonRpcResponse {
        let uri = request
            .params
            .get("uri")
            .and_then(Value::as_str)
            .unwrap_or("");
        JsonRpcResponse::success(
            request.id,
            json!({"uri": uri, "contents": [{"mimeType": "text/plain", "text": "Demo resource"}]}),
        )
    }

    async fn handle_tools_call(self: &Arc<Self>, request: JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse::success(request.id, json!({"echo": request.params}))
    }
}

pub async fn mcp_handler(
    State(router): State<Arc<McpRouter>>,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    Json(router.handle_jsonrpc(request).await)
}

pub async fn mcp_stream(State(_router): State<Arc<McpRouter>>) -> impl IntoResponse {
    let interval = IntervalStream::new(time::interval(Duration::from_secs(5))).map(|_| {
        Ok::<Event, std::convert::Infallible>(Event::default().data("{\"event\":\"heartbeat\"}"))
    });
    Sse::new(interval)
}

#[derive(Debug, Deserialize)]
pub struct CreateUpstreamRequest {
    pub name: String,
    pub upstream: UpstreamInput,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum UpstreamInput {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Http {
        url: String,
        #[serde(default)]
        bearer: Option<String>,
    },
}

impl From<UpstreamInput> for UpstreamConfig {
    fn from(value: UpstreamInput) -> Self {
        match value {
            UpstreamInput::Stdio { command, args } => UpstreamConfig::Stdio {
                command,
                args,
                env: Default::default(),
            },
            UpstreamInput::Http { url, bearer } => UpstreamConfig::Http { url, bearer },
        }
    }
}

pub async fn http_list_upstreams(State(router): State<Arc<McpRouter>>) -> impl IntoResponse {
    let items = router.registry.list().await;
    Json(json!({"upstreams": items}))
}

pub async fn http_create_upstream(
    State(router): State<Arc<McpRouter>>,
    Json(payload): Json<CreateUpstreamRequest>,
) -> impl IntoResponse {
    router
        .registry
        .add(payload.name.clone(), payload.upstream.into())
        .await;
    StatusCode::CREATED
}

#[derive(Debug, Deserialize)]
pub struct CreateProviderRequest {
    pub name: String,
    pub kind: String,
}

pub async fn http_list_providers(State(router): State<Arc<McpRouter>>) -> impl IntoResponse {
    match router.db.list_providers().await {
        Ok(items) => Json(json!({"providers": items})).into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

pub async fn http_create_provider(
    State(router): State<Arc<McpRouter>>,
    Json(payload): Json<CreateProviderRequest>,
) -> impl IntoResponse {
    match router.db.create_provider(payload.name, payload.kind).await {
        Ok(provider) => (StatusCode::CREATED, Json(provider)).into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateSubscriptionRequest {
    pub user_id: String,
    pub tier: String,
    pub expires_at: Option<String>,
}

pub async fn http_list_subscriptions(State(router): State<Arc<McpRouter>>) -> impl IntoResponse {
    match router.db.list_subscriptions().await {
        Ok(items) => Json(json!({"subscriptions": items})).into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

pub async fn http_create_subscription(
    State(router): State<Arc<McpRouter>>,
    Json(payload): Json<CreateSubscriptionRequest>,
) -> impl IntoResponse {
    let expires = match payload.expires_at.as_deref() {
        Some(ts) => chrono::DateTime::parse_from_rfc3339(ts)
            .ok()
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        None => None,
    };
    match router
        .db
        .create_subscription(payload.user_id, payload.tier, expires)
        .await
    {
        Ok(sub) => (StatusCode::CREATED, Json(sub)).into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub display_name: Option<String>,
}

pub async fn http_list_users(State(router): State<Arc<McpRouter>>) -> impl IntoResponse {
    match router.db.list_users().await {
        Ok(items) => Json(json!({"users": items})).into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

pub async fn http_create_user(
    State(router): State<Arc<McpRouter>>,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    match router
        .db
        .create_user(payload.email, payload.display_name)
        .await
    {
        Ok(user) => (StatusCode::CREATED, Json(user)).into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}
