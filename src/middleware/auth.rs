// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use crate::config::{Config, CorsConfig, SecurityConfig};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

/// Authentication state with rate limiting
#[derive(Clone)]
pub struct AuthState {
    pub api_keys: Vec<String>,
    pub failed_attempts: Arc<RwLock<HashMap<IpAddr, Vec<Instant>>>>,
    pub max_attempts: usize,
    pub ban_duration: Duration,
    pub window_duration: Duration,
    pub trusted_proxies: HashSet<IpAddr>,
}

impl AuthState {
    pub fn new(security_config: &SecurityConfig, cors_config: &CorsConfig) -> Self {
        Self {
            api_keys: security_config.api_keys.clone(),
            failed_attempts: Arc::new(RwLock::new(HashMap::new())),
            max_attempts: security_config.max_auth_attempts,
            ban_duration: Duration::from_secs(security_config.ban_duration_secs),
            window_duration: Duration::from_secs(security_config.auth_window_secs),
            trusted_proxies: cors_config.trusted_proxies.clone(),
        }
    }

    /// Check if authentication is enabled
    pub fn is_enabled(&self) -> bool {
        !self.api_keys.is_empty()
    }

    /// Check rate limit and record failure atomically
    pub async fn check_and_record_failure(&self, client_ip: IpAddr) -> Result<(), StatusCode> {
        let mut attempts = self.failed_attempts.write().await;

        let now = Instant::now();
        let attempt_times = attempts.entry(client_ip).or_insert_with(Vec::new);

        // Remove old attempts outside the window
        attempt_times.retain(|timestamp| now.duration_since(*timestamp) < self.window_duration);

        // Check if too many attempts in window
        if attempt_times.len() >= self.max_attempts {
            // Don't add new attempt since we're at limit
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }

        // Add this attempt
        attempt_times.push(Instant::now());

        Ok(())
    }

    pub async fn record_success(&self, client_ip: IpAddr) {
        let mut attempts = self.failed_attempts.write().await;
        attempts.remove(&client_ip);
    }

    /// Start background cleanup task with supervision
    /// Returns the task handle for lifecycle management
    pub fn start_cleanup_task(&self) -> tokio::task::JoinHandle<()> {
        let failed_attempts = self.failed_attempts.clone();
        let window_duration = self.window_duration;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // Clean every 5 minutes

            loop {
                interval.tick().await; // Wait for interval
                let mut attempts = failed_attempts.write().await;
                let now = Instant::now();

                // Clean up old attempts
                for (_, attempt_times) in attempts.iter_mut() {
                    attempt_times
                        .retain(|timestamp| now.duration_since(*timestamp) < window_duration);
                }

                // Remove IPs with empty attempt lists to prevent memory DoS
                attempts.retain(|_, times| !times.is_empty());
            }
        })
    }

    pub fn validate_api_key(&self, key: &str) -> bool {
        // Use HashSet for O(1) lookup
        self.api_keys
            .iter()
            .any(|valid_key| valid_key.as_bytes().ct_eq(key.as_bytes()).into())
    }
}

/// Extract client IP from request with proxy validation
fn extract_client_ip(
    request: &Request,
    trusted_proxies: &HashSet<IpAddr>,
) -> Result<IpAddr, StatusCode> {
    // Try to get the actual connection IP (cannot be spoofed)
    let peer_ip = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.ip())
        .ok_or(StatusCode::BAD_REQUEST)?;

    // If connection is from a trusted proxy, check x-forwarded-for header
    if trusted_proxies.contains(&peer_ip) {
        if let Some(forwarded_for) = request.headers().get("x-forwarded-for") {
            if let Ok(forwarded_str) = forwarded_for.to_str() {
                // Get leftmost IP (original client)
                if let Some(first_ip) = forwarded_str.split(',').next() {
                    if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                        return Ok(ip);
                    }
                }
            }
        }
    }

    // Fallback: Use the actual connection IP (prevents spoofing)
    Ok(peer_ip)
}

pub async fn auth_middleware(
    State((config, auth)): State<(Arc<Config>, Arc<AuthState>)>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // If no API keys configured, check if production mode requires auth
    if !auth.is_enabled() {
        if config.security.require_auth_in_prod && !cfg!(debug_assertions) {
            error!("Authentication required but no API keys configured");
        }
        return Ok(next.run(request).await);
    }

    // Extract client IP
    let client_ip = match extract_client_ip(&request, &auth.trusted_proxies) {
        Ok(ip) => ip,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Check rate limit and record failure atomically (fixes race condition)
    if let Err(status) = auth.check_and_record_failure(client_ip).await {
        warn!("Rate-limited authentication attempt from: {}", client_ip);
        return Err(status);
    }

    // Check for API key in headers
    let api_key = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .or_else(|| {
            request
                .headers()
                .get("x-api-key")
                .and_then(|h| h.to_str().ok())
        });

    // Use constant-time comparison for security
    let is_valid = match api_key {
        Some(key) => auth.validate_api_key(key),
        None => false,
    };

    if is_valid {
        debug!("Request authenticated successfully from: {}", client_ip);
        auth.record_success(client_ip).await;
        Ok(next.run(request).await)
    } else {
        warn!(
            "Authentication failed from: {} (missing or invalid API key)",
            client_ip
        );
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_comparison() {
        use subtle::ConstantTimeEq;

        let key1 = "test-api-key-with-sufficient-entropy";
        let key2 = "test-api-key-with-sufficient-entropy";
        let key3 = "different-key-with-sufficient-entropy";

        // Same keys should match
        let result: bool = key1.as_bytes().ct_eq(key2.as_bytes()).into();
        assert!(result);

        // Different keys should not match
        let result: bool = key1.as_bytes().ct_eq(key3.as_bytes()).into();
        assert!(!result);

        // Empty key should not match non-empty key
        let result: bool = key1.as_bytes().ct_eq(b"").into();
        assert!(!result);
    }

    #[test]
    fn test_ip_parsing() {
        let ip1: IpAddr = "192.168.1.1".parse().unwrap();
        let ip2: IpAddr = "10.0.0.1".parse().unwrap();
        let ip3: IpAddr = "::1".parse().unwrap();

        assert_eq!(ip1, IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 1)));
        assert_eq!(ip2, IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 1)));
        assert_eq!(
            ip3,
            IpAddr::V6(std::net::Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1))
        );
    }

    #[test]
    fn test_duration_creation() {
        let duration1 = Duration::from_secs(60);
        let duration2 = Duration::from_secs(300);

        assert_eq!(duration1.as_secs(), 60);
        assert_eq!(duration2.as_secs(), 300);
    }
}
