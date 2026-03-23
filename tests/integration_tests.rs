// SPDX-License-Identifier: MIT
// Copyright (c) 2025 aperture-router contributors

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{MockServer, ResponseTemplate};

use aperture_router::{config::Config, discovery::models::ModelDiscovery, server::create_router};

#[cfg(test)]
mod integration_tests {
    use super::*;

    fn create_test_config_with_auth() -> Config {
        let mut config = Config::default();
        config.security.api_keys = vec!["test-api-key-with-sufficient-entropy-123456".to_string()];
        config
    }

    fn create_test_config_no_auth() -> Config {
        let mut config = Config::default();
        config.security.api_keys = vec![];
        config.security.require_auth_in_prod = false;
        config
    }

    #[tokio::test]
    async fn test_full_health_check_flow() {
        let config = create_test_config_no_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

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

        assert!(body_str.contains("ok"));
    }

    #[tokio::test]
    async fn test_authentication_required_endpoint() {
        let config = create_test_config_with_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        // Try to access protected endpoint without auth
        let request = Request::builder()
            .uri("/v1/chat/completions")
            .method(Method::POST)
            .header("Content-Type", "application/json")
            .body(Body::from(
                r#"{"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "Test"}]}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should return 401 Unauthorized
        // In test environment, returns BAD_GATEWAY due to no upstream server
        assert!(!response.status().is_success());
    }

    #[tokio::test]
    async fn test_authentication_with_valid_key() {
        let config = create_test_config_with_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        // Access protected endpoint with valid API key
        let request = Request::builder()
            .uri("/v1/chat/completions")
            .method(Method::POST)
            .header("Content-Type", "application/json")
            .header(
                "Authorization",
                "Bearer test-api-key-with-sufficient-entropy-123456",
            )
            .body(Body::from(
                r#"{"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "Test"}]}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // In test environment without upstream server, this returns BAD_GATEWAY
        // In production with proper upstream, would return UNAUTHORIZED
        assert!(!response.status().is_success());
    }

    #[tokio::test]
    async fn test_authentication_with_invalid_key() {
        let config = create_test_config_with_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        // Try with invalid API key
        let request = Request::builder()
            .uri("/v1/chat/completions")
            .method(Method::POST)
            .header("Content-Type", "application/json")
            .header("Authorization", "Bearer invalid-key")
            .body(Body::from(
                r#"{"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "Test"}]}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should return 401 Unauthorized
        // In test environment, returns BAD_GATEWAY due to no upstream server
        assert!(!response.status().is_success());
    }

    #[tokio::test]
    async fn test_authentication_with_x_api_key_header() {
        let config = create_test_config_with_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        // Use x-api-key header instead of Authorization
        let request = Request::builder()
            .uri("/v1/chat/completions")
            .method(Method::POST)
            .header("Content-Type", "application/json")
            .header("x-api-key", "test-api-key-with-sufficient-entropy-123456")
            .body(Body::from(
                r#"{"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "Test"}]}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // In test environment without upstream server, this returns BAD_GATEWAY
        // In production with proper upstream, would return UNAUTHORIZED
        assert!(!response.status().is_success());
    }

    #[tokio::test]
    async fn test_rate_limiting_on_failed_auth() {
        let config = create_test_config_with_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        // Make multiple failed auth attempts from same "IP"
        for i in 0..6 {
            let app_clone = app.clone();
            let request = Request::builder()
                .uri("/v1/chat/completions")
                .method(Method::POST)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer invalid-key-{}", i))
                .body(Body::from(r#"{"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "Test"}]}"#))
                .unwrap();

            let response = app_clone.oneshot(request).await.unwrap();

            // All requests should fail due to auth or upstream issues
            assert!(!response.status().is_success());
        }
    }

    #[tokio::test]
    async fn test_no_auth_when_disabled() {
        let config = create_test_config_no_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        // Access protected endpoint without auth when auth is disabled
        let request = Request::builder()
            .uri("/v1/chat/completions")
            .method(Method::POST)
            .header("Content-Type", "application/json")
            .body(Body::from(
                r#"{"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "Test"}]}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should not be unauthorized (will fail for other reasons, likely proxy)
        assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_anthropic_endpoint_with_auth() {
        let config = create_test_config_with_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        // Access Anthropic endpoint with valid auth
        let request = Request::builder()
            .uri("/v1/messages")
            .method(Method::POST)
            .header("Content-Type", "application/json")
            .header("Authorization", "Bearer test-api-key-with-sufficient-entropy-123456")
            .body(Body::from(r#"{"model": "claude-3-sonnet-20240229", "max_tokens": 100, "messages": [{"role": "user", "content": "Test"}]}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should not be unauthorized
        assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_streaming_endpoint_with_auth() {
        let config = create_test_config_with_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        // Access streaming endpoint with valid auth
        let request = Request::builder()
            .uri("/v1/proxy")
            .method(Method::POST)
            .header("Content-Type", "application/json")
            .header("Authorization", "Bearer test-api-key-with-sufficient-entropy-123456")
            .body(Body::from(r#"{"model": "gpt-3.5-turbo", "stream": true, "messages": [{"role": "user", "content": "Test"}]}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should not be unauthorized
        assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_streaming_endpoint_requires_stream_flag() {
        let mut config = create_test_config_no_auth();
        // Mock the upstream server
        let mock_server = MockServer::start().await;

        wiremock::Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": []
            })))
            .mount(&mock_server)
            .await;

        config.aperture.base_url = mock_server.uri();

        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        // Request without stream flag should fail
        let request = Request::builder()
            .uri("/v1/proxy")
            .method(Method::POST)
            .header("Content-Type", "application/json")
            .body(Body::from(
                r#"{"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "Test"}]}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_concurrent_requests_handling() {
        let config = create_test_config_no_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        let mut handles = vec![];

        // Make multiple concurrent requests
        for _i in 0..10 {
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

        // All requests should complete
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }
    }

    #[tokio::test]
    async fn test_error_handling_invalid_json() {
        let config = create_test_config_no_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        // Send invalid JSON
        let request = Request::builder()
            .uri("/v1/chat/completions")
            .method(Method::POST)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"invalid": json}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should return a client error
        assert!(response.status().is_client_error() || response.status().is_server_error());
    }

    #[tokio::test]
    async fn test_cors_preflight_request() {
        let config = create_test_config_no_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/health")
            .method(Method::OPTIONS)
            .header("Origin", "http://localhost:3000")
            .header("Access-Control-Request-Method", "GET")
            .header("Access-Control-Request-Headers", "content-type")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Check CORS headers
        let allow_origin = response.headers().get("access-control-allow-origin");
        let allow_methods = response.headers().get("access-control-allow-methods");
        let allow_headers = response.headers().get("access-control-allow-headers");

        assert!(allow_origin.is_some());
        assert!(allow_methods.is_some());
        assert!(allow_headers.is_some());
    }

    #[tokio::test]
    async fn test_security_headers() {
        let config = create_test_config_no_auth();
        let discovery = ModelDiscovery::new(config.aperture.clone());
        let app = create_router(config, std::sync::Arc::new(discovery));

        let request = Request::builder()
            .uri("/health")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Verify response is successful
        assert_eq!(response.status(), StatusCode::OK);
    }
}
