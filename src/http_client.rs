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

/// Cap for non-streaming upstream response bodies (10 MB, matches docs).
pub const MAX_NON_STREAMING_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Cap for upstream error bodies kept for logging/conversion (64 KiB).
pub const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// Read a response body in bounded chunks, aborting past `max_bytes`.
///
/// Chunked or close-delimited responses have no Content-Length, so a post-hoc
/// size check after `.text()`/`.bytes()` still buffers the entire body first.
/// Reading chunk-by-chunk caps memory and drops the connection mid-body on
/// overflow instead of re-reading the whole thing.
pub async fn read_body_capped(
    mut response: reqwest::Response,
    max_bytes: usize,
    context: &str,
) -> anyhow::Result<Vec<u8>> {
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| anyhow::anyhow!("{context}: failed reading body: {e}"))?
    {
        if body.len() + chunk.len() > max_bytes {
            anyhow::bail!("{context}: response exceeds {max_bytes} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
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
