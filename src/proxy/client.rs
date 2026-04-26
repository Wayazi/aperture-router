// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use futures::{stream, Stream, StreamExt};
use reqwest::Client;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net;
use tracing::{debug, error, info, warn};
use url::Url;

use crate::config::{ApertureConfig, HttpConfig};
use crate::http_client::{create_client_with_timeouts, is_allowed_endpoint};

/// HTTP client for proxying requests to Aperture
#[derive(Clone)]
pub struct ProxyClient {
    client: Client,
    aperture_config: ApertureConfig,
    max_streaming_size_bytes: usize,
}

impl ProxyClient {
    pub fn new(
        aperture_config: ApertureConfig,
        http_config: HttpConfig,
        max_streaming_size_bytes: usize,
    ) -> anyhow::Result<Self> {
        // Enforce HTTPS for non-Tailscale deployments ONLY when API key is configured
        // This prevents API key exposure over HTTP while allowing HTTP for:
        // - Tailscale deployments (encrypted at network layer)
        // - Localhost development
        // - Deployments without API keys (using other auth methods)
        let has_api_key = aperture_config.api_key.is_some();
        let is_tailscale = aperture_config.base_url.contains("100.100.")
            || aperture_config.base_url.contains(".tsnet.");

        // Allow HTTP for localhost (development/testing) - any port
        // Use proper URL parsing to detect all localhost forms (IPv6 [::1], 127.x, etc.)
        let host_str = Url::parse(&aperture_config.base_url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()));

        let is_localhost = host_str
            .as_deref()
            .map(|host| {
                // Check literal "localhost" hostname
                host == "localhost"
                // Check loopback IPs (127.x.x.x, ::1, etc.)
                || host.parse::<IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
            })
            .unwrap_or(false);

        // Only enforce HTTPS if: API key is set AND not Tailscale AND not localhost
        if has_api_key
            && !is_tailscale
            && !is_localhost
            && !aperture_config.base_url.starts_with("https://")
        {
            return Err(anyhow::anyhow!(
                "HTTPS required for non-Tailscale Aperture gateway when API key is configured. \
                 Either use HTTPS, or use Tailscale/localhost (network-layer encryption), \
                 or remove API key to use other authentication methods."
            ));
        }

        let client = create_client_with_timeouts(
            http_config.request_timeout_secs,
            http_config.connect_timeout_secs,
        )
        .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

