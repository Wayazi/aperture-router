// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use axum::http::StatusCode;
use tracing::debug;

const MAX_TRACKED_IPS: usize = 10000;

pub struct RateLimiter {
    requests: Arc<RwLock<HashMap<IpAddr, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            max_requests,
            window,
        }
    }

    pub async fn check_rate_limit(&self, client_ip: IpAddr) -> Result<(), StatusCode> {
        let mut requests = self.requests.write().await;
        let now = Instant::now();

        if requests.len() >= MAX_TRACKED_IPS && !requests.contains_key(&client_ip) {
            if let Some(oldest_ip) = requests
                .iter()
                .filter_map(|(ip, times)| times.last().map(|t| (*ip, *t)))
                .min_by_key(|(_, t)| *t)
                .map(|(ip, _)| ip)
            {
                debug!("Rate limiter evicting oldest IP {}", oldest_ip);
                requests.remove(&oldest_ip);
            }
        }

        let entry = requests.entry(client_ip).or_insert_with(Vec::new);
        entry.retain(|&time| now.duration_since(time) < self.window);

        if entry.len() >= self.max_requests {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }

        entry.push(now);
        Ok(())
    }

    /// Manually cleanup expired entries (used in tests)
    #[cfg(test)]
    pub async fn cleanup_expired(&self) {
        let mut requests = self.requests.write().await;
        let now = Instant::now();
        
        for entry in requests.values_mut() {
            entry.retain(|&time| now.duration_since(time) < self.window);
        }
        
        requests.retain(|_, entry| !entry.is_empty());
    }

    pub fn start_cleanup_task(&self) -> tokio::task::JoinHandle<()> {
        let requests = self.requests.clone();
        let window = self.window;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300));

            loop {
                interval.tick().await;
                let mut reqs = requests.write().await;
                let now = Instant::now();

                for entry in reqs.values_mut() {
                    entry.retain(|&time| now.duration_since(time) < window);
                }

                reqs.retain(|_, entry| !entry.is_empty());
            }
        })
    }
}

impl Clone for RateLimiter {
    fn clone(&self) -> Self {
        Self {
            requests: Arc::clone(&self.requests),
            max_requests: self.max_requests,
            window: self.window,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_rate_limit_allows_within_limit() {
        let limiter = RateLimiter::new(10, Duration::from_secs(60));
        let ip = "192.168.1.1".parse().unwrap();
        
        for _ in 0..10 {
            assert!(limiter.check_rate_limit(ip).await.is_ok());
        }
    }
    
    #[tokio::test]
    async fn test_rate_limit_blocks_over_limit() {
        let limiter = RateLimiter::new(5, Duration::from_secs(60));
        let ip = "192.168.1.1".parse().unwrap();
        
        for _ in 0..5 {
            limiter.check_rate_limit(ip).await.ok();
        }
        
        // 6th request should fail
        assert_eq!(
            limiter.check_rate_limit(ip).await,
            Err(StatusCode::TOO_MANY_REQUESTS)
        );
    }
    
    #[tokio::test]
    async fn test_rate_limit_independent_per_ip() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        let ip1 = "192.168.1.1".parse().unwrap();
        let ip2 = "192.168.1.2".parse().unwrap();
        
        assert!(limiter.check_rate_limit(ip1).await.is_ok());
        assert!(limiter.check_rate_limit(ip1).await.is_ok());
        assert_eq!(limiter.check_rate_limit(ip1).await, Err(StatusCode::TOO_MANY_REQUESTS));
        
        // ip2 should still be allowed
        assert!(limiter.check_rate_limit(ip2).await.is_ok());
        assert!(limiter.check_rate_limit(ip2).await.is_ok());
        assert_eq!(limiter.check_rate_limit(ip2).await, Err(StatusCode::TOO_MANY_REQUESTS));
    }
    
    #[tokio::test]
    async fn test_rate_limit_memory_cap() {
        // Create limiter with small capacity for testing
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        
        // Add IPs up to just below the cap
        for i in 0..(MAX_TRACKED_IPS - 1) {
            let ip: IpAddr = format!("192.168.{}.{}", i / 256, i % 256).parse().unwrap();
            assert!(limiter.check_rate_limit(ip).await.is_ok());
        }
        
        // Verify we can still add new IPs (oldest should be evicted)
        let new_ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(limiter.check_rate_limit(new_ip).await.is_ok());
        
        // Verify the map doesn't exceed the cap
        let requests = limiter.requests.read().await;
        assert!(requests.len() <= MAX_TRACKED_IPS);
    }
    
    #[tokio::test]
    async fn test_rate_limit_window_expiry() {
        let limiter = RateLimiter::new(1, Duration::from_millis(100));
        let ip = "192.168.1.1".parse().unwrap();

        assert!(limiter.check_rate_limit(ip).await.is_ok());
        assert_eq!(limiter.check_rate_limit(ip).await, Err(StatusCode::TOO_MANY_REQUESTS));

        tokio::time::sleep(Duration::from_millis(150)).await;

        assert!(limiter.check_rate_limit(ip).await.is_ok());
    }
}
