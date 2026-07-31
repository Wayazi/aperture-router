// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{
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
    server::AppState,
    types::validation::{
        validate_max_tokens, validate_message_content, validate_model_name, validate_role,
    },
};

fn json_error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        axum::Json(serde_json::json!({
            "error": {
                "message": message,
                "type": "invalid_request_error",
                "code": code
            }
        })),
    )
        .into_response()
}

/// Maximum extra JSON fields (prevent memory exhaustion)
const MAX_OTHER_FIELDS: usize = 50;
/// Maximum content length per message (1MB)
const MAX_CONTENT_SIZE: usize = 1024 * 1024;

/// Handle streaming proxy requests with true SSE streaming
/// Supports both OpenAI and Anthropic formats, including tool/function calling and extended thinking
pub async fn handle_proxy_stream(
    State(state): State<AppState>,
    Json(mut request): Json<Value>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Response> {
    info!("Handling streaming proxy request");

    // Resolve model alias if present
    if let Some(model) = request.get("model").and_then(|m| m.as_str()) {
        let resolved_model = state.config.resolve_model_alias(model);
        if resolved_model != model {
            debug!("Resolved model alias: {} -> {}", model, resolved_model);
            if let Some(obj) = request.as_object_mut() {
                obj.insert("model".to_string(), Value::String(resolved_model));
            }
        }
    }

    // Validate model name if present
    if let Some(model) = request.get("model").and_then(|m| m.as_str()) {
        if let Err(e) = validate_model_name(model) {
            warn!("Invalid model name in streaming request: {}", e);
            return Err(json_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_model_name",
                &e,
            ));
        }
    }

    // Validate messages array if present
    if let Some(messages) = request.get("messages").and_then(|m| m.as_array()) {
        let max_messages = state.config.security.max_messages;
        if messages.len() > max_messages {
            warn!(
                "Too many messages in streaming request: {} (max {})",
                messages.len(),
                max_messages
            );
            return Err(json_error_response(
                StatusCode::BAD_REQUEST,
                "too_many_messages",
                &format!("Too many messages (max {})", max_messages),
            ));
        }

        // Validate roles and content in messages
        for (i, msg) in messages.iter().enumerate() {
            if let Some(role) = msg.get("role").and_then(|r| r.as_str()) {
                if let Err(e) = validate_role(role) {
                    warn!("Invalid role in streaming message {}: {}", i, e);
                    return Err(json_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_role",
                        &format!("Invalid role in message {}: {}", i, e),
                    ));
                }
            }

            // Validate content length (string content)
            if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                if content.len() > MAX_CONTENT_SIZE {
                    warn!(
                        "Content too large in streaming message {}: {} bytes",
                        i,
                        content.len()
                    );
                    return Err(json_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_content",
                        "Content too large",
                    ));
                }
                if let Err(e) = validate_message_content(content) {
                    warn!("Invalid content in streaming message {}: {}", i, e);
                    return Err(json_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_content",
                        &e,
                    ));
                }
            }

            // Validate content array (multi-modal content)
            if let Some(content_array) = msg.get("content").and_then(|c| c.as_array()) {
                if content_array.len() > 100 {
                    warn!(
                        "Too many content blocks in streaming message {}: {}",
                        i,
                        content_array.len()
                    );
                    return Err(json_error_response(
                        StatusCode::BAD_REQUEST,
                        "too_many_content_blocks",
                        "Too many content blocks",
                    ));
                }
                for block in content_array {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        if text.len() > MAX_CONTENT_SIZE {
                            warn!(
                                "Content block too large in streaming message {}: {} bytes",
                                i,
                                text.len()
                            );
                            return Err(json_error_response(
                                StatusCode::BAD_REQUEST,
                                "invalid_content",
                                "Content block too large",
                            ));
                        }
                    }
                }
            }
        }
    }

    // Validate extra fields count (prevent memory exhaustion)
    if let Some(obj) = request.as_object() {
        if obj.len() > MAX_OTHER_FIELDS {
            warn!("Too many fields in streaming request: {}", obj.len());
            return Err(json_error_response(
                StatusCode::BAD_REQUEST,
                "too_many_fields",
                &format!("Too many extra fields (max {})", MAX_OTHER_FIELDS),
            ));
        }
    }

    // Validate max_tokens if present
    if let Some(max_tokens) = request.get("max_tokens").and_then(|t| t.as_u64()) {
        if let Ok(max_tokens_u32) = u32::try_from(max_tokens) {
            if let Err(e) = validate_max_tokens(max_tokens_u32) {
                warn!("Invalid max_tokens in streaming request: {}", e);
                return Err(json_error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_max_tokens",
                    &e,
                ));
            }
        }
    }

    // Check if stream is enabled
    let is_streaming = request
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_streaming {
        debug!("Stream flag not set, returning bad request");
        return Err(json_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Stream must be set to true for this endpoint",
        ));
    }

    // Check if extended thinking should be included (default: hide)
    // Using query parameter or header: include_thinking=true
    let include_thinking = request
        .get("include_thinking")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Log if tools are present in the request
    if let Some(tools) = request.get("tools") {
        info!(
            "Streaming request includes {} tools",
            tools.as_array().map(|a| a.len()).unwrap_or(0)
        );
    }

    // Log extended thinking preference
    if include_thinking {
        info!("Extended thinking will be included in stream");
    } else {
        debug!("Extended thinking will be filtered from stream");
    }

    // Serialize request
    let body = match serde_json::to_vec(&request) {
        Ok(body) => body,
        Err(e) => {
            error!("Failed to serialize streaming request: {}", e);
            return Err(json_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Invalid request format",
            ));
        }
    };

    // Determine endpoint based on request format
    let endpoint = if request.get("max_tokens").is_some() {
        "v1/messages" // Anthropic format
    } else {
        "v1/chat/completions" // OpenAI format
    };

    // Forward request to Aperture
    let response = match state
        .proxy_client
        .forward_request_stream(endpoint, body)
        .await
    {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to forward streaming request: {}", e);
            return Err(json_error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "Failed to forward streaming request",
            ));
        }
    };

    // Convert response chunks to SSE events, preserving tool_calls
    let include_thinking_stream = include_thinking;
    let max_json_depth = state.config.security.max_json_depth;
    let keep_alive_interval = state.config.http.sse_keep_alive_secs;
    let sse_stream = response.flat_map(move |chunk| {
        match chunk {
            Ok(data) => {
                // Parse SSE format from upstream
                // A single chunk may contain multiple SSE events
                // We need to yield one event per data line
                let events: Vec<Result<Event, Infallible>> =
                    process_sse_chunk_lines(&data, include_thinking_stream, max_json_depth)
                        .into_iter()
                        .map(Ok)
                        .collect();
                stream::iter(events)
            }
            Err(e) => {
                error!("Stream chunk error: {}", e);
                // Return generic error, don't expose internal details
                let events: Vec<Result<Event, Infallible>> = vec![Ok(
                    Event::default().data(r#"{"error": "Stream processing error"}"#)
                )];
                stream::iter(events)
            }
        }
    });

    Ok(Sse::new(sse_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(keep_alive_interval))
            .text("keepalive"),
    ))
}

