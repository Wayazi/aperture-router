// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
    routing::{get, post},
    Router,
};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use tower_http::{
    cors::CorsLayer, limit::RequestBodyLimitLayer, set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{
    config::Config, discovery::models::ModelDiscovery, middleware::AuthState,
    proxy::client::ProxyClient, ProviderRegistry,
};

/// Header name for session ID (client can provide to group requests)
const SESSION_ID_HEADER: &str = "x-session-id";

/// Middleware to add request ID and session ID for tracing
///
/// Session ID allows grouping multiple requests from the same client session.
/// Clients can send `X-Session-ID` header to maintain session continuity.
/// If not provided, a new session ID is generated and returned in response.
async fn add_request_id(request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4();

    // Get or generate session ID
    // Client can provide X-Session-ID header to maintain session across requests
    let session_id = if let Some(header_value) = request.headers().get(SESSION_ID_HEADER) {
        match header_value.to_str() {
            Ok(s) => match Uuid::parse_str(s) {
                Ok(uuid) => uuid,
                Err(e) => {
                    debug!(
                        "Invalid session ID format provided: {}, generating new one",
                        e
                    );
                    Uuid::new_v4()
                }
            },
            Err(_) => {
                debug!("Session ID header contains non-UTF8 characters, generating new one");
                Uuid::new_v4()
            }
        }
    } else {
        Uuid::new_v4()
    };

    // Add both IDs to tracing span for log grouping
    let span = tracing::info_span!(
        "request",
        request_id = %request_id,
        session_id = %session_id,
        method = %request.method(),
        path = %request.uri().path(),
    );

    // Log request start with session context
    info!(parent: &span, "Request started");

    // Run the request in the span
    let mut response = next.run(request).await;

    // Add session ID to response headers so client can reuse it
    match axum::http::HeaderValue::from_str(&session_id.to_string()) {
        Ok(header_value) => {
            response.headers_mut().insert(
                axum::http::HeaderName::from_static(SESSION_ID_HEADER),
                header_value,
            );
        }
        Err(e) => {
            warn!("Failed to set session ID response header: {}", e);
        }
    }

    // Log request completion
    info!(
        parent: &span,
        status = %response.status(),
        "Request completed"
    );

    response
}

/// Application state shared across all routes
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub auth_state: AuthState,
    pub proxy_client: ProxyClient,
    pub discovery: Arc<ModelDiscovery>,
    pub provider_registry: Arc<ProviderRegistry>,
    pub cleanup_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub refresh_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub shutdown_token: CancellationToken,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<Config>,
        auth_state: AuthState,
        proxy_client: ProxyClient,
        discovery: Arc<ModelDiscovery>,
        provider_registry: Arc<ProviderRegistry>,
        cleanup_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
        refresh_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
        shutdown_token: CancellationToken,
    ) -> Self {
        Self {
            config,
            auth_state,
            proxy_client,
            discovery,
            provider_registry,
            cleanup_handle,
            refresh_handle,
            shutdown_token,
        }
    }
}

fn create_cors_layer(config: &crate::config::CorsConfig) -> CorsLayer {
    let headers = [
        axum::http::header::CONTENT_TYPE,
        axum::http::header::AUTHORIZATION,
        axum::http::header::ACCEPT,
        axum::http::HeaderName::from_static("x-api-key"),
        axum::http::HeaderName::from_static("x-session-id"),
    ];

    let methods = [
        axum::http::Method::GET,
        axum::http::Method::POST,
        axum::http::Method::OPTIONS,
    ];

    if config.allowed_origins.is_empty() {
        let origins = [
            "http://localhost:3000"
                .parse()
                .expect("Invalid localhost CORS origin"),
            "http://127.0.0.1:3000"
                .parse()
                .expect("Invalid 127.0.0.1 CORS origin"),
        ];

        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(methods)
            .allow_headers(headers)
            .allow_credentials(true)
    } else {
        let origins: Result<Vec<axum::http::HeaderValue>, _> = config
            .allowed_origins
            .iter()
            .map(|origin| origin.parse())
            .collect();

        match origins {
            Ok(origin_headers) => CorsLayer::new()
                .allow_origin(origin_headers)
                .allow_methods(methods)
                .allow_headers(headers)
                .allow_credentials(true),
            Err(e) => {
                tracing::warn!("Invalid CORS origin configuration: {}, using defaults", e);
                let fallback_origins = [
                    "http://localhost:3000"
                        .parse()
                        .expect("Invalid localhost CORS origin"),
                    "http://127.0.0.1:3000"
                        .parse()
                        .expect("Invalid 127.0.0.1 CORS origin"),
                ];

                CorsLayer::new()
                    .allow_origin(fallback_origins)
                    .allow_methods(methods)
                    .allow_headers(headers)
                    .allow_credentials(true)
            }
        }
    }
}

