use axum::response::sse::Event;
use serde_json::Value;
use std::collections::HashMap;
use tracing::debug;
use uuid::Uuid;

pub fn anthropic_request_to_openai(anthropic: &Value) -> Value {
    let mut messages = Vec::new();

    if let Some(system) = anthropic.get("system") {
        let system_text = extract_text_from_content(system);
        if !system_text.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system_text
            }));
        }
    }

    if let Some(msgs) = anthropic.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = msg.get("content");
            let expanded = convert_anthropic_message_expanded(role, content);
            messages.extend(expanded);
        }
    }

    let mut openai = serde_json::json!({
        "model": anthropic.get("model").unwrap_or(&Value::Null),
        "messages": messages,
    });

    if let Some(v) = anthropic.get("max_tokens") {
        openai["max_tokens"] = v.clone();
    }
    if let Some(v) = anthropic.get("temperature") {
        openai["temperature"] = v.clone();
    }
    if let Some(v) = anthropic.get("stream") {
        openai["stream"] = v.clone();
    }
    if let Some(v) = anthropic.get("top_p") {
        openai["top_p"] = v.clone();
    }
    if let Some(v) = anthropic.get("stop_sequences") {
        openai["stop"] = v.clone();
    }

    if let Some(tools) = anthropic.get("tools").and_then(|t| t.as_array()) {
        let openai_tools: Vec<Value> = tools
            .iter()
            .filter_map(convert_anthropic_tool_to_openai)
            .collect();
        if !openai_tools.is_empty() {
            openai["tools"] = Value::Array(openai_tools);
        }
    }

    if let Some(tc) = anthropic.get("tool_choice") {
        openai["tool_choice"] = convert_anthropic_tool_choice(tc);
    }

    openai
}

pub fn openai_response_to_anthropic(openai: &Value) -> Value {
    let choice = openai
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());
    let message = choice.and_then(|c| c.get("message"));

    let mut content: Vec<Value> = Vec::new();

    if let Some(msg) = message {
        if let Some(reasoning) = msg.get("reasoning_content").and_then(|c| c.as_str()) {
            if !reasoning.is_empty() {
                content.push(serde_json::json!({
                    "type": "thinking",
                    "thinking": reasoning
                }));
            }
        }

        if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
            if !text.is_empty() {
                content.push(serde_json::json!({
                    "type": "text",
                    "text": text
                }));
            }
        }

        if let Some(tool_calls) = msg.get("tool_calls").and_then(|t| t.as_array()) {
            for tc in tool_calls {
                let input = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|a| a.as_str())
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                    .unwrap_or(Value::Object(serde_json::Map::new()));

                content.push(serde_json::json!({
                    "type": "tool_use",
                    "id": tc.get("id").unwrap_or(&Value::Null),
                    "name": tc.get("function").and_then(|f| f.get("name")).unwrap_or(&Value::Null),
                    "input": input
                }));
            }
        }
    }

    if content.is_empty() {
        content.push(serde_json::json!({
            "type": "text",
            "text": ""
        }));
    }

    let stop_reason = choice
        .and_then(|c| c.get("finish_reason").and_then(|f| f.as_str()))
        .map(|r| match r {
            "stop" => "end_turn",
            "length" => "max_tokens",
            "tool_calls" => "tool_use",
            _ => "end_turn",
        })
        .unwrap_or("end_turn");

    let usage = openai.get("usage");

    serde_json::json!({
        "id": format!("msg_{}", Uuid::new_v4().simple()),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": openai.get("model").unwrap_or(&Value::Null),
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": usage.and_then(|u| u.get("prompt_tokens")).unwrap_or(&serde_json::json!(0)),
            "output_tokens": usage.and_then(|u| u.get("completion_tokens")).unwrap_or(&serde_json::json!(0)),
        }
    })
}

