// SPDX-License-Identifier: MIT
// Copyright (c) 2025 aperture-router contributors

use aperture_router::config::{CorsConfig, SecurityConfig};
use aperture_router::middleware::AuthState;
use http::StatusCode;
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

#[cfg(test)]
mod auth_tests {
    use super::*;

    fn create_auth_state(api_keys: Vec<String>) -> AuthState {
        let security_config = SecurityConfig {
            api_keys,
            max_body_size_bytes: 10 * 1024 * 1024,
            max_auth_attempts: 5,
            auth_window_secs: 60,
            ban_duration_secs: 300,
            require_auth_in_prod: true,
            max_json_depth: 256,
            max_streaming_size_bytes: 100 * 1024 * 1024,
        };

        let cors_config = CorsConfig::default();

        AuthState::new(&security_config, &cors_config)
    }

    #[tokio::test]
    async fn test_auth_state_is_enabled_no_keys() {
        let auth_state = create_auth_state(vec![]);
        assert!(!auth_state.is_enabled());
    }

    #[tokio::test]
    async fn test_auth_state_is_enabled_with_keys() {
        let api_keys = vec![
            "abcdefghijklmnopqrstuvwxyz123456".to_string(),
            "987654321zyxwvutsrqponmlkjihgfedcba".to_string(),
        ];
        let auth_state = create_auth_state(api_keys);
        assert!(auth_state.is_enabled());
    }

    #[tokio::test]
    async fn test_validate_api_key_valid() {
        let api_keys = vec!["test-api-key-with-sufficient-entropy-32".to_string()];
        let auth_state = create_auth_state(api_keys);

        assert!(auth_state.validate_api_key("test-api-key-with-sufficient-entropy-32"));
    }

    #[tokio::test]
    async fn test_validate_api_key_invalid() {
        let api_keys = vec!["valid-key-with-32-characters-20-unique".to_string()];
        let auth_state = create_auth_state(api_keys);

        assert!(!auth_state.validate_api_key("invalid-key"));
        assert!(!auth_state.validate_api_key(""));
    }

    #[tokio::test]
    async fn test_validate_api_key_multiple_keys() {
        let api_keys = vec![
            "first-valid-key-with-enough-entropy-1234".to_string(),
            "second-valid-key-with-different-chars-5678".to_string(),
        ];
        let auth_state = create_auth_state(api_keys);

        assert!(auth_state.validate_api_key("first-valid-key-with-enough-entropy-1234"));
        assert!(auth_state.validate_api_key("second-valid-key-with-different-chars-5678"));
        assert!(!auth_state.validate_api_key("non-existent-key"));
    }

    #[tokio::test]
    async fn test_check_and_record_failure_within_limits() {
        let api_keys = vec!["test-api-key-with-sufficient-entropy-32".to_string()];
        let auth_state = create_auth_state(api_keys);

        let client_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // Record several failures within limits
        for i in 0..4 {
            let result = auth_state.check_and_record_failure(client_ip).await;
            assert!(result.is_ok(), "Attempt {} should succeed", i);
        }
    }

