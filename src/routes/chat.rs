// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{extract::State, response::IntoResponse, Json};
use http::StatusCode;
use tracing::{debug, warn};

use crate::{
    routes::proxy::{proxy_handler, HasModel},
    server::AppState,
    types::openai::ChatCompletionRequest,
};

impl HasModel for ChatCompletionRequest {
    fn model(&self) -> &str {
        &self.model
    }
}

/// OpenAI chat completions endpoint
pub async fn chat_completions(
    State((_, _, proxy_client, discovery, _)): State<AppState>,
    Json(request): Json<ChatCompletionRequest>,
) -> impl axum::response::IntoResponse {
    // Validate model exists
    if !discovery.is_valid_model(&request.model).await {
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
    proxy_handler::<ChatCompletionRequest>(
        State(proxy_client),
        Json(request),
        "v1/chat/completions",
    )
    .await
    .into_response()
}
