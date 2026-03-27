use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Anthropic messages request
/// Uses flatten to pass through all fields we don't explicitly handle
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageRequest {
    pub model: String,
    #[serde(default)]
    pub max_tokens: u32,
    pub messages: Vec<Message>,
    /// System prompt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>,
    /// Tools available to the model
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    /// Tool choice configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Metadata for the request (e.g., user_id)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    /// Capture any other fields we don't explicitly handle
    #[serde(flatten)]
    pub other: HashMap<String, Value>,
}

/// Message content - can be a simple string or array of content blocks
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Content {
    /// Simple string content
    Text(String),
    /// Array of content blocks (for multimodal, tool_use, tool_result, etc.)
    Blocks(Vec<ContentBlock>),
}

impl Content {
    /// Get the text content, regardless of format
    pub fn as_text(&self) -> String {
        match self {
            Content::Text(s) => s.clone(),
            Content::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| {
                    if b.r#type == "text" {
                        b.text.as_ref().cloned()
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: String,
    pub content: Content,
}

/// Content block that supports all Anthropic content types:
/// - text: has `type`, `text`
/// - tool_use: has `type`, `id`, `name`, `input`
/// - tool_result: has `type`, `tool_use_id`, `content`, `is_error`
/// - thinking: has `type`, `thinking`
/// - image: has `type`, `source`
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContentBlock {
    /// The content block type (text, tool_use, tool_result, thinking, image, etc.)
    #[serde(rename = "type")]
    pub r#type: String,
    /// Text content (for type="text")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Tool use ID (for type="tool_use" and type="tool_result")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Tool name (for type="tool_use")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Tool input (for type="tool_use")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// Tool use ID reference (for type="tool_result")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// Tool result content (for type="tool_result")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    /// Error flag (for type="tool_result")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Thinking content (for type="thinking")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// Image source (for type="image")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Value>,
    /// Capture any other fields we don't explicitly handle
    #[serde(flatten)]
    pub other: HashMap<String, Value>,
}

/// Anthropic messages response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageResponse {
    pub id: String,
    pub r#type: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub stop_reason: String,
    pub usage: MessageUsage,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}
