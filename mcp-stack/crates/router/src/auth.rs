use std::sync::Arc;

use axum::{
    http::{header::AUTHORIZATION, StatusCode},
    middleware::{from_fn, Next},
    response::Response,
    Router,
};

pub fn apply_auth(app: Router, bearer: Option<String>) -> Router {
    if let Some(token) = bearer.filter(|t| !t.trim().is_empty()) {
        let token = Arc::new(token);
        app.layer(from_fn(move |req, next| {
            let token = token.clone();
            async move { check(req, next, token).await }
        }))
    } else {
        app
    }
}

async fn check(
    req: axum::http::Request<axum::body::Body>,
    next: Next,
    token: Arc<String>,
) -> Result<Response, StatusCode> {
    let authorized = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim() == format!("Bearer {}", token))
        .unwrap_or(false);
    if !authorized {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}