fn extract_text_from_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn convert_anthropic_message_expanded(role: &str, content: Option<&Value>) -> Vec<Value> {
    let mut messages = Vec::new();

    if role == "assistant" {
        if let Some(Value::Array(blocks)) = content {
            let mut has_tool_use = false;
            let mut text_parts: Vec<Value> = Vec::new();
            let mut tool_calls: Vec<Value> = Vec::new();
            let mut tc_index: usize = 0;

            for b in blocks {
                let block_type = b.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                match block_type {
                    "text" => {
                        if let Some(text) = b.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(serde_json::json!({
                                "type": "text",
                                "text": text
                            }));
                        }
                    }
                    "tool_use" => {
                        has_tool_use = true;
                        let id = b
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = b
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input = b
                            .get("input")
                            .cloned()
                            .unwrap_or(Value::Object(serde_json::Map::new()));
                        let arguments = serde_json::to_string(&input).unwrap_or_default();

                        tool_calls.push(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "index": tc_index,
                            "function": {
                                "name": name,
                                "arguments": arguments
                            }
                        }));
                        tc_index += 1;
                    }
                    _ => {}
                }
            }

            if has_tool_use {
                let content_val = if text_parts.is_empty() {
                    Value::Null
                } else {
                    let text: String = text_parts
                        .iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    Value::String(text)
                };

                messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": content_val,
                    "tool_calls": tool_calls
                }));
                return messages;
            }
        }

        messages.push(convert_anthropic_message(role, content));
        return messages;
    }

    if let Some(Value::Array(blocks)) = content {
        let mut tool_results: Vec<Value> = Vec::new();
        let mut non_tool_blocks: Vec<&Value> = Vec::new();

        for b in blocks {
            let block_type = b.get("type").and_then(|t| t.as_str()).unwrap_or("text");
            if block_type == "tool_result" {
                let tool_use_id = b
                    .get("tool_use_id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut result_text =
                    extract_text_from_content(b.get("content").unwrap_or(&Value::Null));
                if b.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false) {
                    result_text = format!("Error: {}", result_text);
                }
                tool_results.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
                    "content": result_text
                }));
            } else {
                non_tool_blocks.push(b);
            }
        }

        if !tool_results.is_empty() {
            if !non_tool_blocks.is_empty() {
                let user_content = convert_blocks_to_openai_content(&non_tool_blocks);
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": user_content
                }));
            }
            messages.extend(tool_results);
            return messages;
        }
    }

    messages.push(convert_anthropic_message(role, content));
    messages
}

fn convert_blocks_to_openai_content(blocks: &[&Value]) -> Value {
    let parts: Vec<Value> = blocks
        .iter()
        .map(|b| {
            let block_type = b.get("type").and_then(|t| t.as_str()).unwrap_or("text");
            match block_type {
                "text" => serde_json::json!({
                    "type": "text",
                    "text": b.get("text").unwrap_or(&Value::Null)
                }),
                "image" => {
                    let source = b.get("source").unwrap_or(&Value::Null);
                    serde_json::json!({
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}",
                                source.get("media_type").and_then(|m| m.as_str()).unwrap_or("image/png"),
                                source.get("data").and_then(|d| d.as_str()).unwrap_or("")
                            )
                        }
                    })
                }
                _ => serde_json::json!({
                    "type": "text",
                    "text": serde_json::to_string(*b).unwrap_or_default()
                }),
            }
        })
        .collect();
    Value::Array(parts)
}

fn convert_anthropic_message(role: &str, content: Option<&Value>) -> Value {
    let openai_content = match content {
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(Value::Array(blocks)) => {
            let has_non_text = blocks
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) != Some("text"));
            if has_non_text {
                let parts: Vec<Value> = blocks
                    .iter()
                    .map(|b| {
                        let block_type = b.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                        match block_type {
                            "text" => serde_json::json!({
                                "type": "text",
                                "text": b.get("text").unwrap_or(&Value::Null)
                            }),
                            "image" => {
                                let source = b.get("source").unwrap_or(&Value::Null);
                                serde_json::json!({
                                    "type": "image_url",
                                    "image_url": {
                                        "url": format!("data:{};base64,{}",
                                            source.get("media_type").and_then(|m| m.as_str()).unwrap_or("image/png"),
                                            source.get("data").and_then(|d| d.as_str()).unwrap_or("")
                                        )
                                    }
                                })
                            }
                            "tool_result" => {
                                let text = extract_text_from_content(b.get("content").unwrap_or(&Value::Null));
                                serde_json::json!({
                                    "type": "text",
                                    "text": text
                                })
                            }
                            _ => serde_json::json!({
                                "type": "text",
                                "text": serde_json::to_string(b).unwrap_or_default()
                            }),
                        }
                    })
                    .collect();
                Value::Array(parts)
            } else {
                let text: String = blocks
                    .iter()
                    .filter_map(|b| {
                        b.get("text")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Value::String(text)
            }
        }
        Some(v) => v.clone(),
        None => Value::String(String::new()),
    };

    serde_json::json!({
        "role": role,
        "content": openai_content
    })
}

