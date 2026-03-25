// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use futures::{stream, Stream, StreamExt};
use reqwest::Client;
use std::collections::HashSet;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tracing::{debug, error, info};
use url::Url;

use crate::config::{ApertureConfig, HttpConfig};

/// HTTP client for proxying requests to Aperture
#[derive(Clone)]
pub struct ProxyClient {
    client: Client,
    aperture_config: ApertureConfig,
    allowed_endpoints: HashSet<String>,
    max_streaming_size_bytes: usize,
}

impl ProxyClient {
    pub fn new(
        aperture_config: ApertureConfig,
        http_config: HttpConfig,
        max_streaming_size_bytes: usize,
    ) -> anyhow::Result<Self> {
        let timeout = Duration::from_secs(http_config.request_timeout_secs);
        let connect_timeout = Duration::from_secs(http_config.connect_timeout_secs);

        // Enforce HTTPS for non-Tailscale deployments ONLY when API key is configured
        // This prevents API key exposure over HTTP while allowing HTTP for:
        // - Tailscale deployments (encrypted at network layer)
        // - Localhost development
        // - Deployments without API keys (using other auth methods)
        let has_api_key = aperture_config.api_key.is_some();
        let is_tailscale = aperture_config.base_url.contains("100.100.")
            || aperture_config.base_url.contains(".tsnet.");

        // Allow HTTP for localhost (development/testing) - any port
        let is_localhost = aperture_config.base_url.contains("localhost:")
            || aperture_config.base_url.contains("127.0.0.1:")
            || aperture_config.base_url.contains("::ffff:127.0.0.1:");

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

        let client = Client::builder()
            .timeout(timeout)
            .connect_timeout(connect_timeout)
            // CRITICAL: Disable redirects to prevent SSRF bypass
            // An attacker could redirect to internal IPs (e.g., 169.254.169.254)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

        // Default allowed endpoints
        let allowed_endpoints: HashSet<String> = [
            "v1/chat/completions".to_string(),
            "v1/messages".to_string(),
            "v1/models".to_string(),
            "v1/embeddings".to_string(),
        ]
        .into_iter()
        .collect();

        Ok(Self {
            client,
            aperture_config,
            allowed_endpoints,
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
            error!("Upstream request failed with status: {}", status);
            return Err(anyhow::anyhow!(
                "Upstream service returned error: {}",
                status.as_u16()
            ));
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
            let status_code = status.as_u16();
            error!("Upstream streaming request failed with status: {}", status);
            return Ok(Box::pin(stream::once(async move {
                Err(anyhow::anyhow!(
                    "Upstream service returned error: {}",
                    status_code
                ))
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
            // Check size limit first
            let current = total_bytes.load(Ordering::Relaxed);
            if current > max_size {
                return Err(anyhow::anyhow!(
                    "Streaming response size limit exceeded (max {} MB)",
                    max_size / 1024 / 1024
                ));
            }

            chunk_result
                .map_err(|e| anyhow::anyhow!("Stream error: {}", e))
                .and_then(|bytes| {
                    // Update size counter
                    total_bytes.fetch_add(bytes.len(), Ordering::Relaxed);

                    // Check limit after update
                    let current = total_bytes.load(Ordering::Relaxed);
                    if current > max_size {
                        return Err(anyhow::anyhow!(
                            "Streaming response size limit exceeded (max {} MB)",
                            max_size / 1024 / 1024
                        ));
                    }

                    std::str::from_utf8(&bytes)
                        .map(|s| s.to_string())
                        .map_err(|e| anyhow::anyhow!("UTF-8 error: {}", e))
                })
        });

        Ok(Box::pin(byte_stream))
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

        // SSRF protection: check for metadata endpoints only
        // Note: We don't block internal IPs here because providers are admin-configured
        // and may legitimately use Tailscale (100.64.0.0/10), localhost, etc.
        // The redirect policy (disabled) provides the main SSRF defense.
        if let Some(host) = parsed_url.host_str() {
            if is_metadata_endpoint(host) {
                return Err(anyhow::anyhow!(
                    "Access to metadata endpoint '{}' is blocked (SSRF protection)",
                    host
                ));
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
            return Err(anyhow::anyhow!(
                "Upstream service returned error: {}",
                status.as_u16()
            ));
        }

        info!("Request to {} succeeded with status: {}", url, response.status());
        Ok(response)
    }

    /// Validate endpoint and return parsed URL
    /// Performs endpoint whitelist check, URL parsing, scheme validation, and SSRF protection
    fn validate_endpoint(&self, endpoint: &str) -> anyhow::Result<url::Url> {
        // Validate endpoint is in whitelist
        if !self.allowed_endpoints.contains(endpoint) {
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

/// Check if an IP address is internal/private
fn is_internal_ip(host: &str) -> bool {
    host.parse::<IpAddr>()
        .map(|ip| match ip {
            IpAddr::V4(v4) => {
                v4.is_private() || v4.is_loopback() || v4.is_link_local()
                // Also block shared/carrier-grade NAT (100.64.0.0/10)
                || v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1])
            }
            IpAddr::V6(v6) => {
                // Check for IPv4-mapped IPv6 addresses (::ffff:x.x.x.x)
                // These can encode internal IPv4 addresses and bypass checks
                if let Some(v4) = v6.to_ipv4_mapped() {
                    return v4.is_private()
                        || v4.is_loopback()
                        || v4.is_link_local()
                        || v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]);
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
        })
        .unwrap_or(false)
}

/// Check if a host is a metadata endpoint (AWS/GCP/Azure)
fn is_metadata_endpoint(host: &str) -> bool {
    host.contains("169.254.169.254")
        || host.contains("metadata.google.internal")
        || host.contains("metadata.azure.com")
}
