use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    routing::post,
    Router,
};
use hyper::body::to_bytes;
use mcp_router::{
    admin,
    auth::{AuthConfig, AuthLayer},
    crypto::KeyManager,
    router::{handle_rpc, RouterState},
    sse::SseHub,
    subs::SubscriptionStore,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "secret-token";

#[tokio::test]
async fn admin_endpoints_flow() -> anyhow::Result<()> {
    let key_manager = Arc::new(KeyManager::from_bytes(&[11u8; 32])?);
    let store = SubscriptionStore::new("sqlite::memory:?cache=shared", key_manager.clone()).await?;
    let auth = AuthLayer::new(AuthConfig::new(Some(ADMIN_TOKEN.to_string())));
    let sse = SseHub::new();
    let state = RouterState::new(store.clone(), auth.clone(), sse);

    let app = Router::new()
        .nest("/api", admin::router(state.clone()))
        .route("/mcp", post(handle_rpc))
        .with_state(state.clone());

    let unauthorized = send(&app, Request::builder().uri("/api/upstreams").body(Body::empty())?).await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let provider_resp = send(
        &app,
        json_request(
            Method::POST,
            "/api/providers",
            Some(ADMIN_TOKEN),
            json!({
                "slug": "openai",
                "display_name": "OpenAI",
                "description": "LLM provider",
            }),
        )?,
    )
    .await;
    assert_eq!(provider_resp.status(), StatusCode::OK);
    let provider: Value = read_json(provider_resp).await;
    assert_eq!(provider["slug"], "openai");

    let key_resp = send(
        &app,
        json_request(
            Method::POST,
            "/api/providers/keys",
            Some(ADMIN_TOKEN),
            json!({
                "provider_slug": "openai",
                "name": "api_key",
                "value": "sk-test",
            }),
        )?,
    )
    .await;
    assert_eq!(key_resp.status(), StatusCode::NO_CONTENT);

    let user_resp = send(
        &app,
        json_request(
            Method::POST,
            "/api/users",
            Some(ADMIN_TOKEN),
            json!({
                "email": "admin@example.com",
                "name": "Admin",
            }),
        )?,
    )
    .await;
    assert_eq!(user_resp.status(), StatusCode::OK);
    let user: Value = read_json(user_resp).await;
    let user_id = user["id"].as_str().unwrap().to_string();

    let subscription_resp = send(
        &app,
        json_request(
            Method::POST,
            "/api/subscriptions",
            Some(ADMIN_TOKEN),
            json!({
                "user_id": user_id,
                "tier": "pro",
            }),
        )?,
    )
    .await;
    assert_eq!(subscription_resp.status(), StatusCode::OK);

    let token_resp = send(
        &app,
        json_request(
            Method::POST,
            "/api/tokens",
            Some(ADMIN_TOKEN),
            json!({
                "user_id": user["id"].as_str().unwrap(),
            }),
        )?,
    )
    .await;
    assert_eq!(token_resp.status(), StatusCode::OK);

    let list_tokens = send(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri("/api/tokens")
            .header("Authorization", format!("Bearer {}", ADMIN_TOKEN))
            .body(Body::empty())?,
    )
    .await;
    assert_eq!(list_tokens.status(), StatusCode::OK);
    let tokens: Value = read_json(list_tokens).await;
    assert_eq!(tokens.as_array().map(|arr| arr.len()), Some(1));

    let upstream_resp = send(
        &app,
        json_request(
            Method::POST,
            "/api/upstreams",
            Some(ADMIN_TOKEN),
            json!({
                "name": "demo-http",
                "kind": "http",
                "url": "http://localhost/mcp",
                "args": [],
            }),
        )?,
    )
    .await;
    assert_eq!(upstream_resp.status(), StatusCode::CREATED);

    let upstreams = send(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri("/api/upstreams")
            .header("Authorization", format!("Bearer {}", ADMIN_TOKEN))
            .body(Body::empty())?,
    )
    .await;
    assert_eq!(upstreams.status(), StatusCode::OK);
    let upstream_list: Value = read_json(upstreams).await;
    assert_eq!(upstream_list.as_array().map(|arr| arr.len()), Some(1));

    let subscriptions = send(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri("/api/subscriptions")
            .header("Authorization", format!("Bearer {}", ADMIN_TOKEN))
            .body(Body::empty())?,
    )
    .await;
    assert_eq!(subscriptions.status(), StatusCode::OK);

    let stored_key = state
        .store
        .fetch_provider_key("openai", "api_key")
        .await?
        .expect("key stored");
    assert_eq!(stored_key, b"sk-test");

    Ok(())
}

async fn read_json<T: DeserializeOwned>(response: axum::response::Response) -> T {
    let bytes = to_bytes(response.into_body()).await.expect("body bytes");
    serde_json::from_slice(&bytes).expect("json response")
}

async fn send(
    app: &Router<RouterState>,
    request: Request<Body>,
) -> axum::response::Response {
    app.clone().oneshot(request).await.expect("request")
}

fn json_request(
    method: Method,
    uri: &str,
    token: Option<&str>,
    payload: Value,
) -> anyhow::Result<Request<Body>> {
    let mut builder = Request::builder().method(method).uri(uri);
    builder = builder.header("Content-Type", "application/json");
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {}", token));
    }
    let body = Body::from(serde_json::to_vec(&payload)?);
    Ok(builder.body(body)?)
}