fn convert_anthropic_tool_to_openai(tool: &Value) -> Option<Value> {
    let name = tool.get("name").and_then(|n| n.as_str())?;
    let description = tool
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("");
    let input_schema = tool.get("input_schema").unwrap_or(&Value::Null);

    Some(serde_json::json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": input_schema
        }
    }))
}

fn convert_anthropic_tool_choice(tc: &Value) -> Value {
    match tc {
        Value::String(s) => match s.as_str() {
            "auto" => serde_json::json!("auto"),
            "any" => serde_json::json!("required"),
            "none" => serde_json::json!("none"),
            _ => tc.clone(),
        },
        Value::Object(obj) => {
            if obj.get("type").and_then(|t| t.as_str()) == Some("tool") {
                if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
                    return serde_json::json!({
                        "type": "function",
                        "function": {"name": name}
                    });
                }
            }
            tc.clone()
        }
        _ => tc.clone(),
    }
}

#[derive(Debug, Clone)]
pub struct SseEventData {
    pub event_type: String,
    pub data: String,
}

impl From<SseEventData> for Event {
    fn from(sse: SseEventData) -> Event {
        Event::default().event(sse.event_type).data(sse.data)
    }
}

pub struct OpenAIToAnthropicStreamConverter {
    msg_id: String,
    model: String,
    message_started: bool,
    message_ended: bool,
    content_block_index: usize,
    text_block_open: bool,
    thinking_block_open: bool,
    tool_blocks: HashMap<usize, ToolBlockState>,
    tool_block_order: Vec<usize>,
    input_tokens: u32,
    output_tokens: u32,
    line_buffer: String,
}

const MAX_LINE_BUFFER: usize = 1024 * 1024;

struct ToolBlockState {
    id: String,
    name: String,
    block_index: usize,
}

impl OpenAIToAnthropicStreamConverter {
    pub fn new(model: String) -> Self {
        Self {
            msg_id: format!("msg_{}", Uuid::new_v4().simple()),
            model,
            message_started: false,
            message_ended: false,
            content_block_index: 0,
            text_block_open: false,
            thinking_block_open: false,
            tool_blocks: HashMap::new(),
            tool_block_order: Vec::new(),
            input_tokens: 0,
            output_tokens: 0,
            line_buffer: String::new(),
        }
    }

