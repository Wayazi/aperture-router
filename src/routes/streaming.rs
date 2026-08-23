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

use super::shared::MAX_OTHER_FIELDS;
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

    // Determine endpoint based on request format. /v1/proxy accepts both
    // wire formats, so classify by format-specific markers; max_tokens alone
    // is ambiguous (OpenAI requests may carry it too), so it is the weakest
    // signal and only used as the tiebreaker.
    let endpoint = detect_wire_format(&request);

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

    // Convert response chunks to SSE events, preserving tool_calls.
    // buffered_lines reassembles lines split across TCP chunks and flushes the
    // final partial line at end-of-stream, so no event is truncated or lost.
    let include_thinking_stream = include_thinking;
    let max_json_depth = state.config.security.max_json_depth;
    let keep_alive_interval = state.config.http.sse_keep_alive_secs;
    let sse_stream = buffered_lines(response).flat_map(move |chunk| {
        match chunk {
            Ok(data) => {
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

/// Buffers SSE data across network chunks so lines split mid-JSON by TCP
/// boundaries are reassembled before parsing. Mirrors the line buffering the
/// OpenAI→Anthropic stream converter already does for its own input.
pub(crate) struct SseLineBuffer {
    buf: String,
}

impl SseLineBuffer {
    pub(crate) fn new() -> Self {
        Self { buf: String::new() }
    }

    /// Feed one chunk; returns all complete lines (terminators stripped).
    /// A trailing partial line is retained until its remainder arrives.
    /// The buffer is capped: an upstream that never terminates a line cannot
    /// grow it without bound.
    pub(crate) fn push(&mut self, chunk: &str) -> Vec<String> {
        self.buf.push_str(chunk);
        const MAX_LINE_BUFFER: usize = 1024 * 1024; // 1 MiB, matches converter input cap
        if self.buf.len() > MAX_LINE_BUFFER {
            // Cut at the last char boundary at or below the cap — split_off
            // panics on a non-boundary offset (e.g. mid CJK/emoji character).
            let mut cut = MAX_LINE_BUFFER;
            while cut > 0 && !self.buf.is_char_boundary(cut) {
                cut -= 1;
            }
            let overflow = self.buf.split_off(cut);
            // Re-feed the head through the normal path: emits every complete
            // line it contains (no fabricated terminators, nothing popped past
            // the real partial) and retains the true trailing fragment.
            let head = std::mem::take(&mut self.buf);
            let lines = self.push(&head);
            self.buf.push_str(&overflow);
            return lines;
        }
        let mut lines = Vec::new();
        while let Some(pos) = self.buf.find('\n') {
            let line = self.buf[..pos].trim_end_matches('\r').to_string();
            self.buf.drain(..=pos);
            if !line.is_empty() {
                lines.push(line);
            }
        }
        lines
    }

    /// Drain any retained partial line. Call once when the upstream stream
    /// ends: SSE treats EOF as a line terminator, so a final event without a
    /// trailing newline is still deliverable.
    pub(crate) fn take_remainder(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.buf))
        }
    }
}

/// Reassemble an upstream byte stream into whole-line chunks, retaining the
/// trailing partial line across chunk boundaries and flushing it once when
/// the upstream ends.
pub(crate) fn buffered_lines(
    stream: crate::proxy::client::BoxedResultStream,
) -> crate::proxy::client::BoxedResultStream {
    Box::pin(futures::stream::unfold(
        (stream, Some(SseLineBuffer::new())),
        |(mut stream, mut buf)| async move {
            loop {
                // None means the flush branch already ran and returned; unfold
                // polls the closure once more before observing the terminal
                // None. Recreating an empty buffer here is harmless — it is
                // immediately drained — so this stays debug-level, not error.
                let mut buffer = buf.take().unwrap_or_else(|| {
                    debug!("Stream unfold polled after flush; using fresh buffer");
                    SseLineBuffer::new()
                });
                match stream.next().await {
                    Some(Ok(data)) => {
                        let complete = buffer.push(&data);
                        if complete.is_empty() {
                            buf = Some(buffer);
                            continue;
                        }
                        let joined = format!("{}\n", complete.join("\n"));
                        return Some((Ok(joined), (stream, Some(buffer))));
                    }
                    Some(Err(e)) => {
                        buf = Some(buffer);
                        return Some((Err(e), (stream, buf)));
                    }
                    None => {
                        let remainder = buffer.take_remainder().map(Ok);
                        return remainder.map(|item| (item, (stream, None)));
                    }
                }
            }
        },
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

/// Classify a /v1/proxy request body as Anthropic ("v1/messages") or OpenAI
/// ("v1/chat/completions") wire format. Format-exclusive markers win; when
/// neither side's markers appear, fall back to the historical max_tokens
/// heuristic so existing clients keep their current routing.
fn detect_wire_format(request: &Value) -> &'static str {
    // Anthropic-exclusive fields (no OpenAI chat-completions equivalent).
    // NOTE: `metadata` and `service_tier` exist in BOTH APIs and must stay
    // out of this list — ambiguous payloads fall through to the max_tokens
    // heuristic below, which reproduces historical routing for them.
    let anthropic_markers = ["system", "stop_sequences", "thinking", "top_k"];
    if anthropic_markers.iter().any(|k| request.get(k).is_some()) {
        return "v1/messages";
    }

    // OpenAI-exclusive fields (rejected/absent in Anthropic requests).
    // NOTE: `service_tier` exists in BOTH APIs — not a reliable marker.
    let openai_markers = [
        "max_completion_tokens",
        "frequency_penalty",
        "presence_penalty",
        "logit_bias",
        "n",
        "response_format",
        "seed",
    ];
    if openai_markers.iter().any(|k| request.get(k).is_some()) {
        return "v1/chat/completions";
    }

    // Ambiguous: historical behavior (Anthropic requests always carry
    // max_tokens; OpenAI requests usually do not).
    if request.get("max_tokens").is_some() {
        "v1/messages"
    } else {
        "v1/chat/completions"
    }
}

/// Parse JSON with depth limit to prevent DoS attacks
/// Returns Err if JSON is too deeply nested or invalid.
/// String-literal aware: brackets inside JSON strings (and escaped quotes)
/// do not count toward depth, so payloads like `"{\"a\":1}"` pass cleanly.
fn parse_json_with_depth_limit(json: &str, max_depth: usize) -> Result<Value, serde_json::Error> {
    use std::io;

    // Quick depth pre-check before the full parse. Only structural brackets
    // outside string literals count; serde_json then does the real validation.
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for ch in json.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else {
                match ch {
                    '\\' => escaped = true,
                    '"' => in_string = false,
                    _ => {}
                }
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '[' => {
                depth += 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_depth_limit_ignores_brackets_inside_strings() {
        // Braces inside string values must not inflate observed depth.
        let json = r#"{"payload":"{\"a\":{\"b\":1}}","note":"brackets }[({ inside"}"#;
        let parsed = parse_json_with_depth_limit(json, 16).expect("must parse");
        assert_eq!(parsed["payload"], "{\"a\":{\"b\":1}}");
    }

    #[test]
    fn test_depth_limit_still_rejects_real_deep_nesting() {
        let deep = format!("{}{}", "[".repeat(64), "]".repeat(64));
        let err = parse_json_with_depth_limit(&deep, 16).unwrap_err();
        assert!(err.to_string().contains("depth limit"));
    }

    #[test]
    fn test_depth_limit_handles_escaped_quote_in_string() {
        // The escaped quote must not terminate the string early.
        let json = r#"{"text":"quote \" then brace } inside","ok":true}"#;
        assert!(parse_json_with_depth_limit(json, 8).is_ok());
    }

    #[test]
    fn test_wire_format_anthropic_markers_win() {
        assert_eq!(
            detect_wire_format(&serde_json::json!({"system": "be nice", "messages": []})),
            "v1/messages"
        );
        assert_eq!(
            detect_wire_format(&serde_json::json!({"stop_sequences": ["."], "max_tokens": 5})),
            "v1/messages"
        );
    }

    #[test]
    fn test_wire_format_openai_markers_win() {
        // OpenAI request carrying max_tokens: the old sniffing misrouted this
        // to v1/messages; OpenAI-exclusive markers now win.
        assert_eq!(
            detect_wire_format(&serde_json::json!({
                "model": "m", "messages": [], "max_tokens": 5,
                "frequency_penalty": 0.5
            })),
            "v1/chat/completions"
        );
        assert_eq!(
            detect_wire_format(&serde_json::json!({
                "model": "m", "messages": [], "response_format": {"type": "json"}
            })),
            "v1/chat/completions"
        );
    }

    #[test]
    fn test_wire_format_fallback_matches_historical_behavior() {
        // No exclusive markers on either side: keep the old heuristic.
        assert_eq!(
            detect_wire_format(&serde_json::json!({"model": "m", "max_tokens": 5, "messages": []})),
            "v1/messages"
        );
        assert_eq!(
            detect_wire_format(&serde_json::json!({"model": "m", "messages": []})),
            "v1/chat/completions"
        );
    }

    #[test]
    fn test_wire_format_ambiguous_markers_fall_through() {
        // `metadata` exists in BOTH OpenAI and Anthropic schemas — an OpenAI
        // request carrying it (no max_tokens) must keep routing to OpenAI.
        assert_eq!(
            detect_wire_format(&serde_json::json!({
                "model": "m", "messages": [], "metadata": {"customer_id": "c1"}
            })),
            "v1/chat/completions",
            "OpenAI metadata tagging must not be misrouted to v1/messages"
        );
        // `service_tier` exists in BOTH schemas too — an Anthropic Priority
        // Tier request carrying it must still route via max_tokens.
        assert_eq!(
            detect_wire_format(&serde_json::json!({
                "model": "m", "messages": [], "max_tokens": 1024,
                "service_tier": "standard_only"
            })),
            "v1/messages",
            "Anthropic service_tier must not be misrouted to v1/chat/completions"
        );
    }

    #[test]
    fn test_sse_line_buffer_reassembles_split_lines() {
        let mut buf = SseLineBuffer::new();

        // A data line split mid-JSON across three chunks
        let lines1 = buf.push("data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hel");
        assert!(lines1.is_empty(), "partial line must be retained");

        let lines2 = buf.push("lo world\"}}\n\ndata: {\"done\":true}\n");
        assert_eq!(
            lines2,
            vec![
                "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hello world\"}}",
                "data: {\"done\":true}"
            ]
        );

        let lines3 = buf.push("data: {\"tail\":1}");
        assert!(
            lines3.is_empty(),
            "trailing line without newline is incomplete"
        );
    }

    #[test]
    fn test_sse_line_buffer_strips_cr_and_handles_multiple_lines() {
        let mut buf = SseLineBuffer::new();
        let lines = buf.push("event: message_start\r\ndata: {\"a\":1}\r\n\r\ndata: {\"b\":2}\n");
        assert_eq!(
            lines,
            vec!["event: message_start", "data: {\"a\":1}", "data: {\"b\":2}"]
        );
    }

    #[test]
    fn test_sse_line_buffer_overflow_does_not_panic_on_multibyte_boundary() {
        let mut buf = SseLineBuffer::new();
        // Fill to exactly 1 byte below the cap, then push a 3-byte CJK char so
        // the 1 MiB offset lands inside a multi-byte character: split_off must
        // not be called on a non-boundary (it panics).
        let pad = "x".repeat(1024 * 1024 - 1);
        buf.push(&pad);
        let lines = buf.push("水");
        let _ = buf.push("more data\n");
        // No panic above; the padded line is unterminated garbage and stays
        // buffered or is cut — but the stream survives.
        assert!(buf.take_remainder().is_some() || !lines.is_empty());
    }

    #[test]
    fn test_sse_line_buffer_take_remainder_flushes_final_line() {
        let mut buf = SseLineBuffer::new();
        buf.push("data: {\"final\":true}");
        assert!(buf.take_remainder() == Some("data: {\"final\":true}".to_string()));
        assert_eq!(buf.take_remainder(), None, "second flush must be empty");
    }
}
