use serde::{Deserialize, Serialize};

/// Anthropic messages request
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
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
pub struct ContentBlock {
    pub r#type: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}
