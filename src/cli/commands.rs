// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! CLI command handlers
//!
//! Implements handlers for all config subcommands:
//! - wizard: Interactive configuration
//! - fetch: Fetch models from Aperture
//! - list: Show current config
//! - enable/disable: Toggle providers
//! - export: Export config files
//! - validate: Validate config

use super::model_fetcher::{fetch_models, group_by_provider};
use super::opencode_export::OpenCodeConfig;
use super::security::safe_config_summary;
use super::wizard::ConfigWizard;
use crate::config::Config;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Run the interactive configuration wizard
pub async fn run_wizard(
    config_path: &str,
    aperture_url: Option<String>,
    output_path: Option<String>,
) -> anyhow::Result<()> {
    let wizard = ConfigWizard::new(aperture_url, "http://127.0.0.1:8765".to_string());
    let result = wizard.run().await?;

    // Save config
    let save_path = output_path.as_deref().unwrap_or(config_path);
    result.config.save(save_path)?;

    // Save OpenCode config if generated - MERGE with existing
    if let Some(opencode) = result.opencode_config {
        let opencode_path = dirs::config_dir()
            .map(|p| p.join("opencode").join("opencode.json"))
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;

        // Read existing config and merge
        let merged_json = if opencode_path.exists() {
            let existing = std::fs::read_to_string(&opencode_path)?;
            opencode.merge_with_existing(&existing)?
        } else {
            opencode.to_json()?
        };

        std::fs::write(&opencode_path, merged_json)?;

        // Set restrictive permissions for security (API keys in config)
        #[cfg(unix)]
        std::fs::set_permissions(&opencode_path, std::fs::Permissions::from_mode(0o600))?;

        println!("✓ OpenCode config saved to {:?} (preserved existing settings)", opencode_path);
    }

    println!("\n🎉 Configuration complete!");
    println!("   Run 'aperture-router' to start the server.");

    Ok(())
}

/// Fetch and display models from Aperture
pub async fn fetch_models_cmd(url: &str) -> anyhow::Result<()> {
    println!("Fetching models from {}...", url);

    let models = fetch_models(url).await?;
    let grouped = group_by_provider(&models);

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║               AVAILABLE MODELS FROM APERTURE                 ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    for (provider, provider_models) in &grouped {
        let dash_count = 50 - provider.len().max(1);
        println!("┌─ Provider: {} {}─", provider, "─".repeat(dash_count));
        println!("│");

        for model in provider_models {
            println!("│  {} → {}", model.id, model.canonical_id);
            println!("│");
        }
    }

    println!("└{}", "─".repeat(62));
    println!("\nTotal: {} models from {} providers", models.len(), grouped.len());

    Ok(())
}

/// List current configuration
pub fn list_config(config_path: &str) -> anyhow::Result<()> {
    let config = Config::load(config_path)?;

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║               CURRENT CONFIGURATION                          ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("Server: {}:{}", config.host, config.port);
    println!("Aperture: {}", config.aperture.base_url);
    println!("Multi-provider: {}", config.multi_provider_enabled);
    println!();

    if config.providers.is_empty() {
        println!("No providers configured in config.toml.");
        println!("Note: Providers are auto-discovered from Aperture at runtime.");
    } else {
        println!("Providers (from config):");
        for provider in &config.providers {
            let status = if provider.enabled { "enabled" } else { "disabled" };
            println!("  {} [{}]", provider.name, status);
            println!("    URL: {}", provider.base_url);
            println!("    Style: {:?}", provider.endpoint_style);
            println!("    Models: {}", provider.models.join(", "));
            println!("    API Key: {}", if provider.api_key.is_some() { "configured" } else { "none" });
            println!();
        }
    }

    Ok(())
}

/// Enable a provider
pub fn toggle_provider(config_path: &str, provider_name: &str, enable: bool) -> anyhow::Result<()> {
    let mut config = Config::load(config_path)?;

    let provider = config
        .providers
        .iter_mut()
        .find(|p| p.name == provider_name);

    match provider {
        Some(p) => {
            p.enabled = enable;
            config.save(config_path)?;
            println!(
                "✓ Provider '{}' {}",
                provider_name,
                if enable { "enabled" } else { "disabled" }
            );
        }
        None => {
            return Err(anyhow::anyhow!(
                "Provider '{}' not found. Available: {}",
                provider_name,
                config.providers.iter().map(|p| &p.name).cloned().collect::<Vec<_>>().join(", ")
            ));
        }
    }

    Ok(())
}

/// Export configuration
pub async fn export_config(
    config_path: &str,
    toml_format: bool,
    opencode_format: bool,
    output_path: Option<String>,
    router_url: &str,
) -> anyhow::Result<()> {
    let config = Config::load(config_path)?;

    if toml_format || !opencode_format {
        let path = output_path
            .clone()
            .unwrap_or_else(|| "config.toml".to_string());
        config.save(&path)?;
        println!("✓ Config exported to {}", path);
    }

    if opencode_format {
        // Fetch models to get metadata
        let models = fetch_models(&config.aperture.base_url).await?;

        let opencode = OpenCodeConfig::from_router_config(&config, &models, router_url);
        let json = opencode.to_json()?;

        let path = output_path
            .clone()
            .unwrap_or_else(|| "opencode.json".to_string());
        std::fs::write(&path, json)?;
        println!("✓ OpenCode config exported to {}", path);
    }

    Ok(())
}

/// Validate configuration
pub fn validate_config(config_path: &str) -> anyhow::Result<()> {
    let config = Config::load(config_path)?;

    match config.validate() {
        Ok(()) => {
            println!("✅ Configuration is valid");
            println!();
            println!("{}", safe_config_summary(&config));
        }
        Err(e) => {
            println!("❌ Configuration is invalid:");
            println!("   {}", e);
            return Err(anyhow::anyhow!("Configuration validation failed"));
        }
    }

    Ok(())
}
