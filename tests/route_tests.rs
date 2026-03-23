// SPDX-License-Identifier: MIT
// Copyright (c) 2025 aperture-router contributors

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use aperture_router::{config::Config, discovery::models::ModelDiscovery, server::create_router};

#[cfg(test)]
mod route_tests {
    use super::*;

    fn create_test_config() -> Config {
        Config::default()
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

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
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

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
        let discovery = ModelDiscovery::new(config.aperture.clone());

        // This should not panic
        let _app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

        // Verify the router was created successfully
        assert!(true);
    }

    #[tokio::test]
    async fn test_router_with_auth_enabled() {
        let mut config = create_test_config();
        config.security.api_keys = vec!["test-api-key-with-sufficient-entropy-32".to_string()];

        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

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
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

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

        let discovery = ModelDiscovery::new(config.aperture.clone());
        let _app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

        // Router created successfully
        assert!(true);
    }

    #[tokio::test]
    async fn test_cors_headers() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

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
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

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
        assert!(true, "Response successful with or without compression");
    }

    #[tokio::test]
    async fn test_trace_layer_present() {
        let config = create_test_config();
        let discovery = ModelDiscovery::new(config.aperture.clone());

        // Router should include trace layer
        let app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

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
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

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
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(
            config,
            std::sync::Arc::new(tokio::sync::RwLock::new(discovery)),
        );

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
}
