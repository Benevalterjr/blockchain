// src/middleware.rs
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    RequestPartsExt,
};
use std::env;

/// Header custom: X-API-Key
#[derive(Debug)]
pub struct ApiKey(String);

#[async_trait]
impl<S> FromRequestParts<S> for ApiKey
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let api_key = parts
            .headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                (StatusCode::BAD_REQUEST, "Missing X-API-Key header".to_string()).into_response()
            })?;

        let expected_key = env::var("API_KEY")
            .unwrap_or_else(|_| "dev-secret-key-123".to_string());

        if api_key == expected_key {
            Ok(ApiKey(api_key.to_string()))
        } else {
            Err((StatusCode::UNAUTHORIZED, "Invalid API key".to_string()).into_response())
        }
    }
}
