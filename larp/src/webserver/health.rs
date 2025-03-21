use axum::{response::IntoResponse, Extension};

use crate::application::application::Application;

use super::types::Result;
use super::types::{json, ApiResponse};

/// We send a HC check over here
///
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthCheckResponse {
    done: bool,
}

impl ApiResponse for HealthCheckResponse {}

pub async fn health(Extension(_app): Extension<Application>) -> Result<impl IntoResponse> {
    Ok(json(HealthCheckResponse { done: true }))
}
