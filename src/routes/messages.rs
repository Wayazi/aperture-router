// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{extract::State, response::IntoResponse, Json};
use http::StatusCode;
use tracing::{debug, warn};

use crate::{
    routes::proxy::{proxy_handler, HasModel},
    server::AppState,
    types::anthropic::MessageRequest,
};

impl HasModel for MessageRequest {
    fn model(&self) -> &str {
        &self.model
    }
}

/// Anthropic messages endpoint
pub async fn anthropic_messages(
    State(state): State<AppState>,
    Json(request): Json<MessageRequest>,
) -> impl axum::response::IntoResponse {
    // Validate model exists
    if !state.discovery.is_valid_model(&request.model).await {
        warn!("Invalid model requested: {}", request.model);
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": {
                    "message": format!("Model '{}' not found", request.model),
                    "type": "invalid_request_error",
                    "code": "model_not_found"
                }
            })),
        )
            .into_response();
    }

    debug!("Valid model: {}", request.model);
    proxy_handler::<MessageRequest>(State(state.proxy_client), Json(request), "v1/messages")
        .await
        .into_response()
}
