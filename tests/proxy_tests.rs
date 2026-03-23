// SPDX-License-Identifier: MIT
// Copyright (c) 2025 aperture-router contributors

use aperture_router::config::{ApertureConfig, HttpConfig};
use aperture_router::proxy::client::ProxyClient;
use http::StatusCode;
use wiremock::matchers::{method, path};
use wiremock::{MockServer, ResponseTemplate};

#[cfg(test)]
mod proxy_tests {
    use super::*;

    #[tokio::test]
    async fn test_proxy_client_creation() {
        let aperture_config = ApertureConfig {
            base_url: "http://localhost:8080".to_string(),
            api_key: Some("test-api-key".to_string()),
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };

        let client = ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024);
        assert!(client.is_ok(), "ProxyClient should be created successfully");

        let proxy_client = client.unwrap();
        assert_eq!(proxy_client.base_url(), "http://localhost:8080");
    }

    #[tokio::test]
    async fn test_proxy_client_creation_no_api_key() {
        let aperture_config = ApertureConfig {
            base_url: "http://localhost:8080".to_string(),
            api_key: None,
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };

        let client = ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_proxy_client_forward_request_success() {
        let mock_server = MockServer::start().await;

        // Mock the endpoint
        wiremock::Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "test-123",
                "object": "chat.completion",
                "created": 1234567890,
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello!"
                    },
                    "finish_reason": "stop"
                }]
            })))
            .mount(&mock_server)
            .await;

        let aperture_config = ApertureConfig {
            base_url: mock_server.uri(),
            api_key: None,
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };

        let proxy_client =
            ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024).unwrap();

        let request_body = serde_json::json!({
            "model": "gpt-3.5-turbo",
            "messages": [{
                "role": "user",
                "content": "Hello"
            }]
        });

        let result = proxy_client
            .forward_request(
                "v1/chat/completions",
                serde_json::to_vec(&request_body).unwrap(),
            )
            .await;

        assert!(result.is_ok(), "Request should succeed");

        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_proxy_client_forward_request_with_api_key() {
        let mock_server = MockServer::start().await;

        // Mock the endpoint that expects API key
        wiremock::Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(wiremock::matchers::header("x-api-key", "test-api-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "test-456",
                "object": "chat.completion",
                "choices": []
            })))
            .mount(&mock_server)
            .await;

        let aperture_config = ApertureConfig {
            base_url: mock_server.uri(),
            api_key: Some("test-api-key".to_string()),
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };

        let proxy_client =
            ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024).unwrap();

        let request_body = serde_json::json!({"model": "gpt-3.5-turbo"});

        let result = proxy_client
            .forward_request(
                "v1/chat/completions",
                serde_json::to_vec(&request_body).unwrap(),
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_proxy_client_forward_request_server_error() {
        let mock_server = MockServer::start().await;

        // Mock a server error response
        wiremock::Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "error": {
                    "message": "Internal server error",
                    "type": "internal_error"
                }
            })))
            .mount(&mock_server)
            .await;

        let aperture_config = ApertureConfig {
            base_url: mock_server.uri(),
            api_key: None,
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };

        let proxy_client =
            ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024).unwrap();

        let request_body = serde_json::json!({"model": "gpt-3.5-turbo"});

        let result = proxy_client
            .forward_request(
                "v1/chat/completions",
                serde_json::to_vec(&request_body).unwrap(),
            )
            .await;

        assert!(result.is_err(), "Server error should return error");
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Upstream service returned error"));
    }

    #[tokio::test]
    async fn test_proxy_client_forward_request_bad_gateway() {
        let mock_server = MockServer::start().await;

        // Mock a bad gateway error
        wiremock::Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(502).set_body_json(serde_json::json!({
                "error": "Bad Gateway"
            })))
            .mount(&mock_server)
            .await;

        let aperture_config = ApertureConfig {
            base_url: mock_server.uri(),
            api_key: None,
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };

        let proxy_client =
            ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024).unwrap();

        let request_body = serde_json::json!({"model": "gpt-3.5-turbo"});

        let result = proxy_client
            .forward_request(
                "v1/chat/completions",
                serde_json::to_vec(&request_body).unwrap(),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_proxy_client_base_url() {
        let aperture_config = ApertureConfig {
            base_url: "http://test.example.com:8080".to_string(),
            api_key: None,
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };

        let proxy_client =
            ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024).unwrap();

        assert_eq!(proxy_client.base_url(), "http://test.example.com:8080");
    }

    #[tokio::test]
    async fn test_proxy_client_custom_timeouts() {
        let aperture_config = ApertureConfig {
            base_url: "http://localhost:8080".to_string(),
            api_key: None,
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 5,
            request_timeout_secs: 120,
            sse_keep_alive_secs: 15,
        };

        let client = ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024);
        assert!(client.is_ok());

        let proxy_client = client.unwrap();
        assert_eq!(proxy_client.base_url(), "http://localhost:8080");
    }

    #[tokio::test]
    async fn test_proxy_client_forward_request_rate_limit() {
        let mock_server = MockServer::start().await;

        // Mock a rate limit response
        wiremock::Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "error": {
                    "message": "Rate limit exceeded",
                    "type": "rate_limit_error"
                }
            })))
            .mount(&mock_server)
            .await;

        let aperture_config = ApertureConfig {
            base_url: mock_server.uri(),
            api_key: None,
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };

        let proxy_client =
            ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024).unwrap();

        let request_body = serde_json::json!({"model": "gpt-3.5-turbo"});

        let result = proxy_client
            .forward_request(
                "v1/chat/completions",
                serde_json::to_vec(&request_body).unwrap(),
            )
            .await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Upstream service returned error"));
    }

    #[tokio::test]
    async fn test_proxy_client_invalid_endpoint() {
        let mock_server = MockServer::start().await;

        // Mock a 404 response
        wiremock::Mock::given(method("POST"))
            .and(path("/invalid/endpoint"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": "Not found"
            })))
            .mount(&mock_server)
            .await;

        let aperture_config = ApertureConfig {
            base_url: mock_server.uri(),
            api_key: None,
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 10,
            request_timeout_secs: 300,
            sse_keep_alive_secs: 15,
        };

        let proxy_client =
            ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024).unwrap();

        let request_body = serde_json::json!({"model": "gpt-3.5-turbo"});

        let result = proxy_client
            .forward_request(
                "invalid/endpoint",
                serde_json::to_vec(&request_body).unwrap(),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_proxy_client_connection_error() {
        // Use a non-existent URL
        let aperture_config = ApertureConfig {
            base_url: "http://localhost:9999".to_string(), // Port likely not in use
            api_key: None,
            model_refresh_interval_secs: 300,
        };

        let http_config = HttpConfig {
            connect_timeout_secs: 1, // Short timeout
            request_timeout_secs: 1,
            sse_keep_alive_secs: 15,
        };

        let proxy_client =
            ProxyClient::new(aperture_config, http_config, 100 * 1024 * 1024).unwrap();

        let request_body = serde_json::json!({"model": "gpt-3.5-turbo"});

        let result = proxy_client
            .forward_request(
                "v1/chat/completions",
                serde_json::to_vec(&request_body).unwrap(),
            )
            .await;

        // This should fail with a connection error
        assert!(result.is_err());
    }
}
