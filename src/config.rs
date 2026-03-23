// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};

/// Aperture gateway configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApertureConfig {
    /// Base URL for Aperture gateway
    #[serde(default = "default_aperture_url")]
    pub base_url: String,

    /// Optional API key for Aperture authentication
    #[serde(default)]
    pub api_key: Option<String>,

    /// Model refresh interval in seconds
    #[serde(default = "default_model_refresh_interval")]
    pub model_refresh_interval_secs: u64,
}

fn default_aperture_url() -> String {
    "http://localhost:8080".to_string()
}

fn default_model_refresh_interval() -> u64 {
    300
}

/// HTTP client configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpConfig {
    /// Connection timeout in seconds
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,

    /// Request timeout in seconds
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,

    /// SSE keep-alive interval in seconds
    #[serde(default = "default_sse_keep_alive")]
    pub sse_keep_alive_secs: u64,
}

fn default_connect_timeout() -> u64 {
    10
}

fn default_request_timeout() -> u64 {
    300
}

fn default_sse_keep_alive() -> u64 {
    15
}

/// CORS configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorsConfig {
    /// Allowed origins (use "*" for wildcard)
    #[serde(default = "default_allowed_origins")]
    pub allowed_origins: Vec<String>,

    /// Trusted proxy IPs (for x-forwarded-for header validation)
    #[serde(default)]
    pub trusted_proxies: HashSet<IpAddr>,
}

fn default_allowed_origins() -> Vec<String> {
    vec!["http://localhost:3000".to_string()]
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: default_allowed_origins(),
            trusted_proxies: HashSet::new(),
        }
    }
}

/// Rate limiting configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RateLimitConfig {
    /// Requests per second
    #[serde(default = "default_requests_per_second")]
    pub requests_per_second: u64,

    /// Burst size
    #[serde(default = "default_burst_size")]
    pub burst_size: u64,

    /// Health endpoint requests per second (separate limit)
    #[serde(default = "default_health_rate_limit")]
    pub health_requests_per_second: u64,

    /// Health endpoint burst size
    #[serde(default = "default_health_burst_size")]
    pub health_burst_size: u64,
}

fn default_requests_per_second() -> u64 {
    10
}

fn default_burst_size() -> u64 {
    30
}

fn default_health_rate_limit() -> u64 {
    20
}

fn default_health_burst_size() -> u64 {
    50
}

/// Security configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    /// API keys for authentication (empty means no auth)
    #[serde(default)]
    pub api_keys: Vec<String>,

    /// Admin API keys for administrative operations (empty means no separate admin keys)
    /// If empty, regular api_keys are used for admin operations
    #[serde(default)]
    pub admin_api_keys: Vec<String>,

    /// Maximum request body size in bytes
    #[serde(default = "default_max_body_size")]
    pub max_body_size_bytes: usize,

    /// Maximum authentication attempts per window
    #[serde(default = "default_max_auth_attempts")]
    pub max_auth_attempts: usize,

    /// Authentication window duration in seconds
    #[serde(default = "default_auth_window")]
    pub auth_window_secs: u64,

    /// Ban duration in seconds after max attempts
    #[serde(default = "default_ban_duration")]
    pub ban_duration_secs: u64,

    /// Require authentication in production
    #[serde(default = "default_require_auth")]
    pub require_auth_in_prod: bool,

    /// Maximum JSON nesting depth to prevent DoS
    #[serde(default = "default_max_json_depth")]
    pub max_json_depth: usize,

    /// Maximum streaming response size in bytes
    #[serde(default = "default_max_streaming_size")]
    pub max_streaming_size_bytes: usize,
}

fn default_max_body_size() -> usize {
    10 * 1024 * 1024 // 10MB
}

fn default_max_auth_attempts() -> usize {
    5
}

fn default_auth_window() -> u64 {
    60 // 1 minute
}

fn default_ban_duration() -> u64 {
    300 // 5 minutes
}

fn default_require_auth() -> bool {
    true // Default to requiring auth
}

fn default_max_json_depth() -> usize {
    256 // Maximum JSON nesting depth
}

fn default_max_streaming_size() -> usize {
    100 * 1024 * 1024 // 100MB
}

