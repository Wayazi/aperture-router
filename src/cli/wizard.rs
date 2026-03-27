// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! Interactive configuration wizard
//!
//! Provides an interactive CLI wizard for configuring aperture-router:
//! - Aperture URL input
//! - Provider/model selection (dynamic from Aperture, no hardcoded plans)
//! - API key input (secure)
//! - Configuration preview and save

use inquire::{validator::Validation, Confirm, CustomType, MultiSelect, Password, Text};
use std::collections::HashSet;

use super::model_fetcher::{fetch_models, EnrichedModel};
use super::opencode_export::{create_providers_from_models, OpenCodeConfig};
use super::security::{validate_api_key, validate_url, SecretString};
use crate::config::Config;

/// Interactive configuration wizard
pub struct ConfigWizard {
    aperture_url: Option<String>,
    #[allow(dead_code)]
    router_url: String,
}

impl ConfigWizard {
    /// Create a new wizard with optional defaults
    pub fn new(aperture_url: Option<String>, router_url: String) -> Self {
        Self {
            aperture_url,
            router_url,
        }
    }

    /// Run the interactive wizard
    pub async fn run(&self) -> anyhow::Result<WizardResult> {
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║         APERTURE ROUTER CONFIGURATION WIZARD                 ║");
        println!("╚══════════════════════════════════════════════════════════════╝\n");

        // Step 1: Aperture URL
        let aperture_url = self.prompt_aperture_url()?;

        // Step 2: Fetch models
        println!("\n📡 Fetching available models from Aperture...");
        let models = fetch_models(&aperture_url).await?;

        if models.is_empty() {
            return Err(anyhow::anyhow!("No models found at Aperture gateway"));
        }

        println!("✓ Found {} models\n", models.len());

        // Step 3: Select providers (dynamic from Aperture)
        let selected_providers = self.prompt_provider_selection(&models)?;

        if selected_providers.is_empty() {
            return Err(anyhow::anyhow!("No providers selected"));
        }

        // Step 4: Model selection per provider
        let selected_models = self.prompt_model_selection(&models, &selected_providers)?;

        // Step 5: API key (optional)
        let api_key = self.prompt_api_key()?;

        // Step 6: Router settings
        let router_port = self.prompt_router_port()?;

        // Step 7: Summary
        let config = self.build_config(
            &aperture_url,
            &selected_models,
            api_key.clone(),
            router_port,
        );

        self.display_summary(&config, &selected_models);

        // Step 8: Confirm
        let confirm = Confirm::new("Save this configuration?")
            .with_default(true)
            .prompt()?;

        if !confirm {
            println!("\n❌ Configuration cancelled.");
            return Err(anyhow::anyhow!("Configuration cancelled by user"));
        }

        // Step 9: Export options
        let export_opencode = Confirm::new("Export OpenCode configuration as well?")
            .with_default(true)
            .prompt()?;

        let opencode_config = if export_opencode {
            Some(OpenCodeConfig::from_router_config(
                &config,
                &selected_models,
                &format!("http://127.0.0.1:{}", router_port),
            ))
        } else {
            None
        };

        println!("\n✅ Configuration ready to save!");

        Ok(WizardResult {
            config,
            opencode_config,
        })
    }

    fn prompt_aperture_url(&self) -> anyhow::Result<String> {
        let default = self
            .aperture_url
            .as_deref()
            .unwrap_or("http://100.100.100.100");

        Text::new("Aperture gateway URL:")
            .with_default(default)
            .with_validator(|input: &str| match validate_url(input) {
                Ok(_) => Ok(Validation::Valid),
                Err(e) => Ok(Validation::Invalid(e.into())),
            })
            .with_help_message("Your Tailscale Aperture IP or hostname")
            .prompt()
            .map_err(|e| anyhow::anyhow!("Failed to get URL: {}", e))
    }

