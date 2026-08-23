// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! Helpers shared across integration test binaries.

use axum::{extract::ConnectInfo, http::Request};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use aperture_router::{config::Config, discovery::models::ModelDiscovery, server};

pub fn create_test_router(
    config: Config,
    discovery: std::sync::Arc<ModelDiscovery>,
) -> axum::Router {
    let server::RouterHandles { router, .. } = server::create_router(config, discovery);
    router
}

/// Add ConnectInfo extension to a request for testing.
/// Simulates what the server does with into_make_service_with_connect_info.
pub fn add_connect_info<B>(mut request: Request<B>) -> Request<B> {
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        12345,
    )));
    request
}

/// Add ConnectInfo with a specific port (distinct per-IP buckets for
/// rate-limit tests that share 127.0.0.1).
#[allow(dead_code)]
pub fn add_connect_info_port<B>(mut request: Request<B>, port: u16) -> Request<B> {
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        port,
    )));
    request
}
