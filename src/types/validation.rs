// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

/// Validates model name
pub fn validate_model_name(name: &str) -> Result<(), String> {
    if name.len() > 128 {
        Err(format!(
            "Model name too long ({} chars, max 128)",
            name.len()
        ))
    } else {
        Ok(())
    }
}

/// Validates role string for Anthropic API
/// Valid roles: system, user, assistant
pub fn validate_role(role: &str) -> Result<(), String> {
    match role {
        "system" | "user" | "assistant" => Ok(()),
        _ => Err("Invalid role. Must be 'system', 'user', or 'assistant'".to_string()),
    }
}

/// Validates message content
pub fn validate_message_content(content: &str) -> Result<(), String> {
    if content.len() > 1_000_000 {
        Err(format!(
            "Message too long ({} chars, max 1MB)",
            content.len()
        ))
    } else {
        Ok(())
    }
}
