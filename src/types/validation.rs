// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

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

    if name.contains("..") {
        return Err("Model name cannot contain '..'".to_string());
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        return Err("Model name contains invalid characters. Only ASCII alphanumeric, '-', '_', '.', and '/' are allowed".to_string());
    }

    Ok(())
}

pub fn validate_role(role: &str) -> Result<(), String> {
    match role {
        "system" | "user" | "assistant" | "tool" => Ok(()),
        _ => Err(format!(
            "Invalid role '{}'. Must be 'system', 'user', 'assistant', or 'tool'",
            role
        )),
    }
}

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

pub fn validate_max_tokens(max_tokens: u32) -> Result<(), String> {
    if max_tokens == 0 {
        return Err("max_tokens must be greater than 0".to_string());
    }
    if max_tokens > 128_000 {
        return Err(format!("max_tokens too large ({}, max 128000)", max_tokens));
    }
    Ok(())
}

pub fn validate_temperature(temperature: f32) -> Result<(), String> {
    if temperature.is_nan() {
        return Err("temperature must be a valid number".to_string());
    }
    if temperature < 0.0 {
        return Err("temperature must be non-negative".to_string());
    }
    if temperature > 2.0 {
        return Err(format!("temperature too high ({}, max 2.0)", temperature));
    }
    Ok(())
}

pub fn validate_top_p(top_p: f32) -> Result<(), String> {
    if !(0.0..=1.0).contains(&top_p) {
        return Err(format!("top_p must be between 0.0 and 1.0, got {}", top_p));
    }
    Ok(())
}
