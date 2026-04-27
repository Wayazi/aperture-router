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
use super::openclaw_export::OpenClawConfig;
use super::opencode_export::OpenCodeConfig;
use super::security::safe_config_summary;
#[cfg(feature = "wizard")]
use super::wizard::ConfigWizard;
use crate::config::Config;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// Import shared constants and functions from parent module
use super::{is_running_elevated, SYSTEM_CONFIG_PATH};

/// Fix file ownership when saving to system path as root
/// When running under sudo, the file is created as root:root but the
/// aperture-router service runs as aperture-router user, so it can't read the config.
/// This function chowns the file to the aperture-router user using native syscalls.
#[cfg(unix)]
fn fix_system_config_ownership(path: &str) -> anyhow::Result<()> {
    use nix::unistd::{chown, Group, User};

    // Only fix ownership if:
    // 1. We're running elevated (sudo/root)
    // 2. The path is the system config path
    // 3. The aperture-router user exists
    if !is_running_elevated() || path != SYSTEM_CONFIG_PATH {
        return Ok(());
    }

    // Look up the aperture-router user and group using native syscalls
    let user = match User::from_name("aperture-router")? {
        Some(u) => u,
        None => {
            tracing::debug!("aperture-router user not found, skipping ownership fix");
            return Ok(());
        }
    };

    let group = match Group::from_name("aperture-router")? {
        Some(g) => g,
        None => {
            tracing::debug!("aperture-router group not found, skipping ownership fix");
            return Ok(());
        }
    };

    // Use native chown syscall - no PATH search, no shell execution
    match chown(std::path::Path::new(path), Some(user.uid), Some(group.gid)) {
        Ok(()) => {
            tracing::info!("Fixed ownership for system config: {}", path);
        }
        Err(e) => {
            tracing::warn!("Could not fix ownership for {}: {}", path, e);
            eprintln!(
                "Warning: Could not change ownership of {}. The service may not be able to read it.",
                path
            );
            eprintln!("Run: sudo chown aperture-router:aperture-router {}", path);
        }
    }

    Ok(())
}

#[cfg(not(unix))]
fn fix_system_config_ownership(_path: &str) -> anyhow::Result<()> {
    Ok(())
}

/// Run the interactive configuration wizard
#[cfg(feature = "wizard")]
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

    // Fix ownership if saving to system path
    fix_system_config_ownership(save_path)?;

    // Print helpful message about system config
    if save_path == SYSTEM_CONFIG_PATH {
        println!("✓ Config saved to {} (system service)", save_path);
        println!("  Restart service: sudo systemctl restart aperture-router");
    }

    // Save OpenCode config if generated - MERGE with existing
    if let Some(opencode) = result.opencode_config {
        let opencode_path = dirs::config_dir()
            .map(|p| p.join("opencode").join("opencode.json"))
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;

        let opencode_path_str = opencode_path.to_string_lossy().to_string();

        // Safe read: no TOCTOU, no symlink following
        let merged_json = match safe_read_existing_file(&opencode_path_str)? {
            Some(existing) => opencode.merge_with_existing(&existing)?,
            None => opencode.to_json()?,
        };

        std::fs::write(&opencode_path, merged_json)?;

        // Set restrictive permissions for security (API keys in config)
        #[cfg(unix)]
        std::fs::set_permissions(&opencode_path, std::fs::Permissions::from_mode(0o600))?;

        println!(
            "✓ OpenCode config saved to {:?} (preserved existing settings)",
            opencode_path
        );
    }

    println!("\n🎉 Configuration complete!");
    println!("   Run 'aperture-router' to start the server.");

    Ok(())
}

/// Stub for wizard when feature is not enabled
#[cfg(not(feature = "wizard"))]
pub async fn run_wizard(
    _config_path: &str,
    _aperture_url: Option<String>,
    _output_path: Option<String>,
) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "Wizard feature not enabled. Build with --features wizard to use interactive configuration.\n\
         Alternatively, use 'config generate' or set environment variables."
    ))
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
    println!(
        "\nTotal: {} models from {} providers",
        models.len(),
        grouped.len()
    );

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
            let status = if provider.enabled {
                "enabled"
            } else {
                "disabled"
            };
            println!("  {} [{}]", provider.name, status);
            println!("    URL: {}", provider.base_url);
            println!("    Style: {:?}", provider.endpoint_style);
            println!("    Models: {}", provider.models.join(", "));
            println!(
                "    API Key: {}",
                if provider.api_key.is_some() {
                    "configured"
                } else {
                    "none"
                }
            );
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
                config
                    .providers
                    .iter()
                    .map(|p| &p.name)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    Ok(())
}

