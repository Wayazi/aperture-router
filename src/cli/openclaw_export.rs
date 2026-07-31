// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! OpenClaw (openclaw.json) export format
//!
//! Converts router configuration to OpenClaw provider format
//! for integration with LoongClaw and other OpenClaw-compatible AI frameworks.
//!
//! Reference: <https://github.com/openclaw/openclaw> (src/config/types.models.ts)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::model_fetcher::EnrichedModel;
use crate::config::Config;

/// OpenClaw configuration structure (openclaw.json)
///
/// Format reference: openclaw/src/config/types.openclaw.ts
/// Model types: openclaw/src/config/types.models.ts
/// Agent types: openclaw/src/config/types.agent-defaults.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawConfig {
    models: OpenClawModels,
    #[serde(skip_serializing_if = "Option::is_none")]
    agents: Option<OpenClawAgents>,
}

/// Models section of OpenClaw config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawModels {
    /// "merge" = add providers to existing, "replace" = overwrite all
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    providers: HashMap<String, OpenClawProvider>,
}

/// OpenClaw provider entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawProvider {
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(rename = "apiKey", skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
    /// API type: "openai-completions", "anthropic-messages", etc.
    #[serde(rename = "api", skip_serializing_if = "Option::is_none")]
    api_type: Option<String>,
    /// Models array (required by OpenClaw)
    models: Vec<OpenClawModel>,
}

/// OpenClaw model definition
/// Reference: openclaw/src/config/types.models.ts ModelDefinitionConfig
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawModel {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    api: Option<String>,
    reasoning: bool,
    input: Vec<String>,
    cost: OpenClawCost,
    #[serde(rename = "contextWindow")]
    context_window: u32,
    #[serde(rename = "maxTokens")]
    max_tokens: u32,
}

/// Cost information per million tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawCost {
    input: f64,
    output: f64,
    #[serde(rename = "cacheRead")]
    cache_read: f64,
    #[serde(rename = "cacheWrite")]
    cache_write: f64,
}

/// Minimal agents section for model selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawAgents {
    defaults: OpenClawAgentDefaults,
}

/// Agent defaults - model selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawAgentDefaults {
    /// Primary model in "provider/model" format (e.g. "aperture-glm/GLM-5")
    model: String,
}

impl OpenClawConfig {
    /// Create OpenClaw config from router config and enriched models.
    ///
    /// Note: `_config` is reserved for future use (e.g., extracting provider-specific
    /// settings like API keys or endpoint styles). Currently unused as all needed
    /// information comes from the enriched models.
    pub fn from_router_config(
        _config: &Config,
        models: &[EnrichedModel],
        router_url: &str,
    ) -> Self {
        let mut providers_map: HashMap<String, OpenClawProvider> = HashMap::new();
        let mut primary_model_ref = String::new();

        // Group models by provider_id and build provider entries
        for model in models {
            let provider_key = format!("aperture-{}", model.provider_id);

            // Use safe defaults - we only know id and name from Aperture
            // All other fields use conservative defaults
            let oc_model = OpenClawModel {
                id: model.id.clone(),
                name: model.display_name.clone(),
                api: None,
                reasoning: false, // Safe default: assume no extended reasoning
                input: vec!["text".to_string()], // Safe default: all models support text
                cost: OpenClawCost {
                    input: 0.0,
                    output: 0.0,
                    cache_read: 0.0,
                    cache_write: 0.0,
                },
                context_window: 128_000, // Reasonable default
                max_tokens: 4096,        // Reasonable default
            };

            let provider = providers_map
                .entry(provider_key.clone())
                .or_insert_with(|| OpenClawProvider {
                    base_url: format!("{}/v1", router_url.trim_end_matches('/')),
                    api_key: None,
                    api_type: Some("openai-completions".to_string()),
                    models: Vec::new(),
                });

            provider.models.push(oc_model);

            // First model becomes primary (no hardcoded model name assumptions)
            if primary_model_ref.is_empty() {
                primary_model_ref = format!("{}/{}", provider_key, model.id);
            }
        }

        // Fallback if no models found - use sorted keys for deterministic order
        if primary_model_ref.is_empty() {
            let mut sorted_keys: Vec<_> = providers_map.keys().collect();
            sorted_keys.sort(); // Alphabetical order for deterministic behavior
            if let Some(pk) = sorted_keys.first() {
                if let Some(provider) = providers_map.get(*pk) {
                    if let Some(first_model) = provider.models.first() {
                        primary_model_ref = format!("{}/{}", pk, first_model.id);
                    }
                }
            }
        }

        let agents = if primary_model_ref.is_empty() {
            None
        } else {
            Some(OpenClawAgents {
                defaults: OpenClawAgentDefaults {
                    model: primary_model_ref,
                },
            })
        };

        Self {
            models: OpenClawModels {
                mode: Some("merge".to_string()),
                providers: providers_map,
            },
            agents,
        }
    }