    #[tokio::test]
    async fn test_check_and_record_failure_exceeds_limit() {
        let api_keys = vec!["test-api-key-with-sufficient-entropy-32".to_string()];
        let auth_state = create_auth_state(api_keys);

        let client_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // Record failures up to limit
        for _ in 0..5 {
            let result = auth_state.check_and_record_failure(client_ip).await;
            assert!(result.is_ok());
        }

        // Next attempt should fail
        let result = auth_state.check_and_record_failure(client_ip).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_record_success_clears_failures() {
        let api_keys = vec!["test-api-key-with-sufficient-entropy-32".to_string()];
        let auth_state = create_auth_state(api_keys);

        let client_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // Record some failures
        for _ in 0..3 {
            auth_state
                .check_and_record_failure(client_ip)
                .await
                .unwrap();
        }

        // Record success
        auth_state.record_success(client_ip).await;

        // Should be able to make attempts again
        let result = auth_state.check_and_record_failure(client_ip).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_and_record_failure_different_ips() {
        let api_keys = vec!["test-api-key-with-sufficient-entropy-32".to_string()];
        let auth_state = create_auth_state(api_keys);

        let ip1 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

        // Each IP should have its own counter
        for _ in 0..5 {
            auth_state.check_and_record_failure(ip1).await.unwrap();
        }

        // IP1 should be rate limited
        let result1 = auth_state.check_and_record_failure(ip1).await;
        assert!(result1.is_err());

        // IP2 should still be able to make attempts
        let result2 = auth_state.check_and_record_failure(ip2).await;
        assert!(result2.is_ok());
    }

    #[tokio::test]
    async fn test_auth_state_with_custom_limits() {
        let security_config = SecurityConfig {
            api_keys: vec!["test-api-key-with-sufficient-entropy-32".to_string()],
            max_body_size_bytes: 10 * 1024 * 1024,
            max_auth_attempts: 3,  // Lower limit
            auth_window_secs: 30,  // Shorter window
            ban_duration_secs: 60, // Shorter ban
            require_auth_in_prod: true,
            max_json_depth: 256,
            max_streaming_size_bytes: 100 * 1024 * 1024,
        };

        let cors_config = CorsConfig::default();
        let auth_state = AuthState::new(&security_config, &cors_config);

        let client_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // Should fail after 3 attempts
        for _ in 0..3 {
            assert!(auth_state.check_and_record_failure(client_ip).await.is_ok());
        }

        let result = auth_state.check_and_record_failure(client_ip).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_auth_state_trusted_proxies() {
        let mut cors_config = CorsConfig::default();
        cors_config.trusted_proxies = {
            let mut proxies = HashSet::new();
            proxies.insert(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
            proxies.insert(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
            proxies
        };

        let security_config = SecurityConfig {
            api_keys: vec!["test-api-key-with-sufficient-entropy-32".to_string()],
            max_body_size_bytes: 10 * 1024 * 1024,
            max_auth_attempts: 5,
            auth_window_secs: 60,
            ban_duration_secs: 300,
            require_auth_in_prod: true,
            max_json_depth: 256,
            max_streaming_size_bytes: 100 * 1024 * 1024,
        };

        let auth_state = AuthState::new(&security_config, &cors_config);

        assert_eq!(auth_state.trusted_proxies.len(), 2);
        assert!(auth_state
            .trusted_proxies
            .contains(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(auth_state
            .trusted_proxies
            .contains(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
    }

    #[tokio::test]
    async fn test_auth_state_cleanup_task_creation() {
        let api_keys = vec!["test-api-key-with-sufficient-entropy-32".to_string()];
        let auth_state = create_auth_state(api_keys);

        // This should not panic
        auth_state.start_cleanup_task();

        // Give it a moment to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // If we get here, the task started successfully
        assert!(true);
    }

    #[tokio::test]
    async fn test_validate_api_key_constant_time() {
        let api_keys = vec!["abcdefghijklmnopqrstuvwxyz123456".to_string()];
        let auth_state = create_auth_state(api_keys);

        // Test that timing attacks are mitigated by using constant-time comparison
        let valid_key = "abcdefghijklmnopqrstuvwxyz123456";
        let invalid_key = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";

        // Both should take roughly the same time (within reason)
        let start_valid = std::time::Instant::now();
        auth_state.validate_api_key(valid_key);
        let duration_valid = start_valid.elapsed();

        let start_invalid = std::time::Instant::now();
        auth_state.validate_api_key(invalid_key);
        let duration_invalid = start_invalid.elapsed();

        // Allow for some variance, but they should be within an order of magnitude
        let ratio = duration_valid.as_nanos() as f64 / duration_invalid.as_nanos().max(1) as f64;
        assert!(
            ratio > 0.1 && ratio < 10.0,
            "Timing analysis suggests non-constant-time comparison"
        );
    }

    #[tokio::test]
    async fn test_multiple_ips_independent_tracking() {
        let api_keys = vec!["test-api-key-with-sufficient-entropy-32".to_string()];
        let auth_state = create_auth_state(api_keys);

        let ips = vec![
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1)),
        ];

        // Each IP should be able to make 5 attempts
        for ip in &ips {
            for _ in 0..5 {
                assert!(auth_state.check_and_record_failure(*ip).await.is_ok());
            }
        }

        // Now all should be rate limited
        for ip in &ips {
            let result = auth_state.check_and_record_failure(*ip).await;
            assert!(result.is_err());
        }
    }

    #[tokio::test]
    async fn test_auth_state_durations() {
        let security_config = SecurityConfig {
            api_keys: vec!["test-api-key-with-sufficient-entropy-32".to_string()],
            max_body_size_bytes: 10 * 1024 * 1024,
            max_auth_attempts: 5,
            auth_window_secs: 60,
            ban_duration_secs: 300,
            require_auth_in_prod: true,
            max_json_depth: 256,
            max_streaming_size_bytes: 100 * 1024 * 1024,
        };

        let cors_config = CorsConfig::default();
        let auth_state = AuthState::new(&security_config, &cors_config);

        assert_eq!(auth_state.window_duration, Duration::from_secs(60));
        assert_eq!(auth_state.ban_duration, Duration::from_secs(300));
    }
}
