// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use aperture_router::types::validation::{
    validate_message_content, validate_model_name, validate_role,
};

#[test]
fn test_validate_model_name_valid() {
    assert!(validate_model_name("gpt-4").is_ok());
    assert!(validate_model_name("claude-3-opus").is_ok());
    assert!(validate_model_name("glm-4.7").is_ok());
    assert!(validate_model_name("provider/model").is_ok());
    assert!(validate_model_name("model_v2").is_ok());
    assert!(validate_model_name("a").is_ok());
}

#[test]
fn test_validate_model_name_empty() {
    assert!(validate_model_name("").is_err());
}

#[test]
fn test_validate_model_name_too_long() {
    let long_name = "a".repeat(129);
    let result = validate_model_name(&long_name);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("too long"));
}

#[test]
fn test_validate_model_name_max_length() {
    let max_name = "a".repeat(128);
    assert!(validate_model_name(&max_name).is_ok());
}

#[test]
fn test_validate_model_name_injection_attempts() {
    assert!(validate_model_name("model<script>").is_err());
    assert!(validate_model_name("model'or'1'='1").is_err());
    assert!(validate_model_name("model; DROP TABLE").is_err());
    assert!(validate_model_name("model\"quote").is_err());
    assert!(validate_model_name("model\\backslash").is_err());
}

#[test]
fn test_validate_model_name_path_traversal() {
    assert!(validate_model_name("../../../etc/passwd").is_err());
    assert!(validate_model_name("..\\..\\windows").is_err());
}

#[test]
fn test_validate_model_name_special_chars() {
    assert!(validate_model_name("model@host").is_err());
    assert!(validate_model_name("model#tag").is_err());
    assert!(validate_model_name("model$var").is_err());
    assert!(validate_model_name("model%enc").is_err());
    assert!(validate_model_name("model&amp").is_err());
    assert!(validate_model_name("model!bang").is_err());
}

#[test]
fn test_validate_model_name_unicode() {
    assert!(validate_model_name("模型-4").is_err());
    assert!(validate_model_name("model🎉").is_err());
    assert!(validate_model_name("модель").is_err());
}

#[test]
fn test_validate_model_name_spaces() {
    assert!(validate_model_name("model name").is_err());
    assert!(validate_model_name(" model").is_err());
    assert!(validate_model_name("model ").is_err());
}

#[test]
fn test_validate_role_valid() {
    assert!(validate_role("system").is_ok());
    assert!(validate_role("user").is_ok());
    assert!(validate_role("assistant").is_ok());
}

#[test]
fn test_validate_role_invalid() {
    assert!(validate_role("admin").is_err());
    assert!(validate_role("bot").is_err());
    assert!(validate_role("SYSTEM").is_err());
    assert!(validate_role("User").is_err());
    assert!(validate_role("").is_err());
}

#[test]
fn test_validate_message_content_valid() {
    assert!(validate_message_content("Hello").is_ok());
    assert!(validate_message_content(&"a".repeat(1000)).is_ok());
    assert!(validate_message_content(&"a".repeat(1_000_000)).is_ok());
}

#[test]
fn test_validate_message_content_too_long() {
    let long_content = "a".repeat(1_000_001);
    let result = validate_message_content(&long_content);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("too long"));
}

#[test]
fn test_validate_message_content_empty() {
    assert!(validate_message_content("").is_ok());
}

#[test]
fn test_validate_message_content_boundary() {
    let max_content = "a".repeat(1_000_000);
    assert!(validate_message_content(&max_content).is_ok());
}
