use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};
use serde_json;

use crate::{
    auth::BearerToken,
    router::RouterState,
    subs::{ApiTokenRecord, NewProvider, Tier, UpstreamRecord},
    upstream::UpstreamRegistration,
};

pub fn router(state: RouterState) -> Router<RouterState> {
    Router::new()
        .route("/upstreams", get(list_upstreams).post(create_upstream))
        .route("/providers", get(list_providers).post(create_provider))
        .route("/providers/keys", post(store_provider_key))
        .route(
            "/subscriptions",
            get(list_subscriptions).post(upsert_subscription),
        )
        .route("/users", get(list_users).post(create_user))
        .route("/tokens", get(list_tokens).post(issue_token))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct UpstreamRequest {
    name: String,
    kind: String,
    command: Option<String>,
    args: Option<Vec<String>>,
    url: Option<String>,
    bearer: Option<String>,
    provider_slug: Option<String>,
}

#[derive(Debug, Serialize)]
struct UpstreamResponse {
    name: String,
    kind: String,
    command: Option<String>,
    args: Vec<String>,
    url: Option<String>,
    bearer: Option<String>,
    provider_slug: Option<String>,
}

async fn list_upstreams(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
) -> Result<Json<Vec<UpstreamResponse>>, AppError> {
    let records = state.store.list_upstreams().await?;
    let data = records
        .into_iter()
        .map(|record| UpstreamResponse {
            name: record.name.clone(),
            kind: record.kind.clone(),
            command: record.command.clone(),
            args: record.args_vec(),
            url: record.url.clone(),
            bearer: record.bearer.clone(),
            provider_slug: record.provider_slug.clone(),
        })
        .collect();
    Ok(Json(data))
}

async fn create_upstream(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
    Json(payload): Json<UpstreamRequest>,
) -> Result<impl IntoResponse, AppError> {
    let record = UpstreamRecord {
        name: payload.name.clone(),
        kind: payload.kind.clone(),
        command: payload.command.clone(),
        args: payload
            .args
            .as_ref()
            .map(|args| serde_json::to_string(args).unwrap_or_default()),
        url: payload.url.clone(),
        bearer: payload.bearer.clone(),
        provider_slug: payload.provider_slug.clone(),
    };
    state.store.upsert_upstream(&record).await?;
    let registration = UpstreamRegistration {
        name: record.name.clone(),
        kind: record.kind.clone(),
        command: record.command.clone(),
        args: record
            .args
            .as_ref()
            .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
            .unwrap_or_default(),
        url: record.url.clone(),
        bearer: record.bearer.clone(),
        provider_slug: record.provider_slug.clone(),
    };
    state.registry.register(registration).await?;
    let args_vec = record.args_vec();
    let UpstreamRecord {
        name,
        kind,
        command,
        url,
        bearer,
        provider_slug,
        ..
    } = record;
    Ok((
        StatusCode::CREATED,
        Json(UpstreamResponse {
            name,
            kind,
            command,
            args: args_vec,
            url,
            bearer,
            provider_slug,
        }),
    ))
}

#[derive(Debug, Deserialize)]
struct ProviderRequest {
    slug: String,
    display_name: String,
    description: Option<String>,
}

async fn create_provider(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
    Json(payload): Json<ProviderRequest>,
) -> Result<Json<crate::subs::ProviderRecord>, AppError> {
    let record = state
        .store
        .put_provider(&NewProvider {
            slug: payload.slug,
            display_name: payload.display_name,
            description: payload.description,
        })
        .await?;
    Ok(Json(record))
}

async fn list_providers(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
) -> Result<Json<Vec<crate::subs::ProviderRecord>>, AppError> {
    let providers = state.store.list_providers().await?;
    Ok(Json(providers))
}

#[derive(Debug, Deserialize)]
struct ProviderKeyRequest {
    provider_slug: String,
    name: String,
    value: String,
}

async fn store_provider_key(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
    Json(payload): Json<ProviderKeyRequest>,
) -> Result<impl IntoResponse, AppError> {
    state
        .store
        .store_provider_key(
            &payload.provider_slug,
            &payload.name,
            payload.value.as_bytes(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct SubscriptionRequest {
    user_id: String,
    tier: Tier,
    expires_at: Option<DateTimeWrapper>,
    max_tokens: Option<i64>,
    max_requests: Option<i64>,
    max_concurrent: Option<i32>,
}

#[derive(Debug, Clone)]
struct DateTimeWrapper(chrono::DateTime<Utc>);

impl DateTimeWrapper {
    fn into_inner(self) -> chrono::DateTime<Utc> {
        self.0
    }
}

impl Serialize for DateTimeWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_rfc3339())
    }
}

impl<'de> Deserialize<'de> for DateTimeWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let parsed = chrono::DateTime::parse_from_rfc3339(&value)
            .map_err(DeError::custom)?
            .with_timezone(&Utc);
        Ok(DateTimeWrapper(parsed))
    }
}

async fn upsert_subscription(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
    Json(payload): Json<SubscriptionRequest>,
) -> Result<Json<crate::subs::SubscriptionRecord>, AppError> {
    let quotas = match (
        payload.max_tokens,
        payload.max_requests,
        payload.max_concurrent,
    ) {
        (Some(tokens), Some(requests), Some(concurrent)) => Some((tokens, requests, concurrent)),
        _ => None,
    };
    let record = state
        .store
        .upsert_subscription(
            &payload.user_id,
            payload.tier,
            payload.expires_at.map(DateTimeWrapper::into_inner),
            quotas,
        )
        .await?;
    Ok(Json(record))
}

async fn list_subscriptions(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
) -> Result<Json<Vec<crate::subs::SubscriptionRecord>>, AppError> {
    let users = state.store.list_users().await?;
    let mut records = Vec::new();
    for user in users {
        if let Some(subscription) = state.store.get_subscription(&user.id).await? {
            records.push(subscription);
        }
    }
    Ok(Json(records))
}

#[derive(Debug, Deserialize)]
struct UserRequest {
    email: String,
    name: Option<String>,
}

async fn create_user(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
    Json(payload): Json<UserRequest>,
) -> Result<Json<crate::subs::UserRecord>, AppError> {
    let record = state
        .store
        .ensure_user(&payload.email, payload.name.as_deref())
        .await?;
    Ok(Json(record))
}

async fn list_users(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
) -> Result<Json<Vec<crate::subs::UserRecord>>, AppError> {
    let users = state.store.list_users().await?;
    Ok(Json(users))
}

#[derive(Debug, Deserialize)]
struct TokenRequest {
    user_id: String,
    scope: Option<String>,
}

async fn issue_token(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
    Json(payload): Json<TokenRequest>,
) -> Result<Json<ApiTokenRecord>, AppError> {
    let record = state
        .store
        .issue_token(
            &payload.user_id,
            payload.scope.as_deref().unwrap_or("default"),
        )
        .await?;
    Ok(Json(record))
}

async fn list_tokens(
    State(state): State<RouterState>,
    BearerToken(_): BearerToken,
) -> Result<Json<Vec<ApiTokenRecord>>, AppError> {
    let tokens = state.store.list_tokens(None).await?;
    Ok(Json(tokens))
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = StatusCode::INTERNAL_SERVER_ERROR;
        let body = Json(serde_json::json!({
            "error": self.to_string(),
        }));
        (status, body).into_response()
    }
}
