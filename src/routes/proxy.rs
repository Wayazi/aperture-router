// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{body::Body, extract::State, Json};
use http::{StatusCode, response::Response};
use reqwest::Response as ReqwestResponse;
use serde::Serialize;
use tracing::{debug, error, info};

use crate::proxy::client::ProxyClient;
use crate::config::Provider;
use crate::ProviderRegistry;

/// Maximum response size (10MB) to prevent DoS
const MAX_RESPONSE_SIZE: usize = 10 * 1024 * 1024;

/// Trait for requests that have a model field
pub trait HasModel {
    fn model(&self) -> &str;
}

/// Build a JSON error response
fn json_error(status: StatusCode, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"error": message}).to_string(),
        ))
        .expect("failed to build error response")
}

/// Build a JSON response with the given status and body string
fn json_response(status: StatusCode, body: impl Into<String>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.into()))
        .expect("failed to build json response")
}

/// Process a successful upstream response: read body, check size, return
async fn process_upstream_response(response: ReqwestResponse) -> Response<Body> {
    let status = response.status();

    let response_body = match response.text().await {
        Ok(body) => body,
        Err(e) => {
            error!("Failed to read response body: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to read response");
        }
    };

    if response_body.len() > MAX_RESPONSE_SIZE {
        error!("Response too large: {} bytes", response_body.len());
        return json_error(StatusCode::BAD_GATEWAY, "Response too large");
    }

    json_response(status, response_body)
}

/// Build endpoint URL based on provider configuration
fn build_provider_url(provider: &Provider, default_endpoint: &str) -> String {
    ProviderRegistry::build_endpoint_url(provider, default_endpoint)
}

/// Get the API key for a provider
fn get_provider_api_key(provider: &Provider, default_key: Option<&String>) -> Option<String> {
    provider.api_key.clone().or_else(|| default_key.cloned())
}

/// Serialize a request, returning the body bytes or an error response
#[allow(clippy::result_large_err)]
fn serialize_request<T: Serialize>(request: &T) -> Result<Vec<u8>, Response<Body>> {
    serde_json::to_vec(request).map_err(|e| {
        error!("Failed to serialize request: {}", e);
        json_error(StatusCode::BAD_REQUEST, "Invalid request format")
    })
}

/// Proxy request to default Aperture gateway (used when multi-provider is disabled)
async fn proxy_to_default_gateway<T>(
    proxy_client: ProxyClient,
    request: T,
    endpoint: &str,
) -> Response<Body>
where
    T: HasModel + Serialize,
{
    debug!(
        "Proxying request to default gateway {} with model: {:?}",
        endpoint,
        request.model()
    );

    let body = match serialize_request(&request) {
        Ok(b) => b,
        Err(r) => return r,
    };

    match proxy_client.forward_request(endpoint, body).await {
        Ok(response) => process_upstream_response(response).await,
        Err(e) => {
            error!("Proxy error: {}", e);
            json_error(StatusCode::BAD_GATEWAY, "Failed to forward request")
        }
    }
}

/// Generic proxy handler for JSON requests with multi-provider support
pub async fn proxy_handler_multi<T>(
    proxy_client: ProxyClient,
    provider_registry: ProviderRegistry,
    request: T,
    default_endpoint: &str,
    multi_provider_enabled: bool,
) -> Response<Body>
where
    T: HasModel + Serialize,
{
    let model = request.model();
    debug!("Proxying request for model: {}", model);

    // If multi-provider is disabled, skip provider lookup and use default gateway
    if !multi_provider_enabled {
        debug!("Multi-provider disabled, using default Aperture gateway");
        return proxy_to_default_gateway(proxy_client, request, default_endpoint).await;
    }

    // Check if we have a provider for this model
    let provider = provider_registry.get_provider_for_model(model);

    match provider {
        Some(provider) => {
            info!(
                "Routing model '{}' to provider '{}' ({})",
                model, provider.name, provider.base_url
            );

            let url = build_provider_url(provider, default_endpoint);
            debug!("Built URL: {}", url);

            let api_key = get_provider_api_key(provider, proxy_client.api_key());
            let body = match serialize_request(&request) {
                Ok(b) => b,
                Err(r) => return r,
            };

            match proxy_client.forward_request_to_url(&url, body, api_key.as_deref()).await {
                Ok(response) => process_upstream_response(response).await,
                Err(e) => {
                    error!("Proxy error for provider '{}': {}", provider.name, e);
                    json_error(
                        StatusCode::BAD_GATEWAY,
                        &format!("Failed to forward request to provider '{}'", provider.name),
                    )
                }
            }
        }
        None => {
            debug!("No provider found for model '{}', using default gateway", model);
            proxy_to_default_gateway(proxy_client, request, default_endpoint).await
        }
    }
}

/// Generic proxy handler for JSON requests (legacy, single provider)
pub async fn proxy_handler<T>(
    State(proxy_client): State<ProxyClient>,
    Json(request): Json<T>,
    endpoint: &str,
) -> Response<Body>
where
    T: HasModel + Serialize,
{
    debug!(
        "Proxying request to {} with model: {:?}",
        endpoint,
        request.model()
    );

    let body = match serialize_request(&request) {
        Ok(b) => b,
        Err(r) => return r,
    };

    match proxy_client.forward_request(endpoint, body).await {
        Ok(response) => process_upstream_response(response).await,
        Err(e) => {
            error!("Proxy error: {}", e);
            json_error(StatusCode::BAD_GATEWAY, "Failed to forward request")
        }
    }
}
