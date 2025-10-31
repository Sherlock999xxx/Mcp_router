use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, StatusCode},
    response::Response,
};
use thiserror::Error;

#[derive(Clone)]
pub struct AuthConfig {
    bearer: Option<String>,
}

impl AuthConfig {
    pub fn new(bearer: Option<String>) -> Self {
        Self { bearer }
    }

    pub fn is_enabled(&self) -> bool {
        self.bearer.is_some()
    }

    pub fn validate(&self, token: Option<&str>) -> Result<(), AuthError> {
        match (&self.bearer, token) {
            (Some(expected), Some(actual)) if expected == actual => Ok(()),
            (Some(_), _) => Err(AuthError::Unauthorized),
            (None, _) => Ok(()),
        }
    }
}

#[derive(Clone)]
pub struct AuthLayer(Arc<AuthConfig>);

impl AuthLayer {
    pub fn new(config: AuthConfig) -> Self {
        Self(Arc::new(config))
    }

    pub fn config(&self) -> Arc<AuthConfig> {
        self.0.clone()
    }
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("unauthorized")]
    Unauthorized,
}

impl axum::response::IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, "unauthorized").into_response()
    }
}

pub struct BearerToken(pub Option<String>);

#[axum::async_trait]
impl<S> FromRequestParts<S> for BearerToken
where
    Arc<AuthConfig>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let config = Arc::<AuthConfig>::from_ref(state);
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer ").map(|v| v.to_string()));
        config.validate(token.as_deref())?;
        Ok(BearerToken(token))
    }
}

pub trait FromRef<T> {
    fn from_ref(input: &T) -> Arc<AuthConfig>;
}

impl FromRef<AuthLayer> for Arc<AuthConfig> {
    fn from_ref(layer: &AuthLayer) -> Arc<AuthConfig> {
        layer.config()
    }
}

impl FromRef<Arc<AuthConfig>> for Arc<AuthConfig> {
    fn from_ref(cfg: &Arc<AuthConfig>) -> Arc<AuthConfig> {
        cfg.clone()
    }
}
