// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! Constants and helpers shared across the route handlers.

use crate::config::Provider;

/// Maximum upstream providers tried per request before failing over.
pub const MAX_FAILOVER_ATTEMPTS: usize = 3;

/// Maximum extra JSON fields on a request (prevent memory exhaustion).
pub const MAX_OTHER_FIELDS: usize = 50;

/// Resolve the API key for an upstream provider call: the provider's own key
/// if configured, otherwise the default gateway key when the provider points
/// at the same base URL, otherwise none.
///
/// The returned reference borrows from `provider` or `default_key`; callers
/// that need ownership should `.map(str::to_string)` the result.
pub fn provider_api_key<'a>(
    provider: &'a Provider,
    default_key: Option<&'a String>,
    gateway_url: &str,
) -> Option<&'a str> {
    if provider.api_key.is_some() {
        return provider.api_key.as_deref();
    }
    if provider.base_url.trim_end_matches('/') == gateway_url.trim_end_matches('/') {
        default_key.map(|s| s.as_str())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_with_key(key: Option<&str>, base_url: &str) -> Provider {
        Provider {
            name: "test".to_string(),
            base_url: base_url.to_string(),
            api_key: key.map(|s| s.to_string()),
            endpoint_style: crate::config::EndpointStyle::OpenaiV1,
            models: vec![],
            enabled: true,
        }
    }

    #[test]
    fn test_provider_key_preferred_over_default() {
        let p = provider_with_key(Some("provider-key-0123456789abcdef"), "http://a.example");
        let default = "default-key-0123456789abcdef".to_string();
        assert_eq!(
            provider_api_key(&p, Some(&default), "http://gateway.example"),
            Some("provider-key-0123456789abcdef")
        );
    }

    #[test]
    fn test_default_key_only_for_same_gateway_url() {
        let p = provider_with_key(None, "http://gateway.example");
        let default = "default-key-0123456789abcdef".to_string();
        assert_eq!(
            provider_api_key(&p, Some(&default), "http://gateway.example/"),
            Some("default-key-0123456789abcdef"),
            "trailing slash must not break the match"
        );
        assert_eq!(
            provider_api_key(&p, Some(&default), "http://other.example"),
            None
        );
    }

    #[test]
    fn test_no_keys_at_all() {
        let p = provider_with_key(None, "http://gateway.example");
        assert_eq!(provider_api_key(&p, None, "http://gateway.example"), None);
    }
}
