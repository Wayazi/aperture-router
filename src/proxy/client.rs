// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use futures::{stream, Stream, StreamExt};
use reqwest::Client;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::net;
use tracing::{debug, error, info, warn};
use url::Url;

use crate::config::{ApertureConfig, EndpointStyle, HttpConfig};
use crate::http_client::{create_client_with_timeouts, is_allowed_endpoint};
use crate::security::{is_internal_ip, is_internal_ip_strict, is_metadata_endpoint};

/// A boxed stream of upstream text chunks (UTF-8-reassembled) with per-chunk errors.
pub type BoxedResultStream = Pin<Box<dyn Stream<Item = anyhow::Result<String>> + Send>>;

/// Convert a byte stream into a String stream with UTF-8 buffering.
/// Multi-byte characters split across TCP chunks are reassembled correctly.
fn make_utf8_stream(
    response: reqwest::Response,
    total_bytes: AtomicUsize,
    max_size: usize,
) -> Pin<Box<dyn Stream<Item = anyhow::Result<String>> + Send>> {
    let leftover: Mutex<Vec<u8>> = Mutex::new(Vec::new());

    let byte_stream = response.bytes_stream().map(move |chunk_result| {
        chunk_result
            .map_err(|e| anyhow::anyhow!("Stream error: {}", e))
            .and_then(|bytes| {
                let chunk_size = bytes.len();
                loop {
                    let current = total_bytes.load(Ordering::SeqCst);
                    if current + chunk_size > max_size {
                        return Err(anyhow::anyhow!(
                            "Streaming response size limit exceeded (max {} MB, current {})",
                            max_size / 1024 / 1024,
                            current / 1024 / 1024
                        ));
                    }
                    match total_bytes.compare_exchange(
                        current,
                        current + chunk_size,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    ) {
                        Ok(_) => break,
                        Err(_) => continue,
                    }
                }

                // Poisoned-lock recovery: a panic while holding the leftover
                // buffer must not kill the stream; the buffer contents are
                // still plain bytes and remain valid.
                let mut buf = match leftover.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("UTF-8 leftover mutex poisoned; recovering buffer");
                        poisoned.into_inner()
                    }
                };
                buf.extend_from_slice(&bytes);

                match std::str::from_utf8(&buf) {
                    Ok(_) => {
                        let s = String::from_utf8(std::mem::take(&mut *buf)).unwrap_or_default();
                        Ok(s)
                    }
                    Err(e) => {
                        let safe_len = e.valid_up_to();
                        if safe_len == 0 {
                            let needed = e.error_len().unwrap_or(1);
                            if buf.len() > 4 + needed {
                                let s = String::from_utf8_lossy(&buf).to_string();
                                error!("UTF-8 decode failed, lossy fallback: {}", e);
                                buf.clear();
                                return Ok(s);
                            }
                            Ok(String::new())
                        } else {
                            let s = std::str::from_utf8(&buf[..safe_len])
                                .unwrap_or("")
                                .to_string();
                            let remaining = buf[safe_len..].to_vec();
                            buf.clear();
                            buf.extend(remaining);
                            Ok(s)
                        }
                    }
                }
            })
    });

    Box::pin(byte_stream)
}

/// HTTP client for proxying requests to Aperture
#[derive(Clone)]
pub struct ProxyClient {
    client: Client,
    aperture_config: ApertureConfig,
    max_streaming_size_bytes: usize,
    request_timeout: Duration,
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