    pub fn convert_chunk(&mut self, raw_chunk: &str) -> Vec<SseEventData> {
        let mut events = Vec::new();
        self.line_buffer.push_str(raw_chunk);

        if self.line_buffer.len() > MAX_LINE_BUFFER {
            tracing::warn!(
                "Stream line buffer overflow ({} bytes), closing stream",
                self.line_buffer.len()
            );
            self.line_buffer.clear();
            let mut events = Vec::new();
            events.push(make_sse_event(
                "error",
                &serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": "Stream buffer overflow"
                    }
                }),
            ));
            events.extend(self.close_stream());
            return events;
        }

        while let Some(newline_pos) = self.line_buffer.find('\n') {
            let line = self.line_buffer[..newline_pos]
                .trim_end_matches('\r')
                .to_string();
            self.line_buffer = self.line_buffer[newline_pos + 1..].to_string();

            if line.starts_with("event: ") {
                continue;
            }

            if let Some(json_data) = line.strip_prefix("data: ") {
                if json_data.trim() == "[DONE]" {
                    debug!("Stream converter: [DONE] received");
                    events.extend(self.close_stream());
                    continue;
                }

                if let Ok(value) = serde_json::from_str::<Value>(json_data) {
                    events.extend(self.process_openai_chunk(&value));
                }
            }
        }

        events
    }

    fn process_openai_chunk(&mut self, value: &Value) -> Vec<SseEventData> {
        let mut events = Vec::new();

        if !self.message_started {
            self.message_started = true;
            events.push(make_sse_event(
                "message_start",
                &serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": self.msg_id,
                        "type": "message",
                        "role": "assistant",
                        "content": [],
                        "model": self.model,
                        "stop_reason": Value::Null,
                        "stop_sequence": Value::Null,
                        "usage": {
                            "input_tokens": self.input_tokens,
                            "output_tokens": 0
                        }
                    }
                }),
            ));
        }

        if let Some(usage) = value.get("usage") {
            if let Some(t) = usage.get("prompt_tokens").and_then(|t| t.as_u64()) {
                self.input_tokens = t as u32;
            }
        }

        let choice = value
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first());
        let delta = choice.and_then(|c| c.get("delta"));

        if let Some(delta) = delta {
            if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                if !reasoning.is_empty() {
                    if !self.thinking_block_open {
                        self.thinking_block_open = true;
                        events.push(make_sse_event(
                            "content_block_start",
                            &serde_json::json!({
                                "type": "content_block_start",
                                "index": self.content_block_index,
                                "content_block": {"type": "thinking", "thinking": ""}
                            }),
                        ));
                    }
                    events.push(make_sse_event(
                        "content_block_delta",
                        &serde_json::json!({
                            "type": "content_block_delta",
                            "index": self.content_block_index,
                            "delta": {"type": "thinking_delta", "thinking": reasoning}
                        }),
                    ));
                    self.output_tokens += 1;
                }
            }

            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                if !content.is_empty() {
                    if self.thinking_block_open {
                        self.thinking_block_open = false;
                        events.push(make_sse_event(
                            "content_block_stop",
                            &serde_json::json!({
                                "type": "content_block_stop",
                                "index": self.content_block_index
                            }),
                        ));
                        self.content_block_index += 1;
                    }
                    if !self.text_block_open {
                        self.text_block_open = true;
                        events.push(make_sse_event(
                            "content_block_start",
                            &serde_json::json!({
                                "type": "content_block_start",
                                "index": self.content_block_index,
                                "content_block": {"type": "text", "text": ""}
                            }),
                        ));
                    }
                    events.push(make_sse_event(
                        "content_block_delta",
                        &serde_json::json!({
                            "type": "content_block_delta",
                            "index": self.content_block_index,
                            "delta": {"type": "text_delta", "text": content}
                        }),
                    ));
                    self.output_tokens += 1;
                }
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                if self.thinking_block_open {
                    self.thinking_block_open = false;
                    events.push(make_sse_event(
                        "content_block_stop",
                        &serde_json::json!({
                            "type": "content_block_stop",
                            "index": self.content_block_index
                        }),
                    ));
                    self.content_block_index += 1;
                }
                if self.text_block_open {
                    self.text_block_open = false;
                    events.push(make_sse_event(
                        "content_block_stop",
                        &serde_json::json!({
                            "type": "content_block_stop",
                            "index": self.content_block_index
                        }),
                    ));
                    self.content_block_index += 1;
                }

                for tc in tool_calls {
                    let tc_index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

                    if tc.get("id").is_some() {
                        let id = tc
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();

                        let block_index = self.content_block_index;
                        self.content_block_index += 1;

                        self.tool_blocks.insert(
                            tc_index,
                            ToolBlockState {
                                id,
                                name,
                                block_index,
                            },
                        );
                        self.tool_block_order.push(tc_index);

                        let tool_state = self.tool_blocks.get(&tc_index).unwrap();
                        events.push(make_sse_event(
                            "content_block_start",
                            &serde_json::json!({
                                "type": "content_block_start",
                                "index": tool_state.block_index,
                                "content_block": {
                                    "type": "tool_use",
                                    "id": tool_state.id,
                                    "name": tool_state.name,
                                    "input": {}
                                }
                            }),
                        ));
                    }

                    if let Some(args) = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|a| a.as_str())
                    {
                        if !args.is_empty() {
                            if let Some(tool_state) = self.tool_blocks.get(&tc_index) {
                                events.push(make_sse_event(
                                    "content_block_delta",
                                    &serde_json::json!({
                                        "type": "content_block_delta",
                                        "index": tool_state.block_index,
                                        "delta": {
                                            "type": "input_json_delta",
                                            "partial_json": args
                                        }
                                    }),
                                ));
                            }
                        }
                    }
                }
            }
        }

        if let Some(reason) = choice
            .and_then(|c| c.get("finish_reason"))
            .and_then(|f| f.as_str())
        {
            if reason != "null" && !reason.is_empty() {
                events.extend(self.close_with_stop_reason(reason));
            }
        }

        events
    }

    fn close_open_blocks(&mut self) -> Vec<SseEventData> {
        let mut events = Vec::new();

        if self.thinking_block_open {
            self.thinking_block_open = false;
            events.push(make_sse_event(
                "content_block_stop",
                &serde_json::json!({
                    "type": "content_block_stop",
                    "index": self.content_block_index
                }),
            ));
            self.content_block_index += 1;
        }

        if self.text_block_open {
            self.text_block_open = false;
            events.push(make_sse_event(
                "content_block_stop",
                &serde_json::json!({
                    "type": "content_block_stop",
                    "index": self.content_block_index
                }),
            ));
            self.content_block_index += 1;
        }

        for &tc_idx in &self.tool_block_order {
            if let Some(tool_state) = self.tool_blocks.get(&tc_idx) {
                events.push(make_sse_event(
                    "content_block_stop",
                    &serde_json::json!({
                        "type": "content_block_stop",
                        "index": tool_state.block_index
                    }),
                ));
            }
        }

        events
    }

    fn close_with_stop_reason(&mut self, reason: &str) -> Vec<SseEventData> {
        if self.message_ended {
            return vec![];
        }
        self.message_ended = true;
        let mut events = self.close_open_blocks();

        let stop_reason = match reason {
            "stop" => "end_turn",
            "length" => "max_tokens",
            "tool_calls" => "tool_use",
            _ => "end_turn",
        };

        events.push(make_sse_event(
            "message_delta",
            &serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": stop_reason, "stop_sequence": Value::Null},
                "usage": {"output_tokens": self.output_tokens}
            }),
        ));

        events.push(make_sse_event(
            "message_stop",
            &serde_json::json!({"type": "message_stop"}),
        ));

        events
    }

    fn close_stream(&mut self) -> Vec<SseEventData> {
        if self.message_started {
            self.close_with_stop_reason("stop")
        } else {
            vec![]
        }
    }
}

