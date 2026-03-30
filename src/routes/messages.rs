// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{extract::State, response::IntoResponse, Json};
use http::StatusCode;
use tracing::{debug, warn};

use crate::{
    routes::{proxy::{proxy_handler_multi, HasModel}, validate_model_or_error},
    server::AppState,
    types::{anthropic::MessageRequest, validation::{validate_role, validate_message_content}},
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
    // Validate model name format first
    if let Err(response) = validate_model_or_error(&request) {
        return *response;
    }
    
    // Validate max_tokens (Anthropic requires > 0)
    if request.max_tokens == 0 {
        warn!("max_tokens is 0 or missing");
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": {
                    "message": "max_tokens is required and must be greater than 0",
                    "type": "invalid_request_error",
                    "code": "invalid_max_tokens"
                }
            })),
        )
            .into_response();
    }
    
    // Validate max_tokens upper bound
    const MAX_TOKENS_LIMIT: u32 = 1_000_000;
    if request.max_tokens > MAX_TOKENS_LIMIT {
        warn!("max_tokens exceeds limit: {}", request.max_tokens);
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": {
                    "message": format!("max_tokens exceeds limit of {}", MAX_TOKENS_LIMIT),
                    "type": "invalid_request_error",
                    "code": "invalid_max_tokens"
                }
            })),
        )
            .into_response();
    }
    
    // Validate messages
    const MAX_MESSAGES: usize = 1000;
    if request.messages.len() > MAX_MESSAGES {
        warn!("Too many messages: {}", request.messages.len());
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": {
                    "message": format!("Too many messages (max {})", MAX_MESSAGES),
                    "type": "invalid_request_error",
                    "code": "too_many_messages"
                }
            })),
        )
            .into_response();
    }
    
    for (i, msg) in request.messages.iter().enumerate() {
        if let Err(e) = validate_role(&msg.role) {
            warn!("Invalid role in message {}: {}", i, e);
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": format!("Invalid role in message {}: {}", i, e),
                        "type": "invalid_request_error",
                        "code": "invalid_role"
                    }
                })),
            )
                .into_response();
        }
        
        let content_str = msg.content.as_text();
        if let Err(e) = validate_message_content(&content_str) {
            warn!("Invalid content in message {}: {}", i, e);
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": format!("Invalid content in message {}: {}", i, e),
                        "type": "invalid_request_error",
                        "code": "invalid_content"
                    }
                })),
            )
                .into_response();
        }
    }
    
    // Validate other HashMap size (prevent memory exhaustion)
    const MAX_OTHER_FIELDS: usize = 50;
    if request.other.len() > MAX_OTHER_FIELDS {
        warn!("Too many extra fields: {}", request.other.len());
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": {
                    "message": format!("Too many extra fields (max {})", MAX_OTHER_FIELDS),
                    "type": "invalid_request_error",
                    "code": "too_many_fields"
                }
            })),
        )
            .into_response();
    }
    
    // Skip model validation when multi-provider is disabled (all models go to Aperture)
    if state.config.multi_provider_enabled {
        // Validate model exists (check both discovery and provider registry)
        let provider_has_model = state
            .provider_registry
            .get_provider_for_model(&request.model)
            .await
            .is_some();
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
    let provider = state
        .provider_registry
        .get_provider_for_model(&request.model)
        .await;

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