/// Process an SSE chunk from upstream, detecting tool calls and extended thinking
/// Returns a Vec of Events (one per SSE line in the chunk)
fn process_sse_chunk_lines(
    chunk: &str,
    include_thinking: bool,
    max_json_depth: usize,
) -> Vec<Event> {
    let mut event_type = "data".to_string();
    let mut events = Vec::new();

    // Process each line in the chunk
    for line in chunk.lines() {
        // Handle SSE event format: "event: type\ndata: data\n\n"
        if let Some(rest) = line.strip_prefix("event: ") {
            event_type = rest.trim().to_string();
            continue;
        }

        // Handle SSE data format: "data: JSON\n\n"
        if let Some(json_data) = line.strip_prefix("data: ") {
            // Check for special markers
            if json_data == "[DONE]" {
                debug!("Streaming: [DONE] received");
                events.push(Event::default().data("[DONE]"));
                continue;
            }

            // Parse JSON to detect tool calls and extended thinking
            // Limit JSON nesting depth to prevent DoS via deeply nested structures
            let should_include =
                if let Ok(value) = parse_json_with_depth_limit(json_data, max_json_depth) {
                    // Check for extended thinking (Anthropic thinking blocks)
                    if is_thinking_block(&value) {
                        include_thinking
                    } else {
                        // Check for OpenAI tool_calls in delta
                        if check_for_tool_calls_openai(&value) {
                            info!("Streaming: Detected tool call (OpenAI format)");
                        }

                        // Check for Anthropic tool_use content blocks
                        if check_for_tool_calls_anthropic(&value) {
                            info!("Streaming: Detected tool_use (Anthropic format)");
                        }
                        true // Include non-thinking data
                    }
                } else {
                    true // Not valid JSON, include it anyway
                };

            if should_include {
                // Create SSE event with proper type
                if event_type == "data" || event_type.is_empty() {
                    events.push(Event::default().data(json_data));
                } else {
                    events.push(Event::default().event(&event_type).data(json_data));
                }
            }
        }
    }

    // If no events were created, return an empty one
    if events.is_empty() {
        events.push(Event::default().data(""));
    }

    events
}