fn make_sse_event(event_type: &str, data: &Value) -> SseEventData {
    SseEventData {
        event_type: event_type.to_string(),
        data: data.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_anthropic_to_openai_basic() {
        let anthropic = json!({
            "model": "glm-5",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 1024
        });
        let openai = anthropic_request_to_openai(&anthropic);
        assert_eq!(openai["model"], "glm-5");
        assert_eq!(openai["max_tokens"], 1024);
        assert_eq!(openai["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_anthropic_to_openai_with_system() {
        let anthropic = json!({
            "model": "glm-5",
            "system": "You are helpful",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let openai = anthropic_request_to_openai(&anthropic);
        assert_eq!(openai["messages"][0]["role"], "system");
        assert_eq!(openai["messages"][1]["role"], "user");
    }

    #[test]
    fn test_openai_to_anthropic_response() {
        let openai = json!({
            "id": "chatcmpl-123",
            "model": "glm-5",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });
        let anthropic = openai_response_to_anthropic(&openai);
        assert_eq!(anthropic["type"], "message");
        assert_eq!(anthropic["stop_reason"], "end_turn");
        assert_eq!(anthropic["content"][0]["type"], "text");
        assert_eq!(anthropic["content"][0]["text"], "Hello!");
    }

    #[test]
    fn test_openai_response_tool_calls() {
        let openai = json!({
            "id": "chatcmpl-123",
            "model": "glm-5",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": "{\"location\":\"NYC\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 20}
        });
        let anthropic = openai_response_to_anthropic(&openai);
        assert_eq!(anthropic["stop_reason"], "tool_use");
        assert_eq!(anthropic["content"][0]["type"], "tool_use");
        assert_eq!(anthropic["content"][0]["name"], "get_weather");
    }

    #[test]
    fn test_stream_converter_line_buffer_across_chunks() {
        let mut conv = OpenAIToAnthropicStreamConverter::new("test-model".to_string());
        let events1 = conv.convert_chunk("data: ");
        assert!(events1.is_empty());

        let chunk = r#"{"id":"1","object":"chat.completion.chunk","model":"test","choices":[{"index":0,"delta":{"role":"assistant","content":""}}]}"#;
        let events2 = conv.convert_chunk(&format!("{}\n\n", chunk));
        assert!(!events2.is_empty());
        assert!(events2.iter().any(|e| e.event_type == "message_start"));
    }

    #[test]
    fn test_stream_converter_text_only() {
        let mut conv = OpenAIToAnthropicStreamConverter::new("test-model".to_string());

        let chunk1 = r#"data: {"id":"1","object":"chat.completion.chunk","model":"test","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"}}]}"#;
        let events1 = conv.convert_chunk(&format!("{}\n\n", chunk1));
        assert!(events1.iter().any(|e| e.event_type == "message_start"));
        assert!(events1.iter().any(|e| e.event_type == "content_block_start"));
        assert!(events1.iter().any(|e| e.event_type == "content_block_delta"));

        let chunk2 = r#"data: {"id":"1","object":"chat.completion.chunk","model":"test","choices":[{"index":0,"finish_reason":"stop","delta":{}}]}"#;
        let events2 = conv.convert_chunk(&format!("{}\n\n", chunk2));
        assert!(events2.iter().any(|e| e.event_type == "content_block_stop"));
        assert!(events2.iter().any(|e| e.event_type == "message_delta"));
        assert!(events2.iter().any(|e| e.event_type == "message_stop"));
    }
}