    fn prompt_provider_selection(&self, models: &[EnrichedModel]) -> anyhow::Result<Vec<String>> {
        // Group by actual provider ID from Aperture (dynamic, no hardcoded plans)
        let mut providers: Vec<String> = models
            .iter()
            .map(|m| m.provider_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        providers.sort();

        if providers.is_empty() {
            return Ok(vec![]);
        }

        let selected: Vec<String> = MultiSelect::new(
            "Select providers to enable (models will be fetched dynamically at runtime):",
            providers.clone(),
        )
        .with_all_selected_by_default()
        .with_help_message(
            "All models from selected providers will be available through the router",
        )
        .prompt()
        .map_err(|e| anyhow::anyhow!("Failed to select providers: {}", e))?;

        Ok(selected)
    }

    fn prompt_model_selection(
        &self,
        models: &[EnrichedModel],
        selected_providers: &[String],
    ) -> anyhow::Result<Vec<EnrichedModel>> {
        let mut selected_models = Vec::new();

        for provider_id in selected_providers {
            let provider_models: Vec<&EnrichedModel> = models
                .iter()
                .filter(|m| m.provider_id == *provider_id)
                .collect();

            if provider_models.is_empty() {
                continue;
            }

            let model_options: Vec<ModelOption> = provider_models
                .iter()
                .map(|m| ModelOption {
                    model: (*m).clone(),
                    label: format!("{} → {}", m.id, m.canonical_id),
                })
                .collect();

            let selected: Vec<ModelOption> = MultiSelect::new(
                &format!("Select models for provider '{}':", provider_id),
                model_options,
            )
            .with_all_selected_by_default()
            .prompt()
            .map_err(|e| anyhow::anyhow!("Failed to select models: {}", e))?;

            selected_models.extend(selected.into_iter().map(|opt| opt.model));
        }

        Ok(selected_models)
    }

    fn prompt_api_key(&self) -> anyhow::Result<Option<SecretString>> {
        let has_key = Confirm::new("Configure API key for Aperture?")
            .with_default(false)
            .with_help_message("Required if your Aperture requires authentication")
            .prompt()?;

        if !has_key {
            return Ok(None);
        }

        let key = Password::new("API key:")
            .with_help_message("Input will not be displayed")
            .with_validator(|input: &str| {
                if input.is_empty() {
                    Ok(Validation::Invalid("API key cannot be empty".into()))
                } else if let Err(e) = validate_api_key(input) {
                    Ok(Validation::Invalid(e.into()))
                } else {
                    Ok(Validation::Valid)
                }
            })
            .prompt()
            .map_err(|e| anyhow::anyhow!("Failed to get API key: {}", e))?;

        Ok(Some(SecretString::new(key)))
    }

    fn prompt_router_port(&self) -> anyhow::Result<u16> {
        CustomType::new("Router port:")
            .with_default(8765u16)
            .with_help_message("Port for the router to listen on")
            .prompt()
            .map_err(|e| anyhow::anyhow!("Failed to get port: {}", e))
    }

    #[allow(clippy::field_reassign_with_default)]
    fn build_config(
        &self,
        aperture_url: &str,
        selected_models: &[EnrichedModel],
        api_key: Option<SecretString>,
        port: u16,
    ) -> Config {
        let mut config = Config::default();

        config.host = "127.0.0.1".to_string();
        config.port = port;
        config.aperture.base_url = aperture_url.trim().to_string();
        config.aperture.api_key = api_key.map(|s| s.expose().to_string());

        // Create providers from selected models (dynamic, no hardcoded plans)
        config.providers = create_providers_from_models(selected_models, aperture_url);

        // No aliases - use actual model names from Aperture directly

        // Disable auth requirement for development
        config.security.require_auth_in_prod = false;

        config
    }

    fn display_summary(&self, config: &Config, selected_models: &[EnrichedModel]) {
        println!("\n┌──────────────────────────────────────────────────────────────┐");
        println!("│ CONFIGURATION SUMMARY                                        │");
        println!("├──────────────────────────────────────────────────────────────┤");
        println!("│ Router: http://{}:{}", config.host, config.port);
        println!("│ Aperture: {}", config.aperture.base_url);
        println!(
            "│ API Key: {}",
            if config.aperture.api_key.is_some() {
                "configured"
            } else {
                "none"
            }
        );
        println!("│");
        println!("│ Providers (auto-discovered from Aperture):");

        for provider in &config.providers {
            println!("│   {} ({} models)", provider.name, provider.models.len());
            for model in &provider.models {
                let enriched = selected_models.iter().find(|m| m.id == *model);
                let display = enriched.map(|m| m.display_name.as_str()).unwrap_or(model);
                println!("│     - {}", display);
            }
        }

        println!("│");
        println!("│ Note: Models are refreshed automatically from Aperture");
        println!("│       New models/providers will be detected at runtime");
        println!("└──────────────────────────────────────────────────────────────┘");
    }
}

/// Result of the wizard
pub struct WizardResult {
    pub config: Config,
    pub opencode_config: Option<OpenCodeConfig>,
}

/// Helper struct for model selection
struct ModelOption {
    model: EnrichedModel,
    label: String,
}

impl std::fmt::Display for ModelOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}
