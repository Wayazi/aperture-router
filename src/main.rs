// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use clap::Parser;
use std::sync::Arc;
use tracing::info;

use aperture_router::{config::Config, discovery::models::ModelDiscovery, server};

#[derive(Parser, Debug)]
#[command(name = "aperture-router")]
#[command(about = "Universal AI router for Tailscale Aperture", long_about = None)]
#[command(version)]
struct Args {
    /// Config file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Enable debug mode
    #[arg(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Load .env file if it exists
    dotenvy::dotenv().ok();

    // Initialize tracing (fixed: no memory leak)
    let log_filter = if args.debug {
        "aperture_router=debug,tower_http=debug,axum=debug".to_string()
    } else {
        std::env::var("RUST_LOG").unwrap_or_else(|_| "aperture_router=info".to_string())
    };

    tracing_subscriber::fmt()
        .with_env_filter(&log_filter)
        .init();

    info!("Starting Aperture Router v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = Config::load(&args.config)?;
    info!("Loaded configuration from {}", args.config);
    info!("Aperture gateway: {}", config.aperture.base_url);
    info!("Server address: {}", config.server_addr()?);

    // Check authentication status
    if config.security.require_auth_in_prod && config.security.api_keys.is_empty() {
        if !cfg!(debug_assertions) {
            return Err(anyhow::anyhow!("Production mode requires authentication but no API keys configured. Set APERTURE_ALLOW_NO_AUTH=1 to override (not recommended)"));
        }
        info!("⚠️  WARNING: Running without authentication in production mode!");
    }

    // Initialize and fetch models
    let mut discovery = ModelDiscovery::new(config.aperture.clone());
    info!("Fetching models from Aperture...");
    let models = discovery.fetch_models().await?;
    info!("Discovered {} models", models.len());
    info!("Available models:");
    for model in &models {
        info!("  - {}: {}", model.id, model.object);
    }

    // Create router
    let app = server::create_router(
        config.clone(),
        Arc::new(tokio::sync::RwLock::new(discovery)),
    );

    // Start server with graceful shutdown
    let addr = config.server_addr()?;
    info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down gracefully...");
        },
        _ = terminate => {
            info!("Received TERM signal, shutting down gracefully...");
        },
    }
}