        Ok(Self {
            client,
            aperture_config,
            max_streaming_size_bytes,
        })
    }

    /// Forward a request to Aperture
    pub async fn forward_request(
        &self,
        endpoint: &str,
        body: Vec<u8>,
    ) -> anyhow::Result<reqwest::Response> {
        let parsed_url = self.validate_endpoint(endpoint)?;

        debug!("Forwarding request to {}", parsed_url);
        info!("Proxying to: {}", endpoint);

        let mut request = self
            .client
            .post(parsed_url)
            .header("Content-Type", "application/json");

        // Add API key if configured
        if let Some(ref api_key) = self.aperture_config.api_key {
            request = request.header("x-api-key", api_key);
        }

        let response = request.body(body).send().await?;

        // Return error for non-success status codes
        if !response.status().is_success() {
            let status = response.status();
            error!(
                "Upstream request to {} failed with status: {}",
                endpoint, status
            );
            // Log detailed error internally, return generic error to client
            return Err(anyhow::anyhow!("Service temporarily unavailable"));
        }

        info!("Request succeeded with status: {}", response.status());
        Ok(response)
    }

    /// Forward a streaming request to Aperture, returning chunks as they arrive
    pub async fn forward_request_stream(
        &self,
        endpoint: &str,
        body: Vec<u8>,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<String>> + Send>>> {
        let parsed_url = self.validate_endpoint(endpoint)?;

        debug!("Forwarding streaming request to {}", parsed_url);
        info!("Proxying streaming to: {}", endpoint);

        let mut request = self
            .client
            .post(parsed_url)
            .header("Content-Type", "application/json");

        // Add API key if configured
        if let Some(ref api_key) = self.aperture_config.api_key {
            request = request.header("x-api-key", api_key);
        }

        let response = request.body(body).send().await?;

        // Check for non-success status codes
        if !response.status().is_success() {
            let status = response.status();
            error!(
                "Upstream streaming request to {} failed with status: {}",
                endpoint, status
            );
            // Return generic error to client (detailed error logged internally)
            return Ok(Box::pin(stream::once(async move {
                Err(anyhow::anyhow!("Service temporarily unavailable"))
            })));
        }

        info!(
            "Streaming request succeeded with status: {}",
            response.status()
        );

        // Track cumulative streaming size with AtomicUsize for thread-safety
        let total_bytes = AtomicUsize::new(0);
        let max_size = self.max_streaming_size_bytes;

        // Convert response body chunks to a stream with size tracking
        let byte_stream = response.bytes_stream().map(move |chunk_result| {
            chunk_result
                .map_err(|e| anyhow::anyhow!("Stream error: {}", e))
                .and_then(|bytes| {
                    // Use compare_exchange loop to prevent TOCTOU race condition
                    // This ensures we check the limit BEFORE adding, not after
                    let chunk_size = bytes.len();
                    loop {
                        let current = total_bytes.load(Ordering::SeqCst);

                        // Check if adding this chunk would exceed the limit
                        if current + chunk_size > max_size {
                            return Err(anyhow::anyhow!(
                                "Streaming response size limit exceeded (max {} MB, current {})",
                                max_size / 1024 / 1024,
                                current / 1024 / 1024
                            ));
                        }

                        // Try to atomically update the counter
                        match total_bytes.compare_exchange(
                            current,
                            current + chunk_size,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        ) {
                            Ok(_) => break,     // Successfully updated
                            Err(_) => continue, // Another thread updated, retry
                        }
                    }

                    std::str::from_utf8(&bytes)
                        .map(|s| s.to_string())
                        .map_err(|e| anyhow::anyhow!("UTF-8 error: {}", e))
                })
        });

        let stream: Pin<Box<dyn Stream<Item = anyhow::Result<String>> + Send>> =
            Box::pin(byte_stream);
        Ok(stream)
    }

    /// Get the base URL for this client
    pub fn base_url(&self) -> &str {
        &self.aperture_config.base_url
    }

    /// Get the API key for this client (if configured)
    pub fn api_key(&self) -> Option<&String> {
        self.aperture_config.api_key.as_ref()
    }

    /// Forward a request to a specific URL (for multi-provider support)
    pub async fn forward_request_to_url(
        &self,
        url: &str,
        body: Vec<u8>,
        api_key: Option<&str>,
    ) -> anyhow::Result<reqwest::Response> {
        // Validate URL is properly formed
        let parsed_url = Url::parse(url)?;

        // Validate scheme is HTTPS or HTTP
        if !matches!(parsed_url.scheme(), "https" | "http") {
            return Err(anyhow::anyhow!(
                "Invalid URL scheme. Only http and https are allowed."
            ));
        }

        // SSRF protection for provider URLs:
        // 1. Always block cloud metadata endpoints (169.254.169.254, etc.)
        // 2. Block internal IPs (private, loopback, link-local)
        //    Exception: CGN range 100.64.0.0/10 is allowed for Tailscale providers
        // 3. DNS rebinding protection: resolve and validate IPs at request time
        if let Some(host) = parsed_url.host_str() {
            // Always block metadata endpoints regardless of provider config
            if is_metadata_endpoint(host) {
                return Err(anyhow::anyhow!(
                    "Access to metadata endpoint '{}' is blocked (SSRF protection)",
                    host
                ));
            }

            // For IP-based hosts, validate they're not private/loopback
            // Exception: CGN range (100.64.0.0/10) allowed for Tailscale
            if let Ok(ip) = host.parse::<IpAddr>() {
                if is_internal_ip_strict(&ip) {
                    return Err(anyhow::anyhow!(
                        "Access to internal IP '{}' is blocked (SSRF protection). \
                         Use Tailscale (100.64.0.0/10) or public IPs for providers.",
                        ip
                    ));
                }
            } else {
                // For hostname-based URLs, resolve DNS and validate IPs (DNS rebinding protection)
                let port = parsed_url
                    .port()
                    .unwrap_or(if parsed_url.scheme() == "https" {
                        443
                    } else {
                        80
                    });
                validate_resolved_ips(host, port).await?;
            }
        }

        debug!("Forwarding request to custom URL: {}", url);

        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json");

        // Add API key if provided
        if let Some(key) = api_key {
            request = request.header("x-api-key", key);
        }

        let response = request.body(body).send().await?;

        // Return error for non-success status codes
        if !response.status().is_success() {
            let status = response.status();
            error!("Upstream request to {} failed with status: {}", url, status);
            // Log detailed error internally, return generic error to client
            return Err(anyhow::anyhow!("Service temporarily unavailable"));
        }

        info!(
            "Request to {} succeeded with status: {}",
            url,
            response.status()
        );
        Ok(response)
    }

    /// Validate endpoint and return parsed URL
    /// Performs endpoint whitelist check, URL parsing, scheme validation, and SSRF protection
    fn validate_endpoint(&self, endpoint: &str) -> anyhow::Result<url::Url> {
        // Validate endpoint is in whitelist (using static list, no allocation)
        if !is_allowed_endpoint(endpoint) {
            error!("Blocked request to disallowed endpoint: {}", endpoint);
            return Err(anyhow::anyhow!(
                "Endpoint '{}' is not in the allowed list",
                endpoint
            ));
        }

        // Construct and validate full URL
        let full_url = format!(
            "{}/{}",
            self.aperture_config.base_url.trim_end_matches('/'),
            endpoint
        );

        // Validate URL is properly formed
        let parsed_url = Url::parse(&full_url)?;

        // Validate scheme is HTTPS or HTTP (for Tailscale)
        if !matches!(parsed_url.scheme(), "https" | "http") {
            return Err(anyhow::anyhow!(
                "Invalid URL scheme in endpoint. Only http and https are allowed."
            ));
        }

        // SSRF Protection: Validate host is not an internal IP (unless it's the configured Aperture gateway)
        // This prevents requests to internal services while allowing localhost/127.0.0.1 for development
        if let Some(host) = parsed_url.host_str() {
            // Skip internal IP check if the host matches the configured base URL
            // This allows legitimate use of localhost/Tailscale while blocking SSRF
            let base_url_host = Url::parse(&self.aperture_config.base_url)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.to_string()));

            let is_configured_gateway = base_url_host.as_deref() == Some(host);

            if !is_configured_gateway && (is_internal_ip(host) || is_metadata_endpoint(host)) {
                return Err(anyhow::anyhow!(
                    "Access to internal hosts '{}' is blocked (SSRF protection)",
                    host
                ));
            }
        }

        Ok(parsed_url)
    }
}

