// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{
    body::Body,
    extract::State,
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    Json,
};
use futures::stream::{self, Stream, StreamExt};
use http::StatusCode;
use serde_json::Value;
use std::{convert::Infallible, time::Duration};
use tracing::{debug, error, info, warn};

use crate::{
    config::EndpointStyle,
    routes::{
        proxy::{proxy_handler_multi, HasModel},
        validate_model_or_error,
    },
    server::AppState,
    types::anthropic::MessageRequest,
    types::conversion::{
        anthropic_request_to_openai, openai_response_to_anthropic, OpenAIToAnthropicStreamConverter,
    },
    types::validation::{
        validate_max_tokens, validate_message_content, validate_role, validate_temperature,
        validate_top_p,
    },
    ProviderRegistry,
};

impl HasModel for MessageRequest {
    fn model(&self) -> &str {
        &self.model
    }
}

fn anthropic_error(status: StatusCode, error_type: &str, message: &str) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "type": "error",
                "error": {
                    "type": error_type,
                    "message": message
                }
            })
            .to_string(),
        ))
        .expect("failed to build error response")
}

fn anthropic_server_error(status: StatusCode, message: &str) -> Response {
    anthropic_error(status, "api_error", message)
}

fn convert_openai_error_to_anthropic(openai_error_body: &str) -> String {
    let error_type = if let Ok(v) = serde_json::from_str::<Value>(openai_error_body) {
        v.get("error")
            .and_then(|e| e.get("type"))
            .and_then(|t| t.as_str())
            .unwrap_or("api_error")
            .to_string()
    } else {
        "api_error".to_string()
    };

    let message = if let Ok(v) = serde_json::from_str::<Value>(openai_error_body) {
        v.get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Upstream error")
            .to_string()
    } else {
        "Upstream error".to_string()
    };

    serde_json::json!({
        "type": "error",
        "error": {
            "type": error_type,
            "message": message
        }
    })
    .to_string()
}

fn safe_api_key<'a>(
    provider: &'a crate::config::Provider,
    default_key: Option<&'a String>,
    gateway_url: &str,
) -> Option<&'a str> {
    if provider.api_key.is_some() {
        return provider.api_key.as_deref();
    }
    if provider.base_url.trim_end_matches('/') == gateway_url.trim_end_matches('/') {
        default_key.map(|s| s.as_str())
    } else {
        None
    }
}

const MAX_FAILOVER_ATTEMPTS: usize = 3;