/// Main configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Server host
    #[serde(default = "default_host")]
    pub host: String,

    /// Server port
    #[serde(default = "default_port")]
    pub port: u16,

    /// Aperture configuration
    #[serde(default)]
    pub aperture: ApertureConfig,

    /// HTTP configuration
    #[serde(default)]
    pub http: HttpConfig,

    /// CORS configuration
    #[serde(default)]
    pub cors: CorsConfig,

    /// Rate limiting configuration
    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    /// Security configuration
    #[serde(default)]
    pub security: SecurityConfig,

    /// Model aliases
    #[serde(default)]
    pub model_aliases: HashMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            aperture: ApertureConfig::default(),
            http: HttpConfig::default(),
            cors: CorsConfig::default(),
            rate_limit: RateLimitConfig::default(),
            security: SecurityConfig::default(),
            model_aliases: HashMap::new(),
        }
    }
}

impl Default for ApertureConfig {
    fn default() -> Self {
        Self {
            base_url: default_aperture_url(),
            api_key: None,
            model_refresh_interval_secs: default_model_refresh_interval(),
        }
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            connect_timeout_secs: default_connect_timeout(),
            request_timeout_secs: default_request_timeout(),
            sse_keep_alive_secs: default_sse_keep_alive(),
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: default_requests_per_second(),
            burst_size: default_burst_size(),
            health_requests_per_second: default_health_rate_limit(),
            health_burst_size: default_health_burst_size(),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            api_keys: Vec::new(),
            admin_api_keys: Vec::new(),
            max_body_size_bytes: default_max_body_size(),
            max_auth_attempts: default_max_auth_attempts(),
            auth_window_secs: default_auth_window(),
            ban_duration_secs: default_ban_duration(),
            require_auth_in_prod: default_require_auth(),
            max_json_depth: default_max_json_depth(),
            max_streaming_size_bytes: default_max_streaming_size(),
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8765
}

/// Configuration loading and validation
impl Config {
    /// Load configuration from file
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let config_content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read config file {}: {}", path, e))?;

        let mut config: Self = toml::from_str(&config_content)
            .map_err(|e| anyhow::anyhow!("Failed to parse config file {}: {}", path, e))?;

        // Override with environment variables
        if let Ok(host) = std::env::var("APERTURE_HOST") {
            tracing::warn!("Overriding host with environment variable: {}", host);
            config.host = host;
        }

        if let Ok(port) = std::env::var("APERTURE_PORT") {
            tracing::warn!("Overriding port with environment variable: {}", port);
            config.port = port
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid APERTURE_PORT: {}", e))?;
        }

        if let Ok(base_url) = std::env::var("APERTURE_BASE_URL") {
            tracing::warn!("Overriding base_url with environment variable");
            config.aperture.base_url = base_url;
        }

        if let Ok(api_key) = std::env::var("APERTURE_API_KEY") {
            tracing::warn!("Overriding api_key with environment variable");
            config.aperture.api_key = Some(api_key);
        }

        config
            .validate()
            .map_err(|e| anyhow::anyhow!("Config validation failed: {}", e))?;

        Ok(config)
    }

    /// Get server address
    pub fn server_addr(&self) -> anyhow::Result<SocketAddr> {
        let addr = format!("{}:{}", self.host, self.port);
        addr.parse()
            .map_err(|e| anyhow::anyhow!("Invalid server address {}: {}", addr, e))
    }

