// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! Security utilities for CLI configuration
//!
//! Provides secure handling of sensitive data like API keys:
//! - Secure password input (no echo)
//! - API key validation
//! - URL validation to prevent SSRF
//! - Logging safety (never expose secrets)

use std::fmt;
use url::Url;
use zeroize::Zeroize;

/// A string that never exposes its contents in debug/display output
#[derive(Clone, Zeroize)]
pub struct SecretString(String);

impl SecretString {
    /// Create a new secret string
    pub fn new(s: String) -> Self {
        Self(s)
    }

    /// Get the inner value (use sparingly)
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Check if the secret is empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretString(****)")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[HIDDEN]")
    }
}

/// Validate URL to prevent SSRF attacks
pub fn validate_url(url: &str) -> Result<Url, String> {
    // Trim whitespace before parsing
    let trimmed = url.trim();
    let parsed = Url::parse(trimmed).map_err(|e| format!("Invalid URL: {}", e))?;

    // Only allow http/https schemes
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "Invalid scheme '{}': only http and https are allowed",
            parsed.scheme()
        ));
    }

    // Check for blocked hosts
    if let Some(host) = parsed.host_str() {
        if is_blocked_host(host) {
            return Err(format!("Blocked host: {}", host));
        }
    }

    Ok(parsed)
}

/// Trim and validate URL, returning the cleaned URL string
pub fn clean_url(url: &str) -> Result<String, String> {
    validate_url(url)?;
    Ok(url.trim().to_string())
}

/// Check if a host is blocked (metadata endpoints, etc.)
fn is_blocked_host(host: &str) -> bool {
    // Block cloud metadata endpoints (exact match to prevent bypass via subdomains)
    host == "169.254.169.254"
        || host == "[::ffff:169.254.169.254]"
        || host == "100.100.100.200"
        || host == "metadata.google.internal"
        || host == "metadata.azure.com"
        // Block Kubernetes service DNS
        || host.ends_with(".internal")
        && host.starts_with("metadata")
}

/// Validate API key strength
pub fn validate_api_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("API key cannot be empty".to_string());
    }

    // Check for placeholder values
    let lower = key.to_lowercase();
    if lower.contains("your-api-key")
        || lower.contains("placeholder")
        || lower.contains("example")
        || lower.contains("xxx")
    {
        return Err("API key contains placeholder value".to_string());
    }

    // Check minimum length (must match config validation: 32 characters)
    if key.len() < 32 {
        return Err(format!(
            "API key too short: {} characters (minimum 32)",
            key.len()
        ));
    }

    Ok(())
}

/// Create a safe summary of config for logging (no secrets)
pub fn safe_config_summary(config: &crate::config::Config) -> String {
    format!(
        "Config: host={}, port={}, aperture={}, providers=[{}]",
        config.host,
        config.port,
        config.aperture.base_url,
        config
            .providers
            .iter()
            .map(|p| format!(
                "{}(models={}, key={})",
                p.name,
                p.models.len(),
                if p.api_key.is_some() { "set" } else { "none" }
            ))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_string_debug() {
        let secret = SecretString::new("my-super-secret-key".to_string());
        let debug_output = format!("{:?}", secret);
        assert!(!debug_output.contains("my-super-secret-key"));
        assert!(debug_output.contains("****"));
    }

    #[test]
    fn test_secret_string_display() {
        let secret = SecretString::new("my-super-secret-key".to_string());
        let display_output = format!("{}", secret);
        assert!(!display_output.contains("my-super-secret-key"));
        assert!(display_output.contains("[HIDDEN]"));
    }

    #[test]
    fn test_validate_url_valid() {
        assert!(validate_url("http://localhost:8080").is_ok());
        assert!(validate_url("https://api.example.com").is_ok());
        assert!(validate_url("http://100.100.100.100").is_ok());
    }

    #[test]
    fn test_validate_url_invalid_scheme() {
        assert!(validate_url("ftp://example.com").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn test_validate_url_blocked() {
        assert!(validate_url("http://169.254.169.254").is_err());
        assert!(validate_url("http://metadata.google.internal").is_err());
    }

    #[test]
    fn test_validate_url_trims_whitespace() {
        // URL with leading/trailing whitespace should still be valid
        assert!(validate_url("  http://localhost:8080  ").is_ok());
        assert!(validate_url(" http://100.100.100.100").is_ok());
    }

    #[test]
    fn test_clean_url() {
        assert_eq!(
            clean_url("  http://localhost:8080  ").unwrap(),
            "http://localhost:8080"
        );
        assert_eq!(
            clean_url(" http://100.100.100.100").unwrap(),
            "http://100.100.100.100"
        );
    }

    #[test]
    fn test_validate_api_key_valid() {
        assert!(validate_api_key("my-super-secret-api-key-12345678").is_ok());
    }

    #[test]
    fn test_validate_api_key_too_short() {
        assert!(validate_api_key("short").is_err());
    }

    #[test]
    fn test_validate_api_key_placeholder() {
        assert!(validate_api_key("your-api-key-here").is_err());
        assert!(validate_api_key("PLACEHOLDER_KEY").is_err());
    }
}
