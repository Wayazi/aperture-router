// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! OpenCode.json export format
//!
//! Converts router configuration to OpenCode provider format
//! for seamless integration with OpenCode AI assistant.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::model_fetcher::EnrichedModel;
use crate::config::{Config, Provider};

/// OpenCode configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeConfig {
    #[serde(rename = "$schema")]
    schema: String,
    model: String,
    small_model: String,
    provider: HashMap<String, OpenCodeProvider>,
}

/// OpenCode provider structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeProvider {
    name: String,
    npm: String,
    models: HashMap<String, OpenCodeModel>,
    options: OpenCodeOptions,
}

/// OpenCode model entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeModel {
    name: String,
}

/// OpenCode provider options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeOptions {
    #[serde(rename = "apiKey")]
    api_key: String,
    #[serde(rename = "baseURL")]
    base_url: String,
}

impl OpenCodeConfig {
    /// Create OpenCode config from router config and enriched models
    /// No hardcoded plans - uses actual model names from Aperture
    pub fn from_router_config(
        _config: &Config,
        models: &[EnrichedModel],
        router_url: &str,
    ) -> Self {
        let mut models_map: HashMap<String, OpenCodeModel> = HashMap::new();
        let mut primary_model = String::new();
        let mut small_model = String::new();

        // Build models map from all fetched models
        for model in models {
            // Use actual model ID (what Aperture expects)
            let model_id = &model.id;
            models_map.insert(
                model_id.clone(),
                OpenCodeModel {
                    name: format!("{} [{}]", model.id, model.provider_id),
                },
            );

            // Heuristic: first non-flash model becomes primary
            if primary_model.is_empty()
                && !model.id.contains("flash")
                && !model.id.contains("haiku")
            {
                primary_model = format!("router/{}", model_id);
            }

            // Heuristic: first flash/haiku model becomes small
            if small_model.is_empty() && (model.id.contains("flash") || model.id.contains("haiku"))
            {
                small_model = format!("router/{}", model_id);
            }
        }

        // Fallback if no models found
        if primary_model.is_empty() {
            if let Some(first_model) = models_map.keys().next() {
                primary_model = format!("router/{}", first_model);
            }
        }
        if small_model.is_empty() {
            small_model = primary_model.clone();
        }

        let mut providers = HashMap::new();
        providers.insert(
            "router".to_string(),
            OpenCodeProvider {
                name: "Aperture Router".to_string(),
                npm: "@ai-sdk/anthropic".to_string(),
                models: models_map,
                options: OpenCodeOptions {
                    api_key: "-".to_string(), // Router handles auth
                    base_url: format!("{}/v1", router_url.trim_end_matches('/')),
                },
            },
        );

        Self {
            schema: "https://opencode.ai/config.json".to_string(),
            model: primary_model,
            small_model,
            provider: providers,
        }
    }

    /// Export to JSON string
    pub fn to_json(&self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize OpenCode config: {}", e))
    }

    /// Merge this config with an existing opencode.json, preserving MCP and other settings
    pub fn merge_with_existing(&self, existing_json: &str) -> anyhow::Result<String> {
        let mut existing: serde_json::Value = serde_json::from_str(existing_json)
            .map_err(|e| anyhow::anyhow!("Failed to parse existing OpenCode config: {}", e))?;

        let new_config = serde_json::to_value(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize new config: {}", e))?;

        // Get the new values
        if let Some(new_obj) = new_config.as_object() {
            let existing_obj = existing
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("Existing config is not an object"))?;

            // Update only our specific fields, preserve everything else
            if let Some(model) = new_obj.get("model") {
                existing_obj.insert("model".to_string(), model.clone());
            }
            if let Some(small_model) = new_obj.get("small_model") {
                existing_obj.insert("small_model".to_string(), small_model.clone());
            }
            if let Some(provider) = new_obj.get("provider") {
                existing_obj.insert("provider".to_string(), provider.clone());
            }
            // Preserve: mcp, theme, autoupdate, and any other existing settings
        }

        serde_json::to_string_pretty(&existing)
            .map_err(|e| anyhow::anyhow!("Failed to serialize merged config: {}", e))
    }
}

/// Create provider configurations from enriched models (dynamic, no hardcoded plans)
pub fn create_providers_from_models(models: &[EnrichedModel], aperture_url: &str) -> Vec<Provider> {
    let mut providers: HashMap<String, Provider> = HashMap::new();

    for model in models {
        let provider_name = model.provider_id.clone();

        let provider = providers.entry(provider_name.clone()).or_insert(Provider {
            name: provider_name,
            base_url: aperture_url.trim().to_string(),
            api_key: None,
            endpoint_style: crate::config::EndpointStyle::Anthropic,
            models: Vec::new(),
            enabled: true,
        });

        // Add model if not already present
        if !provider.models.contains(&model.id) {
            provider.models.push(model.id.clone());
        }
    }

    providers.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_model(id: &str, provider_id: &str) -> EnrichedModel {
        EnrichedModel {
            id: id.to_string(),
            provider_id: provider_id.to_string(),
            canonical_id: format!("{}/{}", provider_id, id),
            display_name: format!("{} [{}]", id, provider_id),
        }
    }

    #[test]
    fn test_opencode_config_creation() {
        let mut config = Config::default();
        config.providers.push(Provider {
            name: "glm".to_string(),
            base_url: "http://100.100.100.100".to_string(),
            api_key: None,
            endpoint_style: crate::config::EndpointStyle::Anthropic,
            models: vec!["GLM-5".to_string(), "glm-4.7".to_string()],
            enabled: true,
        });

        let models = vec![
            create_test_model("GLM-5", "glm"),
            create_test_model("glm-4.7", "glm"),
            create_test_model("glm-4.7-flash", "glm2"),
        ];

        let opencode =
            OpenCodeConfig::from_router_config(&config, &models, "http://127.0.0.1:8765");

        assert!(opencode.model.starts_with("router/"));
        assert!(opencode.provider.contains_key("router"));
    }

    #[test]
    fn test_opencode_json_export() {
        let mut config = Config::default();
        config.providers.push(Provider {
            name: "glm".to_string(),
            base_url: "http://test".to_string(),
            api_key: None,
            endpoint_style: crate::config::EndpointStyle::Anthropic,
            models: vec!["GLM-5".to_string()],
            enabled: true,
        });

        let models = vec![create_test_model("GLM-5", "glm")];

        let opencode =
            OpenCodeConfig::from_router_config(&config, &models, "http://127.0.0.1:8765");
        let json = opencode.to_json().unwrap();

        assert!(json.contains("\"router\""));
        assert!(json.contains("\"baseURL\""));
    }
}
