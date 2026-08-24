// SPDX-License-Identifier: MIT
// Copyright (c) 2025 aperture-router contributors

use aperture_router::config::{
    ApertureConfig, Config, CorsConfig, HttpConfig, RateLimitConfig, SecurityConfig,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::{IpAddr, Ipv4Addr};

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn test_config_default_values() {
        let config = Config::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8765);
        assert_eq!(config.aperture.base_url, "http://localhost:8080");
        assert_eq!(config.http.connect_timeout_secs, 10);
        assert_eq!(config.http.request_timeout_secs, 300);
        assert_eq!(config.rate_limit.requests_per_second, 10);
        assert_eq!(config.rate_limit.burst_size, 30);
        assert_eq!(config.security.max_body_size_bytes, 10 * 1024 * 1024);
    }

    #[test]
    fn test_config_server_addr() {
        let config = Config::default();
        let addr = config.server_addr().expect("Should be valid address");
        assert_eq!(addr.to_string(), "127.0.0.1:8765");
    }

    #[test]
    fn test_config_server_addr_invalid() {
        let config = Config {
            port: 0,
            ..Default::default()
        };
        let result = config.validate();
        assert!(result.is_err(), "Port 0 should be invalid in validation");
        assert!(result.unwrap_err().contains("Port cannot be 0"));
    }

    #[test]
    fn test_aperture_config_default() {
        let config = ApertureConfig::default();
        assert_eq!(config.base_url, "http://localhost:8080");
        assert!(config.api_key.is_none());
        assert_eq!(config.model_refresh_interval_secs, 300);
    }

    #[test]
    fn test_http_config_default() {
        let config = HttpConfig::default();
        assert_eq!(config.connect_timeout_secs, 10);
        assert_eq!(config.request_timeout_secs, 300);
    }

    #[test]
    fn test_cors_config_default() {
        let config = CorsConfig::default();
        assert_eq!(config.allowed_origins, vec!["http://localhost:3000"]);
        assert!(config.trusted_proxies.is_empty());
    }

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.requests_per_second, 10);
        assert_eq!(config.burst_size, 30);
        assert_eq!(config.health_requests_per_second, 20);
        assert_eq!(config.health_burst_size, 50);
    }

    #[test]
    fn test_security_config_default() {
        let config = SecurityConfig::default();
        assert!(config.api_keys.is_empty());
        assert_eq!(config.max_body_size_bytes, 10 * 1024 * 1024);
        assert_eq!(config.max_auth_attempts, 5);
        assert_eq!(config.auth_window_secs, 60);
        assert_eq!(config.ban_duration_secs, 300);
        assert!(config.require_auth_in_prod);
    }

    #[test]
    fn test_config_validation_valid_config() {
        let mut config = Config::default();
        config.security.api_keys = vec!["abcdefghijklmnopqrstuvwxyz123456".to_string()]; // Valid API key with sufficient entropy
        let result = config.validate();
        assert!(result.is_ok(), "Valid config should pass validation");
    }

    #[test]
    fn test_config_validation_port_zero() {
        let config = Config {
            port: 0,
            ..Default::default()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Port cannot be 0"));
    }

    #[test]
    fn test_config_validation_empty_base_url() {
        let mut config = Config::default();
        config.aperture.base_url = String::new();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Aperture base URL cannot be empty"));
    }

    #[test]
    fn test_config_validation_placeholder_api_key() {
        let mut config = Config::default();
        config.aperture.api_key = Some("your-api-key-here".to_string());
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("placeholder"));
    }

    #[test]
    fn test_config_validation_empty_api_key() {
        let mut config = Config::default();
        config.aperture.api_key = Some(String::new());
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_config_validation_api_key_too_short() {
        let mut config = Config::default();
        config.security.api_keys = vec!["short".to_string()];
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too short"));
    }

    #[test]
    fn test_config_validation_api_key_insufficient_entropy() {
        let mut config = Config::default();
        // Only 1 unique character repeated 32 times
        config.security.api_keys = vec!["a".repeat(32)];
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("insufficient entropy"));
    }

    #[test]
    fn test_config_validation_valid_api_key_entropy() {
        let mut config = Config::default();
        // 32 characters with sufficient entropy (at least 20 unique chars)
        config.security.api_keys = vec!["abcdefghijklmnopqrstuvwxyz123456".to_string()];
        let result = config.validate();
        assert!(
            result.is_ok(),
            "API key with sufficient entropy should be valid"
        );
    }

    #[test]
    fn test_config_validation_placeholder_security_key() {
        let mut config = Config::default();
        // The placeholder "your-api-key-here" itself should fail (too short)
        config.security.api_keys = vec!["your-api-key-here".to_string()];
        let result = config.validate();
        // Should fail because it's too short (only 19 chars)
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validation_rate_limit_zero_rps() {
        let mut config = Config::default();
        config.rate_limit.requests_per_second = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("requests per second cannot be 0"));
    }

    #[test]
    fn test_config_validation_rate_limit_zero_burst() {
        let mut config = Config::default();
        config.rate_limit.burst_size = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("burst size cannot be 0"));
    }

    #[test]
    fn test_config_validation_health_rate_limit_zero_rps() {
        let mut config = Config::default();
        config.security.api_keys = vec!["abcdefghijklmnopqrstuvwxyz123456".to_string()];
        config.rate_limit.health_requests_per_second = 0;
        // Deprecated knobs are warn-and-ignore: zero values no longer reject.
        assert!(
            config.validate().is_ok(),
            "deprecated health knobs must be accepted"
        );
    }

    #[test]
    fn test_config_validation_health_rate_limit_zero_burst() {
        let mut config = Config::default();
        config.security.api_keys = vec!["abcdefghijklmnopqrstuvwxyz123456".to_string()];
        config.rate_limit.health_burst_size = 0;
        assert!(
            config.validate().is_ok(),
            "deprecated health knobs must be accepted"
        );
    }

    #[test]
    fn test_config_validation_max_auth_attempts_zero() {
        let mut config = Config::default();
        config.security.max_auth_attempts = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Max authentication attempts cannot be 0"));
    }

    #[test]
    fn test_config_validation_auth_window_zero() {
        let mut config = Config::default();
        config.security.auth_window_secs = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Authentication window cannot be 0"));
    }

    #[test]
    fn test_config_validation_ban_duration_zero() {
        let mut config = Config::default();
        config.security.ban_duration_secs = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Ban duration cannot be 0"));
    }

    #[test]
    fn test_config_validation_max_body_size_zero() {
        let mut config = Config::default();
        config.security.max_body_size_bytes = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Max body size cannot be 0"));
    }

    #[test]
    fn test_config_validation_max_body_size_too_large() {
        let mut config = Config::default();
        config.security.max_body_size_bytes = 101 * 1024 * 1024; // 101MB
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot exceed 100MB"));
    }

    #[test]
    fn test_config_default_max_messages() {
        let config = Config::default();
        assert_eq!(config.security.max_messages, 10000);
    }

    #[test]
    fn test_config_validation_max_messages_zero() {
        let mut config = Config::default();
        config.security.max_messages = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Max messages cannot be 0"));
    }

    #[test]
    fn test_config_validation_max_messages_custom() {
        let mut config = Config::default();
        config.security.api_keys = vec!["abcdefghijklmnopqrstuvwxyz123456".to_string()];
        config.security.max_messages = 50000;
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_load_from_file() {
        let config_content = r#"
            host = "192.168.1.1"
            port = 9000

            [aperture]
            base_url = "http://test.example.com"
            api_key = "test-api-key-32-chars-long-1234567"

            [security]
            api_keys = ["key-with-at-least-32-chars-and-20-unique"]
            max_body_size_bytes = 5242880
        "#;

        // tempfile gives a unique path: parallel test runs cannot collide.
        let config_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
        let config_path = config_file.path().to_str().unwrap().to_string();
        fs::write(&config_path, config_content).expect("Failed to write test config file");
        let config = Config::load(&config_path).expect("Failed to load config");
        assert_eq!(config.host, "192.168.1.1");
        assert_eq!(config.port, 9000);
        assert_eq!(config.aperture.base_url, "http://test.example.com");
        assert_eq!(
            config.aperture.api_key,
            Some("test-api-key-32-chars-long-1234567".to_string())
        );
    }

    #[test]
    fn test_config_load_invalid_file() {
        let result = Config::load("/nonexistent/path/config.toml");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to read config file"));
    }

    #[test]
    fn test_config_load_invalid_toml() {
        let config_content = "invalid [toml syntax";
        let config_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
        let config_path = config_file.path().to_str().unwrap().to_string();
        fs::write(&config_path, config_content).expect("Failed to write test config file");

        let result = Config::load(&config_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse config file"));
    }

    #[test]
    fn test_model_aliases() {
        let mut aliases = HashMap::new();
        aliases.insert("gpt-4".to_string(), "gpt-4-turbo-preview".to_string());
        let config = Config {
            model_aliases: aliases,
            ..Default::default()
        };

        assert_eq!(
            config.model_aliases.get("gpt-4"),
            Some(&"gpt-4-turbo-preview".to_string())
        );
    }

    // ============================================
    // Model alias resolution tests (Task 1.3)
    // ============================================

    #[test]
    fn test_resolve_model_alias_found() {
        let mut aliases = HashMap::new();
        aliases.insert("gpt-4".to_string(), "gpt-4-turbo-preview".to_string());
        aliases.insert("claude-3".to_string(), "claude-3-opus-20240229".to_string());

        let config = Config {
            model_aliases: aliases,
            ..Default::default()
        };

        // Should resolve aliases
        assert_eq!(config.resolve_model_alias("gpt-4"), "gpt-4-turbo-preview");
        assert_eq!(
            config.resolve_model_alias("claude-3"),
            "claude-3-opus-20240229"
        );
    }

    #[test]
    fn test_resolve_model_alias_not_found() {
        let mut aliases = HashMap::new();
        aliases.insert("gpt-4".to_string(), "gpt-4-turbo-preview".to_string());

        let config = Config {
            model_aliases: aliases,
            ..Default::default()
        };

        // Should return original model name if no alias exists
        assert_eq!(config.resolve_model_alias("gpt-4-turbo"), "gpt-4-turbo");
        assert_eq!(config.resolve_model_alias("unknown-model"), "unknown-model");
    }

    #[test]
    fn test_resolve_model_alias_empty_config() {
        let config = Config::default();

        // Should return original model name when no aliases configured
        assert_eq!(config.resolve_model_alias("gpt-4"), "gpt-4");
        assert_eq!(config.resolve_model_alias("claude-3-opus"), "claude-3-opus");
    }

    #[test]
    fn test_resolve_model_alias_chain() {
        // Test that aliases don't chain (one level only)
        let mut aliases = HashMap::new();
        aliases.insert("gpt-4".to_string(), "gpt-4-turbo".to_string());
        // Note: "gpt-4-turbo" -> "gpt-4-turbo-preview" would be a second level
        // We should NOT resolve chains to avoid infinite loops
        aliases.insert("gpt-4-turbo".to_string(), "gpt-4-turbo-preview".to_string());

        let config = Config {
            model_aliases: aliases,
            ..Default::default()
        };

        // Should only resolve one level
        assert_eq!(config.resolve_model_alias("gpt-4"), "gpt-4-turbo");
        // But can resolve another alias directly
        assert_eq!(
            config.resolve_model_alias("gpt-4-turbo"),
            "gpt-4-turbo-preview"
        );
    }

    #[test]
    fn test_trusted_proxies() {
        let mut proxies = HashSet::new();
        proxies.insert(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        proxies.insert(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
        let cors_config = CorsConfig {
            trusted_proxies: proxies,
            ..Default::default()
        };

        assert_eq!(cors_config.trusted_proxies.len(), 2);
        assert!(cors_config
            .trusted_proxies
            .contains(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    }
}

#[cfg(test)]
mod config_save_tests {
    use aperture_router::config::Config;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_save_atomic_write() {
        let config = Config::default();
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let path = temp_file.path().to_str().unwrap().to_string();

        // Save should not leave .tmp file on success
        config.save(&path).expect("Failed to save config");

        assert!(!std::path::Path::new(&format!("{}.tmp", path)).exists());
        assert!(std::path::Path::new(&path).exists());

        // Verify content is valid TOML
        let content = std::fs::read_to_string(&path).expect("Failed to read saved config");
        let loaded: Config = toml::from_str(&content).expect("Saved config is not valid TOML");
        assert_eq!(loaded.host, config.host);
        assert_eq!(loaded.port, config.port);
    }

    #[test]
    fn test_config_save_overwrites_existing() {
        let mut config = Config {
            port: 8080,
            ..Default::default()
        };

        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let path = temp_file.path().to_str().unwrap().to_string();

        config.save(&path).expect("Failed to save config");

        // Modify and save again
        config.port = 9090;
        config.save(&path).expect("Failed to overwrite config");

        let content = std::fs::read_to_string(&path).expect("Failed to read saved config");
        let loaded: Config = toml::from_str(&content).expect("Saved config is not valid TOML");
        assert_eq!(loaded.port, 9090);
    }

    #[cfg(unix)]
    #[test]
    fn test_config_save_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let config = Config::default();
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let path = temp_file.path().to_str().unwrap().to_string();

        config.save(&path).expect("Failed to save config");

        let metadata = std::fs::metadata(&path).expect("Failed to get metadata");
        let mode = metadata.permissions().mode() & 0o777;

        // Config file should have 0o600 permissions (owner read/write only)
        assert_eq!(
            mode, 0o600,
            "Config file should have 0o600 permissions, got 0o{:o}",
            mode
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_config_save_creates_with_secure_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let config = Config::default();

        // Use a path that doesn't exist yet
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let path = temp_dir.path().join("new_config.toml");
        let path_str = path.to_str().unwrap().to_string();

        // File doesn't exist yet
        assert!(!path.exists());

        config.save(&path_str).expect("Failed to save config");

        // Now it should exist with secure permissions
        let metadata = std::fs::metadata(&path).expect("Failed to get metadata");
        let mode = metadata.permissions().mode() & 0o777;

        assert_eq!(
            mode, 0o600,
            "New config file should have 0o600 permissions, got 0o{:o}",
            mode
        );
    }

    #[test]
    fn test_config_save_preserves_secrets() {
        let mut config = Config::default();
        config.security.api_keys = vec!["secret-key-with-sufficient-entropy-12345".to_string()];
        config.security.admin_api_keys = vec!["admin-key-with-sufficient-entropy-123".to_string()];

        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let path = temp_file.path().to_str().unwrap().to_string();

        config.save(&path).expect("Failed to save config");

        let content = std::fs::read_to_string(&path).expect("Failed to read saved config");

        // API keys should be in the saved file (they're not marked as secret in serialization)
        assert!(content.contains("secret-key-with-sufficient-entropy-12345"));
        assert!(content.contains("admin-key-with-sufficient-entropy-123"));
    }
}
