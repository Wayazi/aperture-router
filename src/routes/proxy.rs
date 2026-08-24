use axum::{body::Body, extract::State, Json};
use http::{response::Response, StatusCode};
use reqwest::Response as ReqwestResponse;
use serde::Serialize;
use tracing::{debug, error, info, warn};

use crate::config::Provider;
use crate::proxy::client::ProxyClient;
use crate::ProviderRegistry;

use super::shared::{provider_api_key, MAX_FAILOVER_ATTEMPTS};

use crate::http_client::MAX_NON_STREAMING_RESPONSE_BYTES as MAX_RESPONSE_SIZE;

pub trait HasModel {
    fn model(&self) -> &str;
}

/// A response builder can only fail on invalid status/header values, which
/// these helpers never produce — but instead of panicking on request paths,
/// fall back to a builder-free response that cannot fail.
fn fallback_response() -> Response<Body> {
    let mut resp = Response::new(Body::empty());
    *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    resp
}

fn json_error(status: StatusCode, message: &str) -> Response<Body> {
    match Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"error": message}).to_string(),
        )) {
        Ok(resp) => resp,
        Err(e) => {
            error!("Response build failed ({}), returning bare 500", e);
            fallback_response()
        }
    }
}

fn json_response(status: StatusCode, body: impl Into<String>) -> Response<Body> {
    match Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.into()))
    {
        Ok(resp) => resp,
        Err(e) => {
            error!("Response build failed ({}), returning bare 500", e);
            fallback_response()
        }
    }
}

async fn process_upstream_response(response: ReqwestResponse) -> Response<Body> {
    let status = response.status();

    let response_body = match crate::http_client::read_body_capped(
        response,
        MAX_RESPONSE_SIZE,
        "Upstream response",
    )
    .await
    {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => {
            error!("Failed to read response body: {}", e);
            return json_error(StatusCode::BAD_GATEWAY, "Response too large");
        }
    };

    json_response(status, response_body)
}

fn build_provider_url(provider: &Provider, default_endpoint: &str) -> String {
    ProviderRegistry::build_endpoint_url(provider, default_endpoint)
}

#[allow(clippy::result_large_err)]
fn serialize_request<T: Serialize>(request: &T) -> Result<Vec<u8>, Response<Body>> {
    serde_json::to_vec(request).map_err(|e| {
        error!("Failed to serialize request: {}", e);
        json_error(StatusCode::BAD_REQUEST, "Invalid request format")
    })
}

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

async fn try_provider(
    proxy_client: &ProxyClient,
    provider: &Provider,
    body: &[u8],
    default_endpoint: &str,
) -> Result<Response<Body>, StatusCode> {
    let url = build_provider_url(provider, default_endpoint);
    debug!("Built URL: {}", url);

    let api_key = provider_api_key(provider, proxy_client.api_key(), proxy_client.base_url())
        .map(str::to_string);

    match proxy_client
        .forward_request_to_url_raw(
            &url,
            body.to_vec(),
            api_key.as_deref(),
            provider.endpoint_style,
        )
        .await
    {
        Ok(response) => {
            let status = response.status();
            if status.is_server_error() {
                warn!(
                    "Provider '{}' returned {}, will try next",
                    provider.name, status
                );
                Err(status)
            } else {
                Ok(process_upstream_response(response).await)
            }
        }
        Err(e) => {
            warn!("Provider '{}' connection error: {}", provider.name, e);
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

pub async fn proxy_handler_multi<T>(
    proxy_client: ProxyClient,
    providers: Vec<Provider>,
    request: T,
    default_endpoint: &str,
    multi_provider_enabled: bool,
    _registry: &ProviderRegistry,
) -> Response<Body>
where
    T: HasModel + Serialize,
{
    let model = request.model();
    debug!("Proxying request for model: {}", model);

    if !multi_provider_enabled {
        debug!("Multi-provider disabled, using default Aperture gateway");
        return proxy_to_default_gateway(proxy_client, request, default_endpoint).await;
    }

    if providers.is_empty() {
        debug!(
            "No provider found for model '{}', using default gateway",
            model
        );
        return proxy_to_default_gateway(proxy_client, request, default_endpoint).await;
    }

    let body = match serialize_request(&request) {
        Ok(b) => b,
        Err(r) => return r,
    };

    if providers.len() == 1 {
        let provider = &providers[0];
        info!(
            "Routing model '{}' to provider '{}' ({})",
            model, provider.name, provider.base_url
        );
        return match try_provider(&proxy_client, provider, &body, default_endpoint).await {
            Ok(response) => response,
            Err(_) => json_error(StatusCode::BAD_GATEWAY, "Failed to forward request"),
        };
    }

    info!(
        "Routing model '{}' across {} providers (failover enabled)",
        model,
        providers.len()
    );

    let mut last_error = StatusCode::BAD_GATEWAY;
    for (i, provider) in providers.iter().take(MAX_FAILOVER_ATTEMPTS).enumerate() {
        debug!(
            "Trying provider {}/{}: '{}' ({})",
            i + 1,
            MAX_FAILOVER_ATTEMPTS,
            provider.name,
            provider.base_url
        );

        match try_provider(&proxy_client, provider, &body, default_endpoint).await {
            Ok(response) => {
                if i > 0 {
                    info!(
                        "Succeeded on provider '{}' (attempt {}/{})",
                        provider.name,
                        i + 1,
                        MAX_FAILOVER_ATTEMPTS
                    );
                }
                return response;
            }
            Err(status) => {
                last_error = status;
            }
        }
    }

    error!(
        "All {} providers failed for model '{}'",
        providers.len(),
        model
    );
    json_error(last_error, "All providers failed")
}

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
