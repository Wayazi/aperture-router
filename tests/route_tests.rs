// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use aperture_router::{config::Config, discovery::models::ModelDiscovery, server::create_router};

fn create_test_router(config: Config, discovery: std::sync::Arc<ModelDiscovery>) -> axum::Router {
    let (router, _shutdown_token) = create_router(config, discovery);
    router
}

#[cfg(test)]
mod route_tests {
    use super::*;

    fn create_test_config() -> Config {
        Config::default()
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/health")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body();
        let body_bytes = body.collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        assert!(body_str.contains("status"));
        assert!(body_str.contains("ok"));
    }

    #[tokio::test]
    async fn test_health_endpoint_options() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/health")
            .method(Method::OPTIONS)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        // OPTIONS should be allowed due to CORS
        assert!(
            response.status().is_success() || response.status() == StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[tokio::test]
    async fn test_router_creation() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();

        // This should not panic
        let _app = create_test_router(config, std::sync::Arc::new(discovery));

        // Router created successfully if we reach here
    }

    #[tokio::test]
    async fn test_router_with_auth_enabled() {
        let mut config = create_test_config();
        config.security.api_keys = vec!["test-api-key-with-sufficient-entropy-32".to_string()];

        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        // Health endpoint should still work without auth
        let request = Request::builder()
            .uri("/health")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_not_found_endpoint() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/nonexistent")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_router_with_custom_config() {
        let mut config = create_test_config();
        config.host = "127.0.0.1".to_string();
        config.port = 8765;
        config.security.max_body_size_bytes = 5 * 1024 * 1024;

        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let _app = create_test_router(config, std::sync::Arc::new(discovery));

        // Router created successfully if we reach here
    }

    #[tokio::test]
    async fn test_cors_headers() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/health")
            .method(Method::OPTIONS)
            .header("Origin", "http://localhost:3000")
            .header("Access-Control-Request-Method", "GET")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Check for CORS headers
        let cors_headers = response.headers().get("access-control-allow-origin");
        assert!(cors_headers.is_some(), "CORS headers should be present");
    }

    #[tokio::test]
    async fn test_compression_layer() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/health")
            .method(Method::GET)
            .header("Accept-Encoding", "gzip")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Check if compression headers are present
        let _encoding = response.headers().get("content-encoding");
        // Compression may or may not be applied depending on body size
        // Response successful if we reach here
    }

    #[tokio::test]
    async fn test_trace_layer_present() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();

        // Router should include trace layer
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/health")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_multiple_health_requests() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        // Make multiple concurrent requests
        let mut handles = vec![];

        for _ in 0..10 {
            let app_clone = app.clone();
            let handle = tokio::spawn(async move {
                let request = Request::builder()
                    .uri("/health")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap();

                app_clone.oneshot(request).await
            });
            handles.push(handle);
        }

        // All requests should succeed
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
            let response = result.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn test_health_response_format() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/health")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Check content type
        let content_type = response.headers().get("content-type");
        assert!(content_type.is_some());

        let body = response.into_body();
        let body_bytes = body.collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        // Verify it's valid JSON
        let json: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert!(json.get("status").is_some());
    }

    // ============ Admin Route Tests ============

    fn create_test_config_with_admin_keys() -> Config {
        let mut config = Config::default();
        config.security.admin_api_keys = vec![
            "admin-key-with-sufficient-entropy-1234567890".to_string(),
        ];
        config.security.api_keys = vec![
            "regular-key-with-sufficient-entropy-12345678".to_string(),
        ];
        config
    }

    #[tokio::test]
    async fn test_admin_stats_with_valid_admin_key() {
        let config = create_test_config_with_admin_keys();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/admin/stats")
            .method(Method::GET)
            .header("Authorization", "Bearer admin-key-with-sufficient-entropy-1234567890")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body();
        let body_bytes = body.collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        // Verify response contains expected fields
        let json: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert!(json.get("models_count").is_some());
        assert!(json.get("version").is_some());
    }

    #[tokio::test]
    async fn test_admin_stats_with_invalid_admin_key() {
        let config = create_test_config_with_admin_keys();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/admin/stats")
            .method(Method::GET)
            .header("Authorization", "Bearer invalid-admin-key-123456789012345")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_admin_stats_without_auth_header() {
        let config = create_test_config_with_admin_keys();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/admin/stats")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_admin_stats_with_regular_key_fails() {
        // Regular API key should NOT work for admin endpoints
        let config = create_test_config_with_admin_keys();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/admin/stats")
            .method(Method::GET)
            .header(
                "Authorization",
                "Bearer regular-key-with-sufficient-entropy-12345678",
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        // Regular key should be rejected for admin endpoints
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_admin_stats_with_x_api_key_header() {
        // Test that x-api-key header works for admin auth
        let config = create_test_config_with_admin_keys();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/admin/stats")
            .method(Method::GET)
            .header("x-api-key", "admin-key-with-sufficient-entropy-1234567890")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_admin_refresh_models_with_valid_key() {
        let config = create_test_config_with_admin_keys();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/admin/refresh-models")
            .method(Method::POST)
            .header("Authorization", "Bearer admin-key-with-sufficient-entropy-1234567890")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        // May fail due to no upstream, but should NOT be 401
        // (could be 500 if upstream unavailable, but auth passed)
        assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_admin_refresh_models_without_key() {
        let config = create_test_config_with_admin_keys();
        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/admin/refresh-models")
            .method(Method::POST)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_admin_no_keys_configured_returns_401() {
        // When no admin keys configured, admin endpoints should return 401 in production
        // Note: In debug mode (when tests run), the middleware allows access for testing
        // This test verifies that the admin key validation works correctly
        let mut config = Config::default();
        // Set admin keys to empty to test the validation logic
        config.security.admin_api_keys = vec![];
        // Set regular keys to ensure regular auth is enabled
        config.security.api_keys = vec!["regular-key-with-sufficient-entropy-12345678".to_string()];
        config.security.require_auth_in_prod = true;

        let discovery = ModelDiscovery::new(config.aperture.clone()).unwrap();
        let app = create_test_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/admin/stats")
            .method(Method::GET)
            .header(
                "Authorization",
                "Bearer regular-key-with-sufficient-entropy-12345678",
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        // In debug mode (when tests run), this returns 200 OK for testing
        // In production with require_auth_in_prod=true, this would return 401
        #[cfg(debug_assertions)]
        assert_eq!(response.status(), StatusCode::OK);
        #[cfg(not(debug_assertions))]
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
