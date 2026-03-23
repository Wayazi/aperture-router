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
        config.rate_limit.health_requests_per_second = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Health rate limit requests per second cannot be 0"));
    }

    #[test]
    fn test_config_validation_health_rate_limit_zero_burst() {
        let mut config = Config::default();
        config.rate_limit.health_burst_size = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Health rate limit burst size cannot be 0"));
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

        let config_path = "/tmp/test_config.toml";
        fs::write(config_path, config_content).expect("Failed to write test config file");

        let config = Config::load(config_path).expect("Failed to load config");
        assert_eq!(config.host, "192.168.1.1");
        assert_eq!(config.port, 9000);
        assert_eq!(config.aperture.base_url, "http://test.example.com");
        assert_eq!(
            config.aperture.api_key,
            Some("test-api-key-32-chars-long-1234567".to_string())
        );

        // Clean up
        fs::remove_file(config_path).ok();
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
        let config_path = "/tmp/test_invalid_config.toml";
        fs::write(config_path, config_content).expect("Failed to write test config file");

        let result = Config::load(config_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse config file"));

        fs::remove_file(config_path).ok();
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