    /// Export to JSON string
    pub fn to_json(&self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize OpenClaw config: {}", e))
    }

    /// Merge this config with an existing openclaw.json, preserving all existing settings.
    ///
    /// Only ADDS/updates the `aperture-*` providers. Does NOT touch:
    /// - Other providers (openai, anthropic, custom, etc.)
    /// - Agents, tools, themes, or any other top-level keys
    /// - Existing `agents.defaults.model` (only sets if missing)
    pub fn merge_with_existing(&self, existing_json: &str) -> anyhow::Result<String> {
        let mut existing: serde_json::Value = serde_json::from_str(existing_json)
            .map_err(|e| anyhow::anyhow!("Failed to parse existing OpenClaw config: {}", e))?;

        let new_config = serde_json::to_value(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize new config: {}", e))?;

        let existing_obj = existing
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("Existing config is not an object"))?;

        // Ensure "models" object exists
        if !existing_obj.contains_key("models") {
            existing_obj.insert("models".to_string(), serde_json::json!({}));
        }

        let models_obj = existing_obj
            .get_mut("models")
            .and_then(|m| m.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("Existing models section is not an object"))?;

        // Set mode to "merge" to preserve existing providers
        if !models_obj.contains_key("mode") {
            models_obj.insert("mode".to_string(), serde_json::json!("merge"));
        }

        // Ensure "providers" object exists
        if !models_obj.contains_key("providers") {
            models_obj.insert("providers".to_string(), serde_json::json!({}));
        }

        let providers_obj = models_obj
            .get_mut("providers")
            .and_then(|p| p.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("Existing providers section is not an object"))?;

        // Only ADD/UPDATE our aperture-* providers (preserve all others)
        if let Some(new_providers) = new_config
            .get("models")
            .and_then(|m| m.get("providers"))
            .and_then(|p| p.as_object())
        {
            for (key, value) in new_providers {
                providers_obj.insert(key.clone(), value.clone());
            }
        }

        // Only set agents.defaults.model if not already configured
        if let Some(new_agents) = new_config.get("agents") {
            let has_agents_model = existing_obj
                .get("agents")
                .and_then(|a| a.get("defaults"))
                .and_then(|d| d.get("model"))
                .is_some();

            if !has_agents_model {
                existing_obj.insert("agents".to_string(), new_agents.clone());
            }
        }

        // Everything else in existing_obj is untouched (channels, hooks, plugins, etc.)

        serde_json::to_string_pretty(&existing)
            .map_err(|e| anyhow::anyhow!("Failed to serialize merged config: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;

    fn create_test_model(id: &str, provider_id: &str) -> EnrichedModel {
        EnrichedModel {
            id: id.to_string(),
            provider_id: provider_id.to_string(),
            canonical_id: format!("{}/{}", provider_id, id),
            display_name: format!("{} [{}]", id, provider_id),
        }
    }

    #[test]
    fn test_openclaw_config_creation() {
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

        let openclaw =
            OpenClawConfig::from_router_config(&config, &models, "http://127.0.0.1:8765");

        // Models is an array
        let glm_provider = &openclaw.models.providers["aperture-glm"];
        assert!(!glm_provider.models.is_empty());
        assert!(glm_provider.models[0].id == "GLM-5");
        assert!(glm_provider.models[0].input.contains(&"text".to_string()));
        // Safe defaults
        assert!(!glm_provider.models[0].reasoning); // default
        assert!(glm_provider.models[0].context_window == 128_000); // reasonable default
        assert!(glm_provider.models[0].max_tokens == 4096); // reasonable default

        // Agent model selection uses provider/model format (first model is primary)
        assert_eq!(
            openclaw.agents.as_ref().unwrap().defaults.model,
            "aperture-glm/GLM-5"
        );

        // Mode is merge
        assert_eq!(openclaw.models.mode.as_deref().unwrap(), "merge");
    }

    #[test]
    fn test_openclaw_json_export() {
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

        let openclaw =
            OpenClawConfig::from_router_config(&config, &models, "http://127.0.0.1:8765");
        let json = openclaw.to_json().unwrap();

        assert!(json.contains("\"aperture-glm\""));
        assert!(json.contains("\"baseUrl\""));
        assert!(json.contains("\"openai-completions\""));
        assert!(json.contains("\"reasoning\""));
        assert!(json.contains("\"contextWindow\""));
        assert!(json.contains("\"maxTokens\""));
        assert!(json.contains("\"merge\""));
        // Models should be array, not object
        let has_array = json.contains("\"models\": [");
        assert!(
            has_array,
            "Models should be serialized as array. JSON:\n{}",
            json
        );
    }

    #[test]
    fn test_openclaw_merge_preserves_existing() {
        let config = Config::default();
        let models = vec![create_test_model("GLM-5", "glm")];
        let openclaw =
            OpenClawConfig::from_router_config(&config, &models, "http://127.0.0.1:8765");

        // Existing config with other providers, agents, channels
        let existing = r#"{
            "agents": {
                "defaults": {
                    "model": "openai/gpt-4o"
                }
            },
            "models": {
                "mode": "merge",
                "providers": {
                    "openai": {
                        "baseUrl": "https://api.openai.com/v1",
                        "apiKey": "sk-xxx",
                        "models": [{"id": "gpt-4o", "name": "GPT-4o", "reasoning": false, "input": ["text"], "cost": {"input": 2.5, "output": 10, "cacheRead": 0, "cacheWrite": 0}, "contextWindow": 128000, "maxTokens": 4096}]
                    }
                }
            },
            "channels": {
                "telegram": { "token": "123" }
            }
        }"#;

        let merged = openclaw.merge_with_existing(existing).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&merged).unwrap();

        // Preserved: agents model (NOT overwritten)
        assert_eq!(parsed["agents"]["defaults"]["model"], "openai/gpt-4o");

        // Preserved: channels
        assert_eq!(parsed["channels"]["telegram"]["token"], "123");

        // Preserved: existing openai provider
        assert_eq!(
            parsed["models"]["providers"]["openai"]["baseUrl"],
            "https://api.openai.com/v1"
        );

        // Added: new aperture-glm provider
        assert!(parsed["models"]["providers"]["aperture-glm"].is_object());

        // Preserved: mode
        assert_eq!(parsed["models"]["mode"], "merge");
    }

    #[test]
    fn test_openclaw_merge_sets_agents_if_missing() {
        let config = Config::default();
        let models = vec![create_test_model("GLM-5", "glm")];
        let openclaw =
            OpenClawConfig::from_router_config(&config, &models, "http://127.0.0.1:8765");

        // Config with no agents section
        let existing = r#"{"models": {"providers": {}}}"#;
        let merged = openclaw.merge_with_existing(existing).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&merged).unwrap();

        // Should set agents.defaults.model since it was missing
        assert_eq!(parsed["agents"]["defaults"]["model"], "aperture-glm/GLM-5");
    }

    #[test]
    fn test_openclaw_merge_rejects_malformed_json() {
        let config = Config::default();
        let models = vec![create_test_model("GLM-5", "glm")];
        let openclaw =
            OpenClawConfig::from_router_config(&config, &models, "http://127.0.0.1:8765");

        let malformed = r#"{"models": {invalid json}"#;
        assert!(openclaw.merge_with_existing(malformed).is_err());
    }

    #[test]
    fn test_openclaw_merge_rejects_non_object_json() {
        let config = Config::default();
        let models = vec![create_test_model("GLM-5", "glm")];
        let openclaw =
            OpenClawConfig::from_router_config(&config, &models, "http://127.0.0.1:8765");

        assert!(openclaw.merge_with_existing("null").is_err());
        assert!(openclaw.merge_with_existing("[]").is_err());
        assert!(openclaw.merge_with_existing("\"string\"").is_err());
        assert!(openclaw.merge_with_existing("42").is_err());
    }

    #[test]
    fn test_openclaw_fallback_when_no_models() {
        let config = Config::default();
        let openclaw = OpenClawConfig::from_router_config(&config, &[], "http://127.0.0.1:8765");

        assert!(openclaw.models.providers.is_empty());
        assert!(openclaw.agents.is_none());
    }
}