/// Safely read a file, avoiding symlink attacks
///
/// Returns None if file doesn't exist, or error if it's a symlink or read fails
fn safe_read_existing_file(path: &str) -> anyhow::Result<Option<String>> {
    let path = std::path::Path::new(path);

    // Try to get metadata without following symlinks
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            // Refuse to follow symlinks for security
            if metadata.file_type().is_symlink() {
                return Err(anyhow::anyhow!(
                    "Refusing to follow symlink: {}",
                    path.display()
                ));
            }
            // File exists and is regular, read it
            Ok(Some(std::fs::read_to_string(path)?))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist - this is fine
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

/// Export configuration
pub async fn export_config(
    config_path: &str,
    toml_format: bool,
    opencode_format: bool,
    openclaw_format: bool,
    output_path: Option<String>,
    router_url: &str,
) -> anyhow::Result<()> {
    // Warn if both formats specified with same output
    if opencode_format && openclaw_format && output_path.is_some() {
        eprintln!("Warning: --opencode and --openclaw with -o will overwrite. Each format writes to its own default file.");
    }

    let config = Config::load(config_path)?;

    if toml_format || (!opencode_format && !openclaw_format) {
        let path = output_path
            .clone()
            .unwrap_or_else(|| "config.toml".to_string());
        config.save(&path)?;
        println!("Config exported to {}", path);
    }

    if opencode_format {
        // Fetch models to get metadata
        let models = fetch_models(&config.aperture.base_url).await?;

        let opencode = OpenCodeConfig::from_router_config(&config, &models, router_url);

        let path = output_path
            .clone()
            .unwrap_or_else(|| "opencode.json".to_string());

        // Safe read: no TOCTOU, no symlink following
        let json = match safe_read_existing_file(&path)? {
            Some(existing) => opencode.merge_with_existing(&existing)?,
            None => opencode.to_json()?,
        };

        std::fs::write(&path, json)?;

        // Set restrictive permissions for security (API keys in config)
        #[cfg(unix)]
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;

        println!("OpenCode config exported to {}", path);
    }

    if openclaw_format {
        // Fetch models to get metadata
        let models = fetch_models(&config.aperture.base_url).await?;

        let openclaw = OpenClawConfig::from_router_config(&config, &models, router_url);

        let path = output_path
            .clone()
            .unwrap_or_else(|| "openclaw.json".to_string());

        // Safe read: no TOCTOU, no symlink following
        let json = match safe_read_existing_file(&path)? {
            Some(existing) => openclaw.merge_with_existing(&existing)?,
            None => openclaw.to_json()?,
        };

        std::fs::write(&path, json)?;

        // Set restrictive permissions for security (future API keys in config)
        #[cfg(unix)]
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;

        println!("OpenClaw config exported to {}", path);
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

/// Generate config from environment variables (non-interactive)
pub fn generate_config(
    config_path: &str,
    url: Option<String>,
    output_path: Option<String>,
    generate_key: bool,
) -> anyhow::Result<()> {
    // Get URL from argument or environment
    let aperture_url = url
        .or_else(|| std::env::var("APERTURE_BASE_URL").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Aperture URL required. Use --url or set APERTURE_BASE_URL environment variable"
            )
        })?;

    // Create minimal config
    let mut config = Config::default();
    config.aperture.base_url = aperture_url.clone();

    // Generate API key if requested
    if generate_key {
        let api_key = generate_api_key();
        config.security.api_keys = vec![api_key.clone()];
        // Print to stdout (not stderr which may be captured by logging systems)
        println!("🔑 Generated API key: {}", api_key);
        println!("   Save this key securely - it won't be shown again!");
        println!();
    }

    // Get API key from environment if set
    if let Ok(key) = std::env::var("APERTURE_API_KEY") {
        if !key.is_empty() {
            config.security.api_keys = vec![key];
        }
    }

    // Allow no auth if explicitly set
    if std::env::var("APERTURE_ALLOW_NO_AUTH").is_ok() {
        config.security.require_auth_in_prod = false;
    }

    // Save config
    let save_path = output_path.as_deref().unwrap_or(config_path);
    config.save(save_path)?;

    // Fix ownership if saving to system path
    fix_system_config_ownership(save_path)?;

    println!("✓ Config generated at {}", save_path);
    println!();
    println!("Aperture URL: {}", aperture_url);
    println!(
        "API Keys: {}",
        if config.security.api_keys.is_empty() {
            "none"
        } else {
            "configured"
        }
    );
    println!("Auth Required: {}", config.security.require_auth_in_prod);
    println!();

    // Print helpful message based on config location
    if save_path == SYSTEM_CONFIG_PATH {
        println!("System service config created. To start:");
        println!("  sudo systemctl enable --now aperture-router");
    } else {
        println!("To start the server:");
        println!("  aperture-router --config {}", save_path);
    }

    Ok(())
}

/// Generate a secure random API key that passes validation (32+ chars, 20+ unique)
/// Uses base62 encoding (a-z, A-Z, 0-9) to ensure sufficient character diversity
fn generate_api_key() -> String {
    // Base62 alphabet: a-z (26) + A-Z (26) + 0-9 (10) = 62 unique characters
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

    let mut result = String::with_capacity(40);
    result.push_str("apr_");

    // Use two UUIDs to ensure we have enough bytes for 32+ character output
    let uuid1 = uuid::Uuid::new_v4();
    let uuid2 = uuid::Uuid::new_v4();

    // Combine both UUIDs into a single number for base62 encoding
    let bytes1 = uuid1.as_bytes();
    let bytes2 = uuid2.as_bytes();

    // Encode first UUID
    let mut num1 = u128::from_be_bytes([
        bytes1[0], bytes1[1], bytes1[2], bytes1[3], bytes1[4], bytes1[5], bytes1[6], bytes1[7],
        bytes1[8], bytes1[9], bytes1[10], bytes1[11], bytes1[12], bytes1[13], bytes1[14],
        bytes1[15],
    ]);

    while num1 > 0 {
        let rem = (num1 % 62) as usize;
        result.push(ALPHABET[rem] as char);
        num1 /= 62;
    }

    // Encode second UUID if needed for length
    let mut num2 = u128::from_be_bytes([
        bytes2[0], bytes2[1], bytes2[2], bytes2[3], bytes2[4], bytes2[5], bytes2[6], bytes2[7],
        bytes2[8], bytes2[9], bytes2[10], bytes2[11], bytes2[12], bytes2[13], bytes2[14],
        bytes2[15],
    ]);

    // Ensure we reach at least 32 characters
    while result.len() < 36 && num2 > 0 {
        let rem = (num2 % 62) as usize;
        result.push(ALPHABET[rem] as char);
        num2 /= 62;
    }

    result
}