/// Core internal IP check shared between both variants
/// Returns true if the IP is private, loopback, link-local, or (if block_cgn) CGN range
fn is_internal_ip_impl(ip: &IpAddr, block_cgn: bool) -> bool {
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
            || matches!(v6.octets()[0], 0xfe) && (v6.octets()[1] & 0xc0) == 0x80
            // Block multicast (ff00::/8)
            || v6.is_multicast()
        }
    }
}

/// Check if an IP address is internal/private (blocks CGN range)
fn is_internal_ip(host: &str) -> bool {
    host.parse::<IpAddr>()
        .map(|ip| is_internal_ip_impl(&ip, true))
        .unwrap_or(false)
}

/// Strict internal IP check for provider URL validation (SSRF defense-in-depth)
/// Unlike is_internal_ip(), this does NOT block CGN (100.64.0.0/10) because
/// Tailscale deployments legitimately use this range
fn is_internal_ip_strict(ip: &IpAddr) -> bool {
    is_internal_ip_impl(ip, false)
}

/// Check if a host is a cloud metadata endpoint (by hostname patterns)
fn is_metadata_endpoint(host: &str) -> bool {
    // Exact match for IP-based metadata endpoints
    host == "169.254.169.254"
        || host == "[::ffff:169.254.169.254]"
        // Alibaba Cloud metadata
        || host == "100.100.100.200"
        // Hostname-based metadata endpoints (GCP, Azure)
        || host == "metadata.google.internal"
        || host == "metadata.azure.com"
}

/// Check if an IP address is a cloud metadata IP
fn is_metadata_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            // AWS/GCP/Azure metadata: 169.254.169.254
            octets == [169, 254, 169, 254]
            // Alibaba Cloud metadata: 100.100.100.200
            || octets == [100, 100, 100, 200]
        }
        IpAddr::V6(v6) => {
            // Check for IPv4-mapped metadata addresses
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_metadata_ip(&IpAddr::V4(v4));
            }
            false
        }
    }
}