    /// Configuration validation
    pub fn validate(&self) -> Result<(), String> {
        if self.port == 0 {
            return Err("Port cannot be 0".to_string());
        }

        if self.aperture.base_url.is_empty() {
            return Err("Aperture base URL cannot be empty".to_string());
        }

        // Validate API key configuration
        if let Some(ref api_key) = self.aperture.api_key {
            if api_key.contains("your-api-key-here") || api_key.is_empty() {
                return Err("API key contains placeholder value or is empty".to_string());
            }

            // Require HTTPS if API key is configured
            if !self.aperture.base_url.starts_with("https://") {
                tracing::warn!("API key configured but Aperture gateway uses HTTP. Keys will be transmitted in clear text.");
            }
        }

        // Validate API keys strength
        for key in &self.security.api_keys {
            if key.len() < 32 {
                return Err(format!(
                    "API key too short: {} characters (minimum 32)",
                    key.len()
                ));
            }

            let unique_chars = key.chars().collect::<std::collections::HashSet<_>>();
            if unique_chars.len() < 20 {
                return Err(
                    "API key has insufficient entropy (minimum 20 unique characters)".to_string(),
                );
            }
        }

        // Validate security configuration
        if self
            .security
            .api_keys
            .contains(&"your-api-key-here".to_string())
        {
            return Err("Security API keys contain placeholder value".to_string());
        }

        // Validate admin API keys strength (CRITICAL: same requirements as regular keys)
        for key in &self.security.admin_api_keys {
            if key.len() < 32 {
                return Err(format!(
                    "Admin API key too short: {} characters (minimum 32)",
                    key.len()
                ));
            }

            let unique_chars = key.chars().collect::<std::collections::HashSet<_>>();
            if unique_chars.len() < 20 {
                return Err(
                    "Admin API key has insufficient entropy (minimum 20 unique characters)"
                        .to_string(),
                );
            }
        }

        // Check for placeholder values in admin API keys
        if self
            .security
            .admin_api_keys
            .contains(&"your-admin-api-key-here".to_string())
            || self
                .security
                .admin_api_keys
                .contains(&"your-api-key-here".to_string())
        {
            return Err("Admin API keys contain placeholder value".to_string());
        }

        // Validate rate limits
        if self.rate_limit.requests_per_second == 0 {
            return Err("Rate limit requests per second cannot be 0".to_string());
        }

        if self.rate_limit.burst_size == 0 {
            return Err("Rate limit burst size cannot be 0".to_string());
        }

        if self.rate_limit.health_requests_per_second == 0 {
            return Err("Health rate limit requests per second cannot be 0".to_string());
        }

        if self.rate_limit.health_burst_size == 0 {
            return Err("Health rate limit burst size cannot be 0".to_string());
        }

        // Validate authentication limits
        if self.security.max_auth_attempts == 0 {
            return Err("Max authentication attempts cannot be 0".to_string());
        }

        if self.security.auth_window_secs == 0 {
            return Err("Authentication window cannot be 0 seconds".to_string());
        }

        if self.security.ban_duration_secs == 0 {
            return Err("Ban duration cannot be 0 seconds".to_string());
        }

        // Validate body size limit
        if self.security.max_body_size_bytes == 0 {
            return Err("Max body size cannot be 0".to_string());
        }

        if self.security.max_body_size_bytes > 100 * 1024 * 1024 {
            return Err("Max body size cannot exceed 100MB".to_string());
        }

        // Validate JSON depth limit
        if self.security.max_json_depth < 16 {
            return Err("Max JSON depth must be at least 16".to_string());
        }
        if self.security.max_json_depth > 4096 {
            return Err("Max JSON depth cannot exceed 4096".to_string());
        }

        // Validate streaming size limit
        if self.security.max_streaming_size_bytes == 0 {
            return Err("Max streaming size cannot be 0".to_string());
        }
        if self.security.max_streaming_size_bytes > 1024 * 1024 * 1024 {
            return Err("Max streaming size cannot exceed 1GB".to_string());
        }

        // Production safety check
        if self.security.require_auth_in_prod
            && self.security.api_keys.is_empty()
            && cfg!(debug_assertions)
        {
            return Err("Production mode requires authentication but no API keys configured. Set APERTURE_ALLOW_NO_AUTH=1 to override (not recommended)".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_functions() {
        assert_eq!(default_host(), "127.0.0.1");
        assert_eq!(default_port(), 8765);
        assert_eq!(default_aperture_url(), "http://localhost:8080");
        assert_eq!(default_model_refresh_interval(), 300);
        assert_eq!(default_connect_timeout(), 10);
        assert_eq!(default_request_timeout(), 300);
        assert_eq!(default_allowed_origins(), vec!["http://localhost:3000"]);
        assert_eq!(default_requests_per_second(), 10);
        assert_eq!(default_burst_size(), 30);
        assert_eq!(default_health_rate_limit(), 20);
        assert_eq!(default_health_burst_size(), 50);
        assert_eq!(default_max_body_size(), 10 * 1024 * 1024);
        assert_eq!(default_max_auth_attempts(), 5);
        assert_eq!(default_auth_window(), 60);
        assert_eq!(default_ban_duration(), 300);
        assert!(default_require_auth());
    }

    #[test]
    fn test_config_serialization_deserialization() {
        let config = Config::default();

        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&serialized).unwrap();

        assert_eq!(config.host, deserialized.host);
        assert_eq!(config.port, deserialized.port);
        assert_eq!(config.aperture.base_url, deserialized.aperture.base_url);
    }

    #[test]
    fn test_config_with_custom_values() {
        let config = Config {
            host: "192.168.1.100".to_string(),
            port: 9000,
            aperture: ApertureConfig {
                base_url: "https://custom.example.com".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(config.host, "192.168.1.100");
        assert_eq!(config.port, 9000);
        assert_eq!(config.aperture.base_url, "https://custom.example.com");
    }

    #[test]
    fn test_config_toml_roundtrip() {
        let config = Config::default();

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(config.host, deserialized.host);
        assert_eq!(config.port, deserialized.port);
    }
}
