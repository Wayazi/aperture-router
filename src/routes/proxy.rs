// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{extract::State, response::IntoResponse, Json};
use http::StatusCode;
use serde::Serialize;
use tracing::{debug, error};

use crate::proxy::client::ProxyClient;

/// Maximum response size (10MB) to prevent DoS
const MAX_RESPONSE_SIZE: usize = 10 * 1024 * 1024;

/// Trait for requests that have a model field
pub trait HasModel {
    fn model(&self) -> &str;
}

/// Generic proxy handler for JSON requests
pub async fn proxy_handler<T>(
    State(proxy_client): State<ProxyClient>,
    Json(request): Json<T>,
    endpoint: &str,
) -> impl IntoResponse
where
    T: HasModel + Serialize,
{
    debug!(
        "Proxying request to {} with model: {:?}",
        endpoint,
        request.model()
    );

    // Serialize request to bytes
    let body = match serde_json::to_vec(&request) {
        Ok(body) => body,
        Err(e) => {
            error!("Failed to serialize request: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Invalid request format"
                })),
            )
                .into_response();
        }
    };

    // Forward request to Aperture
    match proxy_client.forward_request(endpoint, body).await {
        Ok(response) => {
            let status = response.status();
            let body = match response.text().await {
                Ok(body) => body,
                Err(e) => {
                    error!("Failed to read response body: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": "Failed to read response"
                        })),
                    )
                        .into_response();
                }
            };

            // Check response size limit
            if body.len() > MAX_RESPONSE_SIZE {
                error!("Response too large: {} bytes", body.len());
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": "Response too large"
                    })),
                )
                    .into_response();
            }

            // Build response using tuple syntax (can't fail)
            (status, Json(body)).into_response()
        }
        Err(e) => {
            error!("Proxy error: {}", e);
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "Failed to forward request"
                })),
            )
                .into_response()
        }
    }
}