        // Allow HTTP for localhost (development/testing) - any port
        // Use proper URL parsing to detect all localhost forms (IPv6 [::1], 127.x, etc.)
        let host_str = Url::parse(&aperture_config.base_url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()));

        // Tailscale assigns addresses from the whole CGNAT range 100.64.0.0/10,
        // so detect it by parsing the host IP rather than substring matching.
        // MagicDNS names end in ".ts.net" — check the parsed HOST as a suffix,
        // not the raw URL: a contains()-style check never matches real hosts
        // ("ts.net" is followed by end-of-string, ':' or '/') and instead
        // whitelists lookalike domains like evil.ts.net.attacker.com.
        let is_tailscale = host_str
            .as_deref()
            .map(|host| {
                host.parse::<IpAddr>()
                    .map(|ip| crate::security::is_cgnat(&ip))
                    .unwrap_or(false)
                    || host.ends_with(".ts.net")
                    || host.ends_with(".tsnet")
            })
            .unwrap_or(false);

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

        let request_timeout = Duration::from_secs(http_config.request_timeout_secs);

        Ok(Self {
            client,
            aperture_config,
            max_streaming_size_bytes,
            request_timeout,
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

        let total_bytes = AtomicUsize::new(0);
        let max_size = self.max_streaming_size_bytes;

        let stream = make_utf8_stream(response, total_bytes, max_size);
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
        endpoint_style: EndpointStyle,
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
        validate_provider_url_host(&parsed_url).await?;

        debug!("Forwarding request to custom URL: {}", url);

        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json");

        if let Some(key) = api_key {
            request = add_auth_header(request, key, endpoint_style);
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

    pub async fn forward_request_to_url_raw(
        &self,
        url: &str,
        body: Vec<u8>,
        api_key: Option<&str>,
        endpoint_style: EndpointStyle,
    ) -> anyhow::Result<reqwest::Response> {
        let parsed_url = Url::parse(url)?;

        if !matches!(parsed_url.scheme(), "https" | "http") {
            return Err(anyhow::anyhow!(
                "Invalid URL scheme. Only http and https are allowed."
            ));
        }

        validate_provider_url_host(&parsed_url).await?;

        debug!("Forwarding request to custom URL (raw): {}", url);

        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json");

        if let Some(key) = api_key {
            request = add_auth_header(request, key, endpoint_style);
        }

        let response = request.body(body).send().await?;

        Ok(response)
    }

    pub async fn forward_request_stream_to_url(
        &self,
        url: &str,
        body: Vec<u8>,
        api_key: Option<&str>,
        endpoint_style: EndpointStyle,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<String>> + Send>>> {
        let parsed_url = Url::parse(url)?;

        if !matches!(parsed_url.scheme(), "https" | "http") {
            return Err(anyhow::anyhow!(
                "Invalid URL scheme. Only http and https are allowed."
            ));
        }

        validate_provider_url_host(&parsed_url).await?;

        debug!("Forwarding streaming request to custom URL: {}", url);

        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json");

        if let Some(key) = api_key {
            request = add_auth_header(request, key, endpoint_style);
        }

        let request_timeout = self.request_timeout;
        let response = tokio::time::timeout(request_timeout, request.body(body).send())
            .await
            .map_err(|_| anyhow::anyhow!("Streaming request to {} timed out", url))??;

        if !response.status().is_success() {
            let status = response.status();
            let body = match crate::http_client::read_body_capped(
                response,
                crate::http_client::MAX_ERROR_BODY_BYTES,
                "Upstream error body",
            )
            .await
            {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(_) => String::new(),
            };
            error!(
                "Upstream streaming request to {} failed with status: {} body: {}",
                url, status, body
            );
            return Err(anyhow::anyhow!("Service temporarily unavailable"));
        }

        info!(
            "Streaming request to {} succeeded with status: {}",
            url,
            response.status()
        );

        let total_bytes = AtomicUsize::new(0);
        let max_size = self.max_streaming_size_bytes;

        let stream = make_utf8_stream(response, total_bytes, max_size);
        Ok(stream)
    }

    fn validate_endpoint(&self, endpoint: &str) -> anyhow::Result<url::Url> {
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

fn add_auth_header(
    mut request: reqwest::RequestBuilder,
    key: &str,
    endpoint_style: EndpointStyle,
) -> reqwest::RequestBuilder {
    match endpoint_style {
        EndpointStyle::Anthropic => {
            request = request.header("x-api-key", key);
        }
        _ => {
            request = request.header("authorization", format!("Bearer {}", key));
        }
    }
    request
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
            // DNS resolution failure — fail CLOSED. An attacker controlling DNS can
            // answer SERVFAIL during validation and rebind to an internal IP at
            // connect time; letting the request proceed would defeat this check.
            warn!(
                "DNS resolution failed for {}: {} (request blocked: fail-closed rebinding protection)",
                host, e
            );
            Err(anyhow::anyhow!(
                "DNS resolution failed for '{}' (SSRF protection: fail-closed)",
                host
            ))
        }
    }
}

/// SSRF host validation shared by all provider-forwarding paths.
///
/// Blocks cloud metadata hosts, internal/loopback IP literals (CGN 100.64.0.0/10
/// allowed for Tailscale), and for hostname-based URLs resolves DNS and validates
/// every returned address (DNS rebinding protection). Every `*_to_url` forward
/// variant must call this before sending.
async fn validate_provider_url_host(parsed_url: &Url) -> anyhow::Result<()> {
    if let Some(host) = parsed_url.host_str() {
        if is_metadata_endpoint(host) {
            return Err(anyhow::anyhow!(
                "Access to metadata endpoint '{}' is blocked (SSRF protection)",
                host
            ));
        }

        if let Ok(ip) = host.parse::<IpAddr>() {
            if is_internal_ip_strict(&ip) {
                return Err(anyhow::anyhow!(
                    "Access to internal IP '{}' is blocked (SSRF protection). \
                     Use Tailscale (100.64.0.0/10) or public IPs for providers.",
                    ip
                ));
            }
        } else {
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
    Ok(())
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

#[cfg(test)]
mod panic_recovery_tests {
    #[test]
    fn test_poisoned_mutex_recovery_pattern() {
        // Reproduce the exact recovery pattern used in make_utf8_stream:
        // a panicked holder poisons the mutex; the reader must recover the
        // (still valid) buffer contents instead of panicking.
        let leftover: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(b"part".to_vec());
        let clone_for_panic = std::sync::Arc::new(leftover);
        let writer = std::sync::Arc::clone(&clone_for_panic);
        let _ = std::thread::spawn(move || {
            let _guard = writer.lock().unwrap();
            panic!("simulated panic while holding the lock");
        })
        .join();

        let mut buf = match clone_for_panic.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                // The data written before the panic is intact.
                poisoned.into_inner()
            }
        };
        assert_eq!(&*buf, b"part", "pre-panic buffer content must survive");
        buf.extend_from_slice(b"-continued");
        assert_eq!(&*buf, b"part-continued");
    }
}
