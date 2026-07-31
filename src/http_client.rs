// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! HTTP client builder for memory efficiency
//!
//! Provides utilities to create reqwest::Client instances with optimized settings:
//! - Connection pool limits to prevent unbounded memory usage
//! - Single TLS backend (rustls) for smaller binary
//! - Configurable timeouts

use reqwest::Client;
use std::time::Duration;

/// Maximum idle connections per host (reduced from default 100)
const MAX_IDLE_PER_HOST: usize = 5;

/// Idle connection timeout in seconds
const IDLE_TIMEOUT_SECS: u64 = 60;

/// Create a customized HTTP client with specific timeouts
///
/// Each caller (ProxyClient, ModelDiscovery) gets its own client instance
/// with connection pooling bounded by MAX_IDLE_PER_HOST.
pub fn create_client_with_timeouts(
    timeout_secs: u64,
    connect_timeout_secs: u64,
) -> anyhow::Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(connect_timeout_secs))
        .pool_max_idle_per_host(MAX_IDLE_PER_HOST)
        .pool_idle_timeout(Duration::from_secs(IDLE_TIMEOUT_SECS))
        // CRITICAL: Disable redirects to prevent SSRF bypass
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
