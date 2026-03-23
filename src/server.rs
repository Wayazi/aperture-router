// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::{Arc, Mutex};
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, limit::RequestBodyLimitLayer,
    set_header::SetResponseHeaderLayer, trace::TraceLayer,
};
use tracing::info;

use crate::{
    config::Config, discovery::models::ModelDiscovery, middleware::AuthState,
    proxy::client::ProxyClient,
};

pub type AppState = (
    Arc<Config>,
    AuthState,
    ProxyClient,
    Arc<ModelDiscovery>,
    Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
);

pub fn create_router(config: Config, discovery: Arc<ModelDiscovery>) -> Router {
    info!("Creating router with authentication and CORS layers");

    // Create proxy client
    let proxy_client = ProxyClient::new(
        config.aperture.clone(),
        config.http.clone(),
        config.security.max_streaming_size_bytes,
    )
    .expect("Failed to create proxy client");

    // Create authentication state
    let auth_state = AuthState::new(&config.security, &config.cors);

    // Start cleanup task for rate limiting and store the handle
    let cleanup_handle = Arc::new(Mutex::new(Some(auth_state.start_cleanup_task())));

    // Setup CORS with configurable origins
    let cors = if config.cors.allowed_origins.is_empty() {
        // Fallback to localhost defaults if no origins configured
        const LOCALHOST_3000: &str = "http://localhost:3000";
        const LOCALHOST_127_3000: &str = "http://127.0.0.1:3000";

        CorsLayer::new()
            .allow_origin([
                LOCALHOST_3000
                    .parse()
                    .expect("Invalid localhost CORS origin"),
                LOCALHOST_127_3000
                    .parse()
                    .expect("Invalid 127.0.0.1 CORS origin"),
            ])
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
                axum::http::header::ACCEPT,
                axum::http::HeaderName::from_static("x-api-key"),
            ])
            .allow_credentials(true)
    } else {
        // Use configured origins - convert Strings to HeaderValues
        let origins: Result<Vec<axum::http::HeaderValue>, _> = config
            .cors
            .allowed_origins
            .iter()
            .map(|origin| origin.parse())
            .collect();

        match origins {
            Ok(origin_headers) => CorsLayer::new()
                .allow_origin(origin_headers)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::ACCEPT,
                    axum::http::HeaderName::from_static("x-api-key"),
                ])
                .allow_credentials(true),
            Err(e) => {
                tracing::warn!("Invalid CORS origin configuration: {}, using defaults", e);
                // Fallback to defaults on parse error
                const LOCALHOST_3000: &str = "http://localhost:3000";
                const LOCALHOST_127_3000: &str = "http://127.0.0.1:3000";

                CorsLayer::new()
                    .allow_origin([
                        LOCALHOST_3000
                            .parse()
                            .expect("Invalid localhost CORS origin"),
                        LOCALHOST_127_3000
                            .parse()
                            .expect("Invalid 127.0.0.1 CORS origin"),
                    ])
                    .allow_methods([
                        axum::http::Method::GET,
                        axum::http::Method::POST,
                        axum::http::Method::OPTIONS,
                    ])
                    .allow_headers([
                        axum::http::header::CONTENT_TYPE,
                        axum::http::header::AUTHORIZATION,
                        axum::http::header::ACCEPT,
                        axum::http::HeaderName::from_static("x-api-key"),
                    ])
                    .allow_credentials(true)
            }
        }
    };

    // Create shared config and auth state for middleware
    let shared_config = Arc::new(config.clone());
    let shared_auth_state = Arc::new(auth_state.clone());
    let shared_auth_state_for_admin = Arc::new(auth_state.clone());

    // Build router with middleware
    // NOTE: Auth middleware is applied via route_layer to protect all routes below it
    // All routes except /health require authentication

    // Admin routes - protected by admin-specific auth (requires admin API key)
    let admin_routes = Router::new()
        .route(
            "/admin/refresh-models",
            post(crate::routes::admin::refresh_models),
        )
        .route("/admin/stats", get(crate::routes::admin::get_stats))
        .route_layer(axum::middleware::from_fn_with_state(
            (Arc::clone(&shared_config), shared_auth_state_for_admin),
            crate::middleware::admin_auth_middleware,
        ))
        .with_state((
            shared_config.clone(),
            auth_state.clone(),
            proxy_client.clone(),
            discovery.clone(),
            Arc::clone(&cleanup_handle),
        ));

    // Regular API routes - protected by regular auth
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
            (Arc::clone(&shared_config), shared_auth_state),
            crate::middleware::auth_middleware,
        ))
        .with_state((
            shared_config,
            auth_state,
            proxy_client,
            discovery,
            Arc::clone(&cleanup_handle),
        ));

    Router::new()
        .route("/health", get(crate::routes::health::health))
        .merge(admin_routes)
        .merge(protected_routes)
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            axum::http::HeaderValue::from_static("default-src 'self'"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("x-frame-options"),
            axum::http::HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("x-content-type-options"),
            axum::http::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("x-xss-protection"),
            axum::http::HeaderValue::from_static("1; mode=block"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("strict-transport-security"),
            axum::http::HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ))
        .layer(RequestBodyLimitLayer::new(
            (config.security.max_body_size_bytes as u64)
                .try_into()
                .expect("Body size limit exceeds usize max"),
        ))
        .layer(cors)
}