/// Check for OpenAI-style tool_calls in streaming delta
fn check_for_tool_calls_openai(value: &Value) -> bool {
    // Check for tool_calls in delta (OpenAI streaming format)
    value
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("tool_calls"))
        .is_some()
}

/// Check for Anthropic-style tool_use content blocks
fn check_for_tool_calls_anthropic(value: &Value) -> bool {
    // Check for content_block_start with tool_use type
    if value.get("type").and_then(|t| t.as_str()) == Some("content_block_start") {
        return value
            .get("content_block")
            .and_then(|cb| cb.get("type"))
            .and_then(|t| t.as_str())
            == Some("tool_use");
    }

    // Check for content_block_delta with tool_use content
    if value.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
        return value
            .get("delta")
            .and_then(|d| d.get("type"))
            .and_then(|t| t.as_str())
            == Some("tool_use");
    }

    false
}

/// Check if the value is an extended thinking block (Claude thinking)
fn is_thinking_block(value: &Value) -> bool {
    // Check for Anthropic thinking content blocks
    if value.get("type").and_then(|t| t.as_str()) == Some("content_block_start") {
        if let Some(content_block) = value.get("content_block") {
            return content_block.get("type").and_then(|t| t.as_str()) == Some("thinking");
        }
    }

    // Check for thinking delta content
    if value.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
        if let Some(delta) = value.get("delta") {
            return delta.get("type").and_then(|t| t.as_str()) == Some("thinking");
        }
    }

    // Check for raw thinking text in message content
    if let Some(content) = value.get("content") {
        if let Some(arr) = content.as_array() {
            for item in arr {
                if let Some(obj) = item.as_object() {
                    if obj.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Parse JSON with depth limit to prevent DoS attacks
/// Returns Err if JSON is too deeply nested or invalid
fn parse_json_with_depth_limit(json: &str, max_depth: usize) -> Result<Value, serde_json::Error> {
    use std::io;

    // First do a quick depth check by counting braces/brackets
    let mut depth = 0;
    let mut max_observed = 0;

    for ch in json.chars() {
        match ch {
            '{' | '[' => {
                depth += 1;
                if depth > max_observed {
                    max_observed = depth;
                }
                if depth > max_depth {
                    return Err(serde_json::Error::io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "JSON depth limit exceeded",
                    )));
                }
            }
            '}' | ']' => {
                if depth == 0 {
                    return Err(serde_json::Error::io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Unmatched closing bracket",
                    )));
                }
                depth -= 1;
            }
            _ => {}
        }
    }

    // If depth check passes, parse normally
    serde_json::from_str(json)
}
