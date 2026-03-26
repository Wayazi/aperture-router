// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{extract::State, response::IntoResponse, Json};
use http::StatusCode;
use tracing::{debug, warn};

use crate::{
    routes::proxy::{proxy_handler_multi, HasModel},
    server::AppState,
    types::anthropic::MessageRequest,
};

impl HasModel for MessageRequest {
    fn model(&self) -> &str {
        &self.model
    }
}

/// Anthropic messages endpoint with multi-provider support
pub async fn anthropic_messages(
    State(state): State<AppState>,
    Json(request): Json<MessageRequest>,
) -> impl axum::response::IntoResponse {
    // Skip model validation when multi-provider is disabled (all models go to Aperture)
    if state.config.multi_provider_enabled {
        // Validate model exists (check both discovery and provider registry)
        let provider_has_model = state.provider_registry.get_provider_for_model(&request.model).await.is_some();
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

    // Get provider for model (if any)
    let provider = state.provider_registry.get_provider_for_model(&request.model).await;

    proxy_handler_multi(
        state.proxy_client,
        provider,
        request,
        "v1/messages",
        state.config.multi_provider_enabled,
        &state.provider_registry,
    )
    .await
    .into_response()
}