/// Resolve hostname and validate all resolved IPs against SSRF protection
/// This prevents DNS rebinding attacks where DNS changes after validation
async fn validate_resolved_ips(host: &str, port: u16) -> anyhow::Result<()> {
    // Skip DNS resolution for IP addresses (already validated)
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }

    // Resolve the hostname
    let addr_str = format!("{}:{}", host, port);
    let addrs_result = net::lookup_host(&addr_str).await;

    match addrs_result {
        Ok(addrs_iterator) => {
            let addrs: Vec<_> = addrs_iterator.collect();
            let addr_count = addrs.len();

            if addrs.is_empty() {
                warn!("DNS resolution returned no addresses for: {}", host);
                return Err(anyhow::anyhow!("DNS resolution failed for host"));
            }

            for addr in addrs {
                let ip = addr.ip();
                if is_internal_ip_strict(&ip) {
                    warn!(
                        "DNS rebinding blocked: {} resolved to internal IP {}",
                        host, ip
                    );
                    return Err(anyhow::anyhow!(
                        "Access to internal IP '{}' (resolved from '{}') is blocked (SSRF protection)",
                        ip, host
                    ));
                }

                // Check for metadata IP
                if is_metadata_ip(&ip) {
                    warn!(
                        "DNS rebinding blocked: {} resolved to metadata IP {}",
                        host, ip
                    );
                    return Err(anyhow::anyhow!(
                        "Access to metadata IP '{}' (resolved from '{}') is blocked",
                        ip,
                        host
                    ));
                }
            }

            debug!(
                "DNS resolution validated for {}: {} address(es)",
                host, addr_count
            );
            Ok(())
        }
        Err(e) => {
            // DNS resolution failure - log but don't block (let the request proceed and fail naturally)
            debug!(
                "DNS resolution failed for {}: {} (will fail at connection time)",
                host, e
            );
            Ok(())
        }
    }
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
        assert!(is_metadata_endpoint("100.100.100.200")); // Alibaba Cloud
    }

    #[test]
    fn test_metadata_endpoint_rejects_subdomains() {
        // Exact match prevents bypass via subdomains
        assert!(!is_metadata_endpoint("not-169.254.169.254.example.com"));
        assert!(!is_metadata_endpoint("fake-metadata.google.internal"));
        assert!(!is_metadata_endpoint("xmetadata.azure.com"));
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
        assert!(!is_internal_ip("203.0.113.1"));
    }

    #[test]
    fn test_internal_ip_blocks_cgn() {
        // CGN (100.64.127.1) is blocked by default is_internal_ip
        assert!(is_internal_ip("100.64.0.1"));
        assert!(is_internal_ip("100.127.255.255"));
    }

    #[test]
    fn test_internal_ip_strict_allows_cgn() {
        // Strict check allows CGN for Tailscale
        let cgn: IpAddr = "100.64.0.1".parse().unwrap();
        assert!(!is_internal_ip_strict(&cgn));
        let cgn2: IpAddr = "100.127.255.255".parse().unwrap();
        assert!(!is_internal_ip_strict(&cgn2));
    }

    #[test]
    fn test_internal_ip_strict_blocks_private() {
        let private: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(is_internal_ip_strict(&private));
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(is_internal_ip_strict(&loopback));
        let link_local: IpAddr = "169.254.1.1".parse().unwrap();
        assert!(is_internal_ip_strict(&link_local));
    }

    #[test]
    fn test_internal_ip_ipv6_loopback() {
        assert!(is_internal_ip("::1"));
    }

    #[test]
    fn test_internal_ip_ipv4_mapped() {
        // IPv4-mapped IPv6 addresses should be caught
        assert!(is_internal_ip("::ffff:10.0.0.1"));
        assert!(is_internal_ip("::ffff:127.0.0.1"));
    }

    #[test]
    fn test_https_enforcement_localhost() {
        // localhost with API key should be allowed (loopback)
        let config = crate::config::ApertureConfig {
            base_url: "http://localhost:8080".to_string(),
            api_key: Some("test-key-with-enough-entropy-abc123".to_string()),
            model_refresh_interval_secs: 300,
        };
        let http_config = crate::config::HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };
        assert!(ProxyClient::new(config, http_config, 1024).is_ok());
    }

    #[test]
    fn test_https_enforcement_127_ip() {
        let config = crate::config::ApertureConfig {
            base_url: "http://127.0.0.1:8080".to_string(),
            api_key: Some("test-key-with-enough-entropy-abc123".to_string()),
            model_refresh_interval_secs: 300,
        };
        let http_config = crate::config::HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };
        assert!(ProxyClient::new(config, http_config, 1024).is_ok());
    }

    #[test]
    fn test_https_enforcement_blocks_http_with_key() {
        let config = crate::config::ApertureConfig {
            base_url: "http://example.com:8080".to_string(),
            api_key: Some("test-key-with-enough-entropy-abc123".to_string()),
            model_refresh_interval_secs: 300,
        };
        let http_config = crate::config::HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };
        assert!(ProxyClient::new(config, http_config, 1024).is_err());
    }
}
