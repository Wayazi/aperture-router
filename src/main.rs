// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use clap::{Parser, Subcommand};
use std::sync::Arc;
use tracing::info;

use aperture_router::{config::Config, discovery::models::ModelDiscovery, server};

#[derive(Parser, Debug)]
#[command(name = "aperture-router")]
#[command(about = "Universal AI router for Tailscale Aperture", long_about = None)]
#[command(version)]
struct Cli {
    /// Config file path
    #[arg(short, long, global = true, default_value = "config.toml")]
    config: String,

    /// Enable debug mode
    #[arg(short, long, global = true)]
    debug: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the router server (default)
    Run,

    /// Configuration management
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    /// Interactive configuration wizard
    Wizard {
        /// Aperture gateway URL
        #[arg(short, long)]
        url: Option<String>,

        /// Output config file path
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Fetch models from Aperture and display them
    Fetch {
        /// Aperture gateway URL
        #[arg(short, long)]
        url: String,
    },

    /// List current configuration
    List,

    /// Enable a provider
    Enable {
        /// Provider name
        name: String,
    },

    /// Disable a provider
    Disable {
        /// Provider name
        name: String,
    },

    /// Export configuration
    Export {
        /// Export as TOML format (default if no format specified)
        #[arg(long)]
        toml: bool,

        /// Export as OpenCode.json format
        #[arg(long)]
        opencode: bool,

        /// Output file path
        #[arg(short, long)]
        output: Option<String>,

        /// Router URL for OpenCode export
        #[arg(long, default_value = "http://127.0.0.1:8765")]
        router_url: String,
    },

    /// Validate configuration
    Validate,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load .env file if it exists
    dotenvy::dotenv().ok();

    // Initialize tracing
    let log_filter = if cli.debug {
        "aperture_router=debug,tower_http=debug,axum=debug".to_string()
    } else {
        std::env::var("RUST_LOG").unwrap_or_else(|_| "aperture_router=info".to_string())
    };

    tracing_subscriber::fmt()
        .with_env_filter(&log_filter)
        .init();

    match cli.command {
        None | Some(Commands::Run) => {
            run_server(&cli.config).await?;
        }
        Some(Commands::Config { command }) => {
            handle_config_command(command, &cli.config).await?;
        }
    }

    Ok(())
}

async fn run_server(config_path: &str) -> anyhow::Result<()> {
    info!("Starting Aperture Router v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = Config::load(config_path)?;
    info!("Loaded configuration from {}", config_path);
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
    let discovery = Arc::new(ModelDiscovery::new(config.aperture.clone())?);
    info!("Fetching models from Aperture...");
    let snapshot = discovery.fetch_models().await?;
    info!("Discovered {} models from {} providers", snapshot.models.len(), snapshot.providers.len());

    for provider in &snapshot.providers {
        let models = discovery.get_models_for_provider(provider).await;
        info!("  Provider '{}': {} models", provider, models.len());
    }

    // Create router (with auto-refresh background task)
    let (app, shutdown_token) = server::create_router(config.clone(), Arc::clone(&discovery));

    // Start server with graceful shutdown
    let addr = config.server_addr()?;
    info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown_token))
        .await?;

    Ok(())
}

async fn handle_config_command(cmd: ConfigCommands, config_path: &str) -> anyhow::Result<()> {
    use aperture_router::cli::commands;

    match cmd {
        ConfigCommands::Wizard { url, output } => {
            commands::run_wizard(config_path, url, output).await?;
        }
        ConfigCommands::Fetch { url } => {
            commands::fetch_models_cmd(&url).await?;
        }
        ConfigCommands::List => {
            commands::list_config(config_path)?;
        }
        ConfigCommands::Enable { name } => {
            commands::toggle_provider(config_path, &name, true)?;
        }
        ConfigCommands::Disable { name } => {
            commands::toggle_provider(config_path, &name, false)?;
        }
        ConfigCommands::Export {
            toml,
            opencode,
            output,
            router_url,
        } => {
            commands::export_config(config_path, toml, opencode, output, &router_url).await?;
        }
        ConfigCommands::Validate => {
            commands::validate_config(config_path)?;
        }
    }

    Ok(())
}

async fn shutdown_signal(shutdown_token: tokio_util::sync::CancellationToken) {
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
            shutdown_token.cancel();
        },
        _ = terminate => {
            info!("Received TERM signal, shutting down gracefully...");
            shutdown_token.cancel();
        },
    }
}