async fn try_provider_non_streaming(
    state: &AppState,
    provider: &crate::config::Provider,
    openai_body: &[u8],
) -> Result<Response, StatusCode> {
    let url = ProviderRegistry::build_endpoint_url(provider, "v1/chat/completions");
    debug!("Built URL for Anthropic-converted request: {}", url);

    let api_key = safe_api_key(
        provider,
        state.proxy_client.api_key(),
        state.proxy_client.base_url(),
    );

    match state
        .proxy_client
        .forward_request_to_url_raw(&url, openai_body.to_vec(), api_key, provider.endpoint_style)
        .await
    {
        Ok(response) => {
            let status = response.status();
            if status.is_server_error() {
                warn!(
                    "Provider '{}' returned {}, will try next",
                    provider.name, status
                );
                return Err(status);
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                let anthropic_err = convert_openai_error_to_anthropic(&body);
                return Ok(Response::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(Body::from(anthropic_err))
                    .expect("failed to build response"));
            }

            let body = response.text().await.map_err(|e| {
                error!("Failed to read response body: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            let openai_response: Value = serde_json::from_str(&body).map_err(|e| {
                error!("Failed to parse OpenAI response: {}", e);
                StatusCode::BAD_GATEWAY
            })?;

            let anthropic_response = openai_response_to_anthropic(&openai_response);
            let response_body = serde_json::to_string(&anthropic_response).map_err(|e| {
                error!("Failed to serialize Anthropic response: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(response_body))
                .expect("failed to build response"))
        }
        Err(e) => {
            warn!("Provider '{}' connection error: {}", provider.name, e);
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

async fn try_provider_streaming(
    state: &AppState,
    provider: &crate::config::Provider,
    openai_body: &[u8],
) -> Result<std::pin::Pin<Box<dyn Stream<Item = anyhow::Result<String>> + Send>>, StatusCode> {
    let url = ProviderRegistry::build_endpoint_url(provider, "v1/chat/completions");
    let api_key = safe_api_key(
        provider,
        state.proxy_client.api_key(),
        state.proxy_client.base_url(),
    );

    match state
        .proxy_client
        .forward_request_stream_to_url(&url, openai_body.to_vec(), api_key, provider.endpoint_style)
        .await
    {
        Ok(stream) => Ok(stream),
        Err(e) => {
            warn!("Streaming provider '{}' failed: {}", provider.name, e);
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

async fn handle_non_streaming_conversion(
    state: &AppState,
    anthropic_value: &Value,
    providers: Vec<crate::config::Provider>,
) -> Response {
    let openai_request = anthropic_request_to_openai(anthropic_value);

    let openai_body = match serde_json::to_vec(&openai_request) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to serialize converted OpenAI request: {}", e);
            return anthropic_server_error(StatusCode::BAD_REQUEST, "Invalid request format");
        }
    };

    debug!(
        "Converted non-streaming request body (first 2000 chars): {:.2000}",
        String::from_utf8_lossy(&openai_body)
    );

    if providers.is_empty() {
        debug!("No providers found, forwarding to default gateway");
        match state
            .proxy_client
            .forward_request("v1/chat/completions", openai_body)
            .await
        {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    return anthropic_server_error(
                        StatusCode::BAD_GATEWAY,
                        "Service temporarily unavailable",
                    );
                }
                let body = response.text().await.unwrap_or_default();
                match serde_json::from_str::<Value>(&body) {
                    Ok(openai_resp) => {
                        let anthropic_resp = openai_response_to_anthropic(&openai_resp);
                        let resp_body = serde_json::to_string(&anthropic_resp).unwrap_or(body);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(Body::from(resp_body))
                            .expect("failed to build response")
                    }
                    Err(_) => {
                        anthropic_server_error(StatusCode::BAD_GATEWAY, "Invalid upstream response")
                    }
                }
            }
            Err(e) => {
                error!("Proxy error: {}", e);
                anthropic_server_error(StatusCode::BAD_GATEWAY, "Failed to forward request")
            }
        }
    } else if providers.len() == 1 {
        let provider = &providers[0];
        info!(
            "Routing Anthropic request for model '{}' to provider '{}' ({})",
            anthropic_value
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown"),
            provider.name,
            provider.base_url
        );
        match try_provider_non_streaming(state, provider, &openai_body).await {
            Ok(response) => response,
            Err(_) => anthropic_server_error(StatusCode::BAD_GATEWAY, "Failed to forward request"),
        }
    } else {
        info!(
            "Routing Anthropic request across {} providers (failover enabled)",
            providers.len()
        );
        let mut last_error = StatusCode::BAD_GATEWAY;
        for (i, provider) in providers.iter().take(MAX_FAILOVER_ATTEMPTS).enumerate() {
            debug!(
                "Trying provider {}/{}: '{}'",
                i + 1,
                MAX_FAILOVER_ATTEMPTS,
                provider.name
            );
            match try_provider_non_streaming(state, provider, &openai_body).await {
                Ok(response) => return response,
                Err(status) => {
                    last_error = status;
                }
            }
        }
        error!(
            "All {} providers failed for Anthropic request",
            providers.len()
        );
        anthropic_server_error(last_error, "All providers failed")
    }
}

async fn handle_streaming_conversion(
    state: &AppState,
    anthropic_value: &Value,
    providers: Vec<crate::config::Provider>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let model = anthropic_value
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();

    let mut openai_request = anthropic_request_to_openai(anthropic_value);
    openai_request["stream"] = Value::Bool(true);

    let openai_body = serde_json::to_vec(&openai_request).map_err(|e| {
        error!(
            "Failed to serialize converted streaming OpenAI request: {}",
            e
        );
        StatusCode::BAD_REQUEST
    })?;

    debug!(
        "Converted streaming request body (first 2000 chars): {:.2000}",
        String::from_utf8_lossy(&openai_body)
    );

    let raw_stream = if providers.is_empty() {
        state
            .proxy_client
            .forward_request_stream("v1/chat/completions", openai_body)
            .await
            .map_err(|e| {
                error!("Failed to forward streaming request: {}", e);
                StatusCode::BAD_GATEWAY
            })?
    } else if providers.len() == 1 {
        let provider = &providers[0];
        info!(
            "Streaming Anthropic request for model '{}' to provider '{}'",
            model, provider.name
        );
        try_provider_streaming(state, provider, &openai_body).await?
    } else {
        info!(
            "Streaming Anthropic request with {} providers (failover)",
            providers.len()
        );
        let mut last_status = StatusCode::BAD_GATEWAY;
        for (i, provider) in providers.iter().take(MAX_FAILOVER_ATTEMPTS).enumerate() {
            match try_provider_streaming(state, provider, &openai_body).await {
                Ok(s) => {
                    if i > 0 {
                        info!(
                            "Streaming succeeded on provider '{}' (attempt {})",
                            provider.name,
                            i + 1
                        );
                    }
                    return Ok(build_anthropic_sse(s, model));
                }
                Err(status) => {
                    last_status = status;
                }
            }
        }
        return Err(last_status);
    };

    Ok(build_anthropic_sse(raw_stream, model))
}

fn build_anthropic_sse(
    raw_stream: std::pin::Pin<Box<dyn Stream<Item = anyhow::Result<String>> + Send>>,
    model: String,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let converter = std::sync::Mutex::new(OpenAIToAnthropicStreamConverter::new(model));

    let sse_stream = raw_stream.flat_map(move |chunk| {
        let events: Vec<Result<Event, Infallible>> = match chunk {
            Ok(data) => {
                let mut conv = converter.lock().unwrap();
                conv.convert_chunk(&data)
                    .into_iter()
                    .map(|sse_event| Ok(Event::from(sse_event)))
                    .collect()
            }
            Err(e) => {
                error!("Stream chunk error: {}", e);
                vec![Ok(Event::default()
                    .data(r#"{"type":"error","error":{"type":"api_error","message":"Stream interrupted"}}"#))]
            }
        };
        stream::iter(events)
    });

    Sse::new(sse_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    )
}

/// Stream an Anthropic-format request directly to an Anthropic-style provider
/// with true SSE passthrough (no buffering, no conversion needed).
async fn handle_anthropic_direct_streaming(
    state: &AppState,
    request: &MessageRequest,
    providers: Vec<crate::config::Provider>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let body = serde_json::to_vec(request).map_err(|_| StatusCode::BAD_REQUEST)?;

    let gateway_url = state.proxy_client.base_url().to_string();
    let default_key = state.proxy_client.api_key().cloned();

    for (i, provider) in providers.iter().take(MAX_FAILOVER_ATTEMPTS).enumerate() {
        let url = ProviderRegistry::build_endpoint_url(provider, "v1/messages");
        let api_key = safe_api_key(provider, default_key.as_ref(), &gateway_url);

        debug!(
            "Streaming Anthropic-direct to provider {}/{}: '{}' ({})",
            i + 1,
            MAX_FAILOVER_ATTEMPTS,
            provider.name,
            url
        );

        match state
            .proxy_client
            .forward_request_stream_to_url(&url, body.clone(), api_key, EndpointStyle::Anthropic)
            .await
        {
            Ok(raw_stream) => {
                let sse_keep_alive_secs = state.config.http.sse_keep_alive_secs;
                let sse_stream = raw_stream.flat_map(move |chunk_result| {
                    let events = match chunk_result {
                        Ok(data) => process_sse_chunk_lines_anthropic(&data),
                        Err(e) => {
                            error!("Stream error in Anthropic-direct: {}", e);
                            vec![Ok(Event::default().data(
                                serde_json::json!({
                                    "type": "error",
                                    "error": {"type": "api_error", "message": "Stream interrupted"}
                                })
                                .to_string(),
                            ))]
                        }
                    };
                    stream::iter(events)
                });

                return Ok(Sse::new(sse_stream).keep_alive(
                    axum::response::sse::KeepAlive::new()
                        .interval(Duration::from_secs(sse_keep_alive_secs))
                        .text("keepalive"),
                ));
            }
            Err(e) => {
                warn!(
                    "Provider '{}' streaming failed (attempt {}/{}): {}",
                    provider.name,
                    i + 1,
                    MAX_FAILOVER_ATTEMPTS,
                    e
                );
            }
        }
    }

    Err(StatusCode::BAD_GATEWAY)
}

/// Parse SSE chunk lines for Anthropic-format passthrough (no conversion needed)
fn process_sse_chunk_lines_anthropic(chunk: &str) -> Vec<Result<Event, Infallible>> {
    let mut events = Vec::new();
    let mut event_type = String::new();

    for line in chunk.lines() {
        let line = line.trim_end_matches('\r');

        if line.is_empty() {
            continue;
        }

        if let Some(et) = line.strip_prefix("event: ") {
            event_type = et.to_string();
            continue;
        }

        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                events.push(Ok(Event::default().data("[DONE]")));
                continue;
            }

            let mut event = Event::default().data(data);
            if !event_type.is_empty() {
                event = event.event(event_type.clone());
            }
            events.push(Ok(event));
            event_type.clear();
        }
    }

    events
}

/// Anthropic messages endpoint with multi-provider support and format conversion
pub async fn anthropic_messages(
    State(state): State<AppState>,
    Json(mut request): Json<MessageRequest>,
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

    // Validate max_tokens if provided
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

    if let Some(temp) = request.temperature {
        if let Err(e) = validate_temperature(temp) {
            return anthropic_error(StatusCode::BAD_REQUEST, "invalid_request_error", &e);
        }
    }

    if let Some(top_p) = request.other.get("top_p").and_then(|v| v.as_f64()) {
        if let Err(e) = validate_top_p(top_p as f32) {
            return anthropic_error(StatusCode::BAD_REQUEST, "invalid_request_error", &e);
        }
    }

    // Skip model validation when multi-provider is disabled (all models go to Aperture)
    if state.config.multi_provider_enabled {
        let provider_has_model = state
            .provider_registry
            .get_providers_for_model(&request.model)
            .await
            .iter()
            .any(|p| p.enabled);
        let discovery_has_model = state.discovery.is_valid_model(&request.model).await;

        if !provider_has_model && !discovery_has_model {
            warn!("Invalid model requested: {}", request.model);
            return anthropic_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &format!("Model '{}' not found", request.model),
            );
        }
    }

    debug!("Valid model: {}", request.model);

    let anthropic_providers: Vec<_> = state
        .provider_registry
        .get_providers_for_model(&request.model)
        .await
        .into_iter()
        .filter(|p| p.endpoint_style == EndpointStyle::Anthropic)
        .collect();

    if !anthropic_providers.is_empty() {
        debug!(
            "Found {} Anthropic-style provider(s) for model '{}', forwarding directly",
            anthropic_providers.len(),
            request.model
        );

        // Use true SSE streaming when stream:true, otherwise buffer
        if request.stream.unwrap_or(false) {
            match handle_anthropic_direct_streaming(&state, &request, anthropic_providers).await {
                Ok(sse) => return sse.into_response(),
                Err(status) => {
                    return anthropic_server_error(status, "Streaming request failed")
                        .into_response();
                }
            }
        }

        return proxy_handler_multi(
            state.proxy_client,
            anthropic_providers,
            request,
            "v1/messages",
            state.config.multi_provider_enabled,
            &state.provider_registry,
        )
        .await
        .into_response();
    }

    info!(
        "No Anthropic-style provider for model '{}', converting to OpenAI format",
        request.model
    );

    let anthropic_value = match serde_json::to_value(&request) {
        Ok(v) => v,
        Err(e) => {
            warn!(
                "Failed to serialize Anthropic request for conversion: {}",
                e
            );
            return anthropic_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Invalid request format",
            );
        }
    };

    let is_streaming = request.stream.unwrap_or(false);

    let openai_providers = state
        .provider_registry
        .get_providers_for_model(&request.model)
        .await;

    if is_streaming {
        match handle_streaming_conversion(&state, &anthropic_value, openai_providers).await {
            Ok(sse) => sse.into_response(),
            Err(status) => {
                anthropic_server_error(status, "Streaming request failed").into_response()
            }
        }
    } else {
        handle_non_streaming_conversion(&state, &anthropic_value, openai_providers)
            .await
            .into_response()
    }
}
