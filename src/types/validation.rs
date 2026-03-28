// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

/// Validates model name
pub fn validate_model_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Model name cannot be empty".to_string());
    }

    if name.len() > 128 {
        return Err(format!(
            "Model name too long ({} chars, max 128)",
            name.len()
        ));
    }

    // Block path traversal attempts
    if name.contains("..") {
        return Err("Model name cannot contain '..'".to_string());
    }

    // Allow ASCII alphanumeric, hyphens, underscores, dots, and forward slashes
    // This covers common model naming patterns like "gpt-4", "claude-3-opus", "provider/model"
    // Using is_ascii_alphanumeric() to reject unicode characters
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        return Err("Model name contains invalid characters. Only ASCII alphanumeric, '-', '_', '.', and '/' are allowed".to_string());
    }

    Ok(())
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