/// Create the router with all routes and middleware
/// Returns (Router, CancellationToken) for graceful shutdown
pub fn create_router(
    config: Config,
    discovery: Arc<ModelDiscovery>,
) -> (Router, CancellationToken) {
    info!("Creating router with authentication and CORS layers");

    // Create provider registry with Aperture URL for auto-discovery
    let provider_registry = Arc::new(ProviderRegistry::with_aperture_url(
        config.providers.clone(),
        config.aperture.base_url.clone(),
    ));

    // Create proxy client
    let proxy_client = ProxyClient::new(
        config.aperture.clone(),
        config.http.clone(),
        config.security.max_streaming_size_bytes,
    )
    .expect("Failed to create proxy client");

    // Create authentication state
    let auth_state = AuthState::new(&config.security, &config.cors);

    // Create shutdown token for graceful termination
    let shutdown_token = CancellationToken::new();

    // Start cleanup task for rate limiting
    let cleanup_handle = Arc::new(Mutex::new(Some(auth_state.start_cleanup_task())));

    // Start model refresh task with registry sync and shutdown support
    let refresh_handle = Arc::new(Mutex::new(Some(
        Arc::clone(&discovery)
            .start_refresh_task(Some(Arc::clone(&provider_registry)), shutdown_token.clone()),
    )));

    // Setup CORS
    let cors = create_cors_layer(&config.cors);

    // Create shared config and auth state (single instance each)
    let shared_config = Arc::new(config.clone());
    let shared_auth_state = Arc::new(auth_state.clone());

    // Create a single AppState wrapped in Arc (reduces clones)
    let app_state = Arc::new(AppState::new(
        shared_config.clone(),
        auth_state.clone(),
        proxy_client.clone(),
        discovery.clone(),
        provider_registry.clone(),
        cleanup_handle.clone(),
        refresh_handle.clone(),
        shutdown_token.clone(),
    ));

    // Admin routes - uses same state via Arc clone (cheap reference increment)
    let admin_routes = Router::new()
        .route(
            "/admin/refresh-models",
            post(crate::routes::admin::refresh_models),
        )
        .route("/admin/stats", get(crate::routes::admin::get_stats))
        .route_layer(axum::middleware::from_fn_with_state(
            (Arc::clone(&shared_config), Arc::clone(&shared_auth_state)),
            crate::middleware::admin_auth_middleware,
        ))
        .with_state((*app_state).clone());

    // Regular API routes - uses same state via Arc clone
    let protected_routes = Router::new()
        .route("/v1/models", get(crate::routes::models::models))
        .route(
            "/v1/proxy",
            post(crate::routes::streaming::handle_proxy_stream),
        )
        .route(
            "/v1/chat/completions",
            post(crate::routes::chat::chat_completions),
        )
        .route(
            "/v1/messages",
            post(crate::routes::messages::anthropic_messages),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            (Arc::clone(&shared_config), Arc::clone(&shared_auth_state)),
            crate::middleware::auth_middleware,
        ))
        .with_state((*app_state).clone());

    let router = Router::new()
        .route("/health", get(crate::routes::health::health))
        .merge(admin_routes)
        .merge(protected_routes)
        .layer(axum::middleware::from_fn(add_request_id))
        .layer(TraceLayer::new_for_http())
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            axum::http::HeaderValue::from_static("default-src 'self'"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("x-frame-options"),
            axum::http::HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("x-content-type-options"),
            axum::http::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("x-xss-protection"),
            axum::http::HeaderValue::from_static("1; mode=block"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("strict-transport-security"),
            axum::http::HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ))
        .layer(RequestBodyLimitLayer::new(
            (config.security.max_body_size_bytes as u64)
                .try_into()
                .expect("Body size limit exceeds usize max"),
        ))
        .layer(cors);

    (router, shutdown_token)
}
