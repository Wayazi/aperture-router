// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use crate::config::{Config, CorsConfig, SecurityConfig};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use zeroize::Zeroizing;

/// Helper to hash an API key for logging (without exposing the actual key)
/// Uses SHA-256 for cryptographic security (prevents rainbow table attacks)
fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let result = hasher.finalize();
    // Only log first 8 bytes (16 hex chars) for brevity
    let truncated: [u8; 8] = result[..8].try_into().unwrap_or([0u8; 8]);
    format!("key_{:02x?}", truncated)
}

/// Maximum number of tracked IPs to prevent memory exhaustion from unique-IP DDoS
const MAX_TRACKED_IPS: usize = 10000;

/// Authentication state with rate limiting
/// Uses Zeroizing<String> to securely wipe API keys from memory on drop
#[derive(Clone)]
pub struct AuthState {
    pub api_keys: Vec<Zeroizing<String>>,
    pub admin_api_keys: Vec<Zeroizing<String>>,
    pub failed_attempts: Arc<RwLock<HashMap<IpAddr, Vec<Instant>>>>,
    pub max_attempts: usize,
    pub ban_duration: Duration,
    pub window_duration: Duration,
    pub trusted_proxies: HashSet<IpAddr>,
}

impl AuthState {
    pub fn new(security_config: &SecurityConfig, cors_config: &CorsConfig) -> Self {
        // Security: Do NOT fall back to regular keys for admin operations
        // Admin endpoints require explicit admin_api_keys configuration
        let admin_keys: Vec<Zeroizing<String>> = security_config
            .admin_api_keys
            .iter()
            .map(|k| Zeroizing::new(k.clone()))
            .collect();

        // Log a warning if admin keys are not configured
        if admin_keys.is_empty() {
            tracing::warn!(
                "No admin API keys configured. Admin endpoints (/admin/*) will be inaccessible."
            );
            tracing::info!("To enable admin access, add admin_api_keys to your configuration.");
        }

        Self {
            api_keys: security_config
                .api_keys
                .iter()
                .map(|k| Zeroizing::new(k.clone()))
                .collect(),
            admin_api_keys: admin_keys,
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

    /// Check if admin authentication is enabled
    pub fn is_admin_enabled(&self) -> bool {
        !self.admin_api_keys.is_empty()
    }

    /// Check rate limit and record failure atomically
    /// Evicts oldest entries when MAX_TRACKED_IPS is exceeded to prevent memory exhaustion
    pub async fn check_and_record_failure(&self, client_ip: IpAddr) -> Result<(), StatusCode> {
        let mut attempts = self.failed_attempts.write().await;

        let now = Instant::now();

        // Evict oldest entries if we've hit the IP tracking cap
        if attempts.len() >= MAX_TRACKED_IPS && !attempts.contains_key(&client_ip) {
            // Find and remove the IP with the oldest most-recent attempt
            if let Some(oldest_ip) = attempts
                .iter()
                .filter_map(|(ip, times)| times.last().map(|t| (*ip, *t)))
                .min_by_key(|(_, t)| *t)
                .map(|(ip, _)| ip)
            {
                debug!(
                    "Evicting oldest tracked IP {} to cap memory usage",
                    oldest_ip
                );
                attempts.remove(&oldest_ip);
            }
        }

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

    /// Validate API key with timing-safe comparison
    /// Compares against ALL keys to prevent timing attacks
    /// Uses bitwise OR to ensure no short-circuit evaluation
    pub fn validate_api_key(&self, key: &str) -> bool {
        let key_bytes = key.as_bytes();
        let mut found = 0u8; // Use integer for constant-time OR
        for valid_key in &self.api_keys {
            // Always perform the comparison (no short-circuit with bitwise OR)
            let matches: u8 = if bool::from(valid_key.as_bytes().ct_eq(key_bytes)) { 1 } else { 0 };
            found |= matches; // Bitwise OR is constant-time
        }
        found == 1
    }

    /// Validate admin API key with timing-safe comparison
    /// Compares against ALL keys to prevent timing attacks
    /// Uses bitwise OR to ensure no short-circuit evaluation
    pub fn validate_admin_key(&self, key: &str) -> bool {
        let key_bytes = key.as_bytes();
        let mut found = 0u8; // Use integer for constant-time OR
        for valid_key in &self.admin_api_keys {
            // Always perform the comparison (no short-circuit with bitwise OR)
            let matches: u8 = if bool::from(valid_key.as_bytes().ct_eq(key_bytes)) { 1 } else { 0 };
            found |= matches; // Bitwise OR is constant-time
        }
        found == 1
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
        .unwrap_or_else(|| {
            // Fallback for test environments where ConnectInfo is not available
            // This is safe in tests but should not happen in production
            if cfg!(test) {
                std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))
            } else {
                // In production, this should never happen
                std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0))
            }
        });

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

/// Admin-specific authentication middleware
/// Requires admin API key for access to administrative endpoints
pub async fn admin_auth_middleware(
    State((_config, auth)): State<(Arc<Config>, Arc<AuthState>)>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Admin endpoints require explicit admin API key configuration
    if !auth.is_admin_enabled() {
        // In production, always require admin keys
        #[cfg(not(debug_assertions))]
        {
            error!("Admin endpoint accessed but no admin API keys configured");
            return Err(StatusCode::UNAUTHORIZED);
        }

        // In dev mode, allow access only with explicit opt-in via env var
        #[cfg(debug_assertions)]
        {
            if std::env::var("APERTURE_ALLOW_DEV_ADMIN").as_deref() == Ok("1") {
                tracing::warn!("Admin endpoint accessed in dev mode without admin keys (APERTURE_ALLOW_DEV_ADMIN=1)");
                return Ok(next.run(request).await);
            }
            return Err(StatusCode::UNAUTHORIZED);
        }

        // Fallback for non-unix or edge cases (should not be reached in normal builds)
        #[allow(unreachable_code)]
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Extract client IP
    let client_ip = match extract_client_ip(&request, &auth.trusted_proxies) {
        Ok(ip) => ip,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Check rate limit and record failure atomically
    if let Err(status) = auth.check_and_record_failure(client_ip).await {
        warn!(
            "Rate-limited admin authentication attempt from: {}",
            client_ip
        );
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

    // Use constant-time comparison with admin keys
    let is_valid = match api_key {
        Some(key) => auth.validate_admin_key(key),
        None => false,
    };

    if is_valid {
        info!(
            client_ip = %client_ip,
            key_hash = hash_api_key(api_key.unwrap_or("")),
            endpoint = "admin",
            "Admin authentication successful"
        );
        auth.record_success(client_ip).await;
        Ok(next.run(request).await)
    } else {
        warn!(
            client_ip = %client_ip,
            endpoint = "admin",
            reason = match api_key {
                Some(_) => "invalid_admin_key",
                None => "missing_api_key",
            },
            "Admin authentication failed"
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
