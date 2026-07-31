// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! Shared SSRF protection utilities
//!
//! Single source of truth for metadata endpoint blocklists and internal IP checks.
//! All modules should use these functions instead of duplicating the logic.

use std::net::IpAddr;

/// Check if a host is a cloud metadata endpoint (exact match to prevent bypass via subdomains)
///
/// Blocks:
/// - AWS/GCP: 169.254.169.254 (and IPv4-mapped IPv6 variant)
/// - Alibaba Cloud: 100.100.100.200
/// - GCP hostname: metadata.google.internal
/// - Azure hostname: metadata.azure.com
pub fn is_metadata_endpoint(host: &str) -> bool {
    host == "169.254.169.254"
        || host == "[::ffff:169.254.169.254]"
        || host == "100.100.100.200"
        || host == "metadata.google.internal"
        || host == "metadata.azure.com"
}

/// Check if a host is a blocked host (metadata endpoints + Kubernetes internal DNS)
///
/// Extends `is_metadata_endpoint` with hostname-based patterns for:
/// - Kubernetes service DNS: `metadata.*.internal`, `*.metadata.internal`
/// - Internal service mesh patterns
/// - Trailing dot bypass prevention (RFC 1034 DNS equivalence)
pub fn is_blocked_host(host: &str) -> bool {
    // Normalize: strip trailing dot (DNS equivalent per RFC 1034)
    // This prevents bypass via "metadata.internal." (trailing dot)
    let normalized = host.strip_suffix('.').unwrap_or(host);

    // First check known metadata endpoints (exact match)
    if is_metadata_endpoint(normalized) {
        return true;
    }

    // Block any .internal domain containing "metadata"
    // Catches metadata.kubernetes.internal, kubernetes-metadata.internal, etc.
    if normalized.ends_with(".internal") && normalized.contains("metadata") {
        return true;
    }

    // Block Kubernetes service DNS for metadata-like services
    if normalized.starts_with("metadata.") && normalized.contains(".svc.") {
        return true;
    }

    false
}

/// Core internal IP check shared across all validation contexts
/// Returns true if the IP is private, loopback, link-local, or (if block_cgn) CGN range
pub fn is_internal_ip_impl(ip: &IpAddr, block_cgn: bool) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let mut blocked = v4.is_private() || v4.is_loopback() || v4.is_link_local();
            if block_cgn {
                // Block shared/carrier-grade NAT (100.64.0.0/10) - used by some networks
                blocked |= v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]);
            }
            blocked
        }
        IpAddr::V6(v6) => {
            // Check for IPv4-mapped IPv6 addresses (::ffff:x.x.x.x)
            // These can encode internal IPv4 addresses and bypass checks
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_internal_ip_impl(&IpAddr::V4(v4), block_cgn);
            }

            // Block loopback (::1)
            v6.is_loopback()
            // Block unique local addresses (fc00::/7)
            || v6.is_unique_local()
            // Block link-local (fe80::/10)
            || v6.is_unicast_link_local()
            // Block multicast (ff00::/8)
            || v6.is_multicast()
        }
    }
}

/// Check if a host string is an internal IP (blocks CGN range)
/// Used for default gateway endpoint validation where CGN should be blocked
pub fn is_internal_ip(host: &str) -> bool {
    host.parse::<IpAddr>()
        .map(|ip| is_internal_ip_impl(&ip, true))
        .unwrap_or(false)
}

/// Strict internal IP check for provider URL validation (SSRF defense-in-depth)
/// Unlike is_internal_ip(), this does NOT block CGN (100.64.0.0/10) because
/// Tailscale deployments legitimately use this range
pub fn is_internal_ip_strict(ip: &IpAddr) -> bool {
    is_internal_ip_impl(ip, false)
}

/// String-based strict internal IP check (parses host and checks, allows CGN)
/// Used for provider URL validation where Tailscale CGN range is legitimate
pub fn is_internal_ip_strict_host(host: &str) -> bool {
    host.parse::<IpAddr>()
        .map(|ip| is_internal_ip_impl(&ip, false))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_endpoint_exact_match() {
        assert!(is_metadata_endpoint("169.254.169.254"));
        assert!(is_metadata_endpoint("metadata.google.internal"));
        assert!(is_metadata_endpoint("metadata.azure.com"));
        assert!(is_metadata_endpoint("[::ffff:169.254.169.254]"));
        assert!(is_metadata_endpoint("100.100.100.200"));
    }

    #[test]
    fn test_metadata_endpoint_rejects_subdomains() {
        assert!(!is_metadata_endpoint("not-169.254.169.254.example.com"));
        assert!(!is_metadata_endpoint("fake-metadata.google.internal"));
        assert!(!is_metadata_endpoint("xmetadata.azure.com"));
    }

    #[test]
    fn test_blocked_host_k8s_patterns() {
        assert!(is_blocked_host("metadata.default.svc.cluster.local"));
        assert!(is_blocked_host("metadata.something.internal"));
        assert!(is_blocked_host("metadata.internal"));
        assert!(is_blocked_host("kubernetes-metadata.internal"));
        assert!(is_blocked_host("some-metadata.internal"));
    }

    #[test]
    fn test_blocked_host_allows_legitimate() {
        assert!(!is_blocked_host("api.example.com"));
        assert!(!is_blocked_host("my-service.internal")); // no "metadata" keyword
        assert!(!is_blocked_host("database.internal"));
    }

    #[test]
    fn test_blocked_host_trailing_dot() {
        assert!(is_blocked_host("metadata.internal."));
        assert!(is_blocked_host("kubernetes-metadata.internal."));
        assert!(is_blocked_host("metadata.google.internal."));
    }

    #[test]
    fn test_internal_ip_blocks_private() {
        assert!(is_internal_ip("10.0.0.1"));
        assert!(is_internal_ip("172.16.0.1"));
        assert!(is_internal_ip("192.168.1.1"));
        assert!(is_internal_ip("127.0.0.1"));
    }

    #[test]
    fn test_internal_ip_allows_public() {
        assert!(!is_internal_ip("8.8.8.8"));
        assert!(!is_internal_ip("1.1.1.1"));
    }

    #[test]
    fn test_internal_ip_blocks_cgn() {
        assert!(is_internal_ip("100.64.0.1"));
        assert!(is_internal_ip("100.127.255.255"));
    }

    #[test]
    fn test_internal_ip_strict_allows_cgn() {
        let cgn: IpAddr = "100.64.0.1".parse().unwrap();
        assert!(!is_internal_ip_strict(&cgn));
    }

    #[test]
    fn test_internal_ip_strict_blocks_private() {
        let private: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(is_internal_ip_strict(&private));
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(is_internal_ip_strict(&loopback));
    }

    #[test]
    fn test_internal_ip_ipv6() {
        assert!(is_internal_ip("::1"));
        assert!(is_internal_ip("::ffff:10.0.0.1"));
        assert!(is_internal_ip("::ffff:127.0.0.1"));
    }
}
