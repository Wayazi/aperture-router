// SPDX-License-Identifier: MIT
// Copyright (c) 2025 aperture-router contributors

use aperture_router::types::anthropic::{Message, MessageRequest};
use aperture_router::types::openai::{ChatCompletionRequest, ChatMessage};
use serde_json::Value;
use std::collections::HashMap;

#[cfg(test)]
mod type_tests {
    use super::*;

    #[test]
    fn test_chat_completion_request_serialization() {
        let request = ChatCompletionRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: Some(Value::String("You are a helpful assistant.".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    other: HashMap::new(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(Value::String("Hello!".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    other: HashMap::new(),
                },
            ],
            temperature: Some(0.7),
            max_tokens: Some(100),
            stream: Some(false),
            tools: None,
            tool_choice: None,
            other: HashMap::new(),
        };

        let json = serde_json::to_string(&request).expect("Failed to serialize");

        assert!(json.contains("\"model\":\"gpt-3.5-turbo\""));
        assert!(json.contains("\"temperature\":0.7"));
        assert!(json.contains("\"max_tokens\":100"));
        assert!(json.contains("\"stream\":false"));
    }

    #[test]
    fn test_chat_completion_request_deserialization() {
        let json = r#"
            {
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Test"}
                ],
                "temperature": 0.5,
                "max_tokens": 200,
                "stream": true
            }
        "#;

        let request: ChatCompletionRequest =
            serde_json::from_str(json).expect("Failed to deserialize");

        assert_eq!(request.model, "gpt-4");
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.temperature, Some(0.5));
        assert_eq!(request.max_tokens, Some(200));
        assert_eq!(request.stream, Some(true));
    }

    #[test]
    fn test_chat_completion_request_minimal() {
        let json = r#"
            {
                "model": "gpt-3.5-turbo",
                "messages": [{"role": "user", "content": "Hello"}]
            }
        "#;

        let request: ChatCompletionRequest =
            serde_json::from_str(json).expect("Failed to deserialize");

        assert_eq!(request.model, "gpt-3.5-turbo");
        assert_eq!(request.messages.len(), 1);
        assert!(request.temperature.is_none());
        assert!(request.max_tokens.is_none());
        assert!(request.stream.is_none());
    }

    #[test]
    fn test_anthropic_message_request_serialization() {
        use aperture_router::types::anthropic::Content;

        let request = MessageRequest {
            model: "claude-3-sonnet-20240229".to_string(),
            max_tokens: 100,
            messages: vec![Message {
                role: "user".to_string(),
                content: Content::Text("Hello Claude!".to_string()),
            }],
            system: None,
            tools: None,
            tool_choice: None,
            temperature: Some(0.7),
            stream: None,
            metadata: None,
            other: HashMap::new(),
        };

        let json = serde_json::to_string(&request).expect("Failed to serialize");

        assert!(json.contains("\"model\":\"claude-3-sonnet-20240229\""));
        assert!(json.contains("\"max_tokens\":100"));
        assert!(json.contains("\"temperature\":0.7"));
    }

    #[test]
    fn test_anthropic_message_request_deserialization() {
        let json = r#"
            {
                "model": "claude-3-opus-20240229",
                "max_tokens": 200,
                "messages": [
                    {"role": "user", "content": "Test message"}
                ],
                "temperature": 0.5
            }
        "#;

        let request: MessageRequest = serde_json::from_str(json).expect("Failed to deserialize");

        assert_eq!(request.model, "claude-3-opus-20240229");
        assert_eq!(request.max_tokens, 200);
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.temperature, Some(0.5));
    }

    #[test]
    fn test_chat_completion_request_with_multiple_messages() {
        let request = ChatCompletionRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: Some(Value::String("System prompt".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    other: HashMap::new(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(Value::String("First user message".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    other: HashMap::new(),
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(Value::String("Assistant response".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    other: HashMap::new(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(Value::String("Second user message".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    other: HashMap::new(),
                },
            ],
            temperature: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            other: HashMap::new(),
        };

        assert_eq!(request.messages.len(), 4);
    }

    #[test]
    fn test_chat_completion_request_edge_cases() {
        // Empty messages
        let request = ChatCompletionRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: vec![],
            temperature: Some(0.0),
            max_tokens: Some(0),
            stream: Some(false),
            tools: None,
            tool_choice: None,
            other: HashMap::new(),
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: ChatCompletionRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.temperature, Some(0.0));
        assert_eq!(deserialized.max_tokens, Some(0));
    }

    #[test]
    fn test_chat_completion_request_large_values() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(Value::String("Test".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                other: HashMap::new(),
            }],
            temperature: Some(2.0),
            max_tokens: Some(4096),
            stream: Some(true),
            tools: None,
            tool_choice: None,
            other: HashMap::new(),
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: ChatCompletionRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.temperature, Some(2.0));
        assert_eq!(deserialized.max_tokens, Some(4096));
    }

    #[test]
    fn test_anthropic_message_request_minimal() {
        let json = r#"
            {
                "model": "claude-3-sonnet-20240229",
                "max_tokens": 100,
                "messages": [{"role": "user", "content": "Test"}]
            }
        "#;

        let request: MessageRequest = serde_json::from_str(json).expect("Failed to deserialize");

        assert_eq!(request.model, "claude-3-sonnet-20240229");
        assert_eq!(request.max_tokens, 100);
        assert!(request.temperature.is_none());
    }

    #[test]
    fn test_serialization_preserves_model_name() {
        let request = ChatCompletionRequest {
            model: "gpt-4-turbo-preview".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(Value::String("Test".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                other: HashMap::new(),
            }],
            temperature: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            other: HashMap::new(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("gpt-4-turbo-preview"));

        let deserialized: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model, "gpt-4-turbo-preview");
    }

    #[test]
    fn test_invalid_json_handling() {
        let invalid_json = r#"{"model": "gpt-3.5-turbo", "messages": [invalid]}"#;

        let result: Result<ChatCompletionRequest, _> = serde_json::from_str(invalid_json);
        assert!(result.is_err(), "Invalid JSON should fail to deserialize");
    }

    #[test]
    fn test_missing_required_field() {
        let missing_model = r#"{"messages": [{"role": "user", "content": "Test"}]}"#;

        let result: Result<ChatCompletionRequest, _> = serde_json::from_str(missing_model);
        assert!(result.is_err(), "Missing 'model' field should fail");
    }

    #[test]
    fn test_tool_calls_in_message() {
        let json = r#"
            {
                "model": "gpt-4",
                "messages": [
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [
                            {
                                "id": "call_123",
                                "type": "function",
                                "function": {
                                    "name": "get_weather",
                                    "arguments": "{\"location\": \"SF\"}"
                                }
                            }
                        ]
                    }
                ]
            }
        "#;

        let request: ChatCompletionRequest =
            serde_json::from_str(json).expect("Failed to deserialize");
        assert!(request.messages[0].tool_calls.is_some());
    }
}
