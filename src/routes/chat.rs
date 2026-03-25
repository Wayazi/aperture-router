// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{extract::State, response::IntoResponse, Json};
use http::StatusCode;
use tracing::{debug, warn};

use crate::{
    routes::proxy::{proxy_handler_multi, HasModel},
    server::AppState,
    types::openai::ChatCompletionRequest,
};

impl HasModel for ChatCompletionRequest {
    fn model(&self) -> &str {
        &self.model
    }
}

/// OpenAI chat completions endpoint with multi-provider support
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(request): Json<ChatCompletionRequest>,
) -> impl axum::response::IntoResponse {
    // Skip model validation when multi-provider is disabled (all models go to Aperture)
    if state.config.multi_provider_enabled {
        // Validate model exists (check both discovery and provider registry)
        let provider_has_model = state.provider_registry.get_provider_for_model(&request.model).is_some();
        let discovery_has_model = state.discovery.is_valid_model(&request.model).await;

        if !provider_has_model && !discovery_has_model {
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
    }

    debug!("Valid model: {}", request.model);
    proxy_handler_multi::<ChatCompletionRequest>(
        state.proxy_client,
        (*state.provider_registry).clone(),
        request,
        "v1/chat/completions",
        state.config.multi_provider_enabled,
    )
    .await
    .into_response()
}
