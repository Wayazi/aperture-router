// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{extract::State, response::IntoResponse, Json};
use http::StatusCode;
use tracing::{debug, warn};

use crate::{
    routes::{
        proxy::{proxy_handler_multi, HasModel},
        shared::MAX_OTHER_FIELDS,
        validate_model_or_error,
    },
    server::AppState,
    types::{
        openai::ChatCompletionRequest,
        validation::{validate_max_tokens, validate_message_content, validate_role},
    },
};

impl HasModel for ChatCompletionRequest {
    fn model(&self) -> &str {
        &self.model
    }
}

/// OpenAI chat completions endpoint with multi-provider support
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(mut request): Json<ChatCompletionRequest>,
) -> impl axum::response::IntoResponse {
    // Resolve model alias before validation
    let original_model = request.model.clone();
    let resolved_model = state.config.resolve_model_alias(&request.model);
    if resolved_model != original_model {
        debug!(
            "Resolved model alias: {} -> {}",
            original_model, resolved_model
        );
        request.model = resolved_model;
    }

    // Validate model name format first
    if let Err(response) = validate_model_or_error(&request) {
        return *response;
    }

    // Validate max_tokens
    if let Some(max_tokens) = request.max_tokens {
        if let Err(e) = validate_max_tokens(max_tokens) {
            warn!("Invalid max_tokens: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": e,
                        "type": "invalid_request_error",
                        "code": "invalid_max_tokens"
                    }
                })),
            )
                .into_response();
        }
    }

    // Validate messages
    let max_messages = state.config.security.max_messages;
    if request.messages.len() > max_messages {
        warn!(
            "Too many messages: {} (max {})",
            request.messages.len(),
            max_messages
        );
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": {
                    "message": format!("Too many messages (max {})", max_messages),
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

        if let Some(content) = &msg.content {
            if let Some(content_str) = content.as_str() {
                if let Err(e) = validate_message_content(content_str) {
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
        }
    }

    // Validate other HashMap size (prevent memory exhaustion)
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
            .get_providers_for_model(&request.model)
            .await
            .iter()
            .any(|p| p.enabled);
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

    let providers = state
        .provider_registry
        .get_providers_for_model(&request.model)
        .await;

    proxy_handler_multi(
        state.proxy_client,
        providers,
        request,
        "v1/chat/completions",
        state.config.multi_provider_enabled,
        &state.provider_registry,
    )
    .await
    .into_response()
}
