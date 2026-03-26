// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! Shared HTTP client for memory efficiency
//!
//! Provides a single shared reqwest::Client instance with optimized settings:
//! - Connection pool limits to prevent unbounded memory usage
//! - Single TLS backend (rustls) for smaller binary
//! - Configurable timeouts

use once_cell::sync::Lazy;
use reqwest::Client;
use std::time::Duration;

/// Default request timeout in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Default connect timeout in seconds
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Maximum idle connections per host (reduced from default 100)
const MAX_IDLE_PER_HOST: usize = 5;

/// Idle connection timeout in seconds
const IDLE_TIMEOUT_SECS: u64 = 60;

/// Global shared HTTP client with optimized settings
///
/// Using a shared client:
/// - Reduces memory by sharing a single connection pool
/// - Reuses connections across all requests
/// - Prevents connection pool bloat
pub static SHARED_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS))
        .pool_max_idle_per_host(MAX_IDLE_PER_HOST)
        .pool_idle_timeout(Duration::from_secs(IDLE_TIMEOUT_SECS))
        // CRITICAL: Disable redirects to prevent SSRF bypass
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("Failed to create shared HTTP client")
});

/// Create a customized HTTP client with specific timeouts
///
/// Use this when you need different timeout settings than the shared client
pub fn create_client_with_timeouts(
    timeout_secs: u64,
    connect_timeout_secs: u64,
) -> anyhow::Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(connect_timeout_secs))
        .pool_max_idle_per_host(MAX_IDLE_PER_HOST)
        .pool_idle_timeout(Duration::from_secs(IDLE_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

/// Static allowed endpoints (no heap allocation)
pub static ALLOWED_ENDPOINTS: &[&str] = &[
    "v1/chat/completions",
    "v1/messages",
    "v1/models",
    "v1/embeddings",
];

/// Check if an endpoint is allowed
pub fn is_allowed_endpoint(endpoint: &str) -> bool {
    ALLOWED_ENDPOINTS.contains(&endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_client_is_created() {
        // Just verify it doesn't panic
        let _ = &*SHARED_CLIENT;
    }

    #[test]
    fn test_allowed_endpoints() {
        assert!(is_allowed_endpoint("v1/chat/completions"));
        assert!(is_allowed_endpoint("v1/models"));
        assert!(!is_allowed_endpoint("v1/unknown"));
    }

    #[test]
    fn test_create_custom_client() {
        let client = create_client_with_timeouts(60, 15);
        assert!(client.is_ok());
    }
}
