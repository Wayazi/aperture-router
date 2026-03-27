// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use crate::config::{EndpointStyle, Provider};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Registry for managing multiple providers and model routing
/// Supports dynamic updates from Aperture discovery
#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    /// All providers indexed by name (dynamic)
    providers: Arc<RwLock<HashMap<String, Provider>>>,

    /// Mapping from model name to provider name (dynamic)
    model_to_provider: Arc<RwLock<HashMap<String, String>>>,

    /// Base URL for Aperture gateway (for auto-generated providers)
    aperture_base_url: String,
}

impl ProviderRegistry {
    /// Create a new provider registry with initial providers from config
    pub fn new(providers: Vec<Provider>) -> Self {
        let mut provider_map = HashMap::new();
        let mut model_map = HashMap::new();

        for provider in providers {
            if provider.enabled {
                let name = provider.name.clone();
                for model in &provider.models {
                    if let Some(existing) = model_map.get(model) {
                        if existing != &name {
                            warn!(
                                "Model '{}' mapped to multiple providers: '{}' will be replaced by '{}'",
                                model, existing, name
                            );
                        }
                    }
                    model_map.insert(model.clone(), name.clone());
                }
                provider_map.insert(name, provider);
            }
        }

        Self {
            providers: Arc::new(RwLock::new(provider_map)),
            model_to_provider: Arc::new(RwLock::new(model_map)),
            aperture_base_url: String::new(),
        }
    }

    /// Create registry with Aperture gateway URL for auto-discovery
    pub fn with_aperture_url(providers: Vec<Provider>, aperture_url: String) -> Self {
        let mut registry = Self::new(providers);
        registry.aperture_base_url = aperture_url;
        registry
    }

    /// Update registry from discovered models (called by auto-refresh)
    pub async fn update_from_discovery(
        &self,
        models_by_provider: &HashMap<String, Vec<String>>,
        aperture_url: &str,
    ) {
        let mut providers = self.providers.write().await;
        let mut model_map = self.model_to_provider.write().await;

        // Track which providers we've seen
        let mut seen_providers: HashSet<String> = HashSet::new();

        for (provider_id, model_ids) in models_by_provider {
            seen_providers.insert(provider_id.clone());

            // Check if provider already exists (from config)
            let provider_exists = providers.contains_key(provider_id);

            if !provider_exists {
                // Auto-create provider from discovery
                let new_provider = Provider {
                    name: provider_id.clone(),
                    base_url: aperture_url.to_string(),
                    api_key: None,
                    endpoint_style: EndpointStyle::Anthropic, // Aperture uses Anthropic style
                    models: model_ids.clone(),
                    enabled: true,
                };

                providers.insert(provider_id.clone(), new_provider);
                info!(
                    "✨ Auto-added provider '{}' with {} models",
                    provider_id,
                    model_ids.len()
                );
            } else {
                // Update existing provider's model list
                if let Some(provider) = providers.get_mut(provider_id) {
                    provider.models = model_ids.clone();
                }
            }

            // Update model mappings
            for model_id in model_ids {
                model_map.insert(model_id.clone(), provider_id.clone());
            }
        }

        // Remove providers that no longer exist (only auto-added ones, not config ones)
        // For now, we keep all providers - removal could be dangerous

        // Log summary
        let total_models = model_map.len();
        let total_providers = providers.len();
        drop(providers);
        drop(model_map);

        info!(
            "Registry updated: {} providers, {} models",
            total_providers, total_models
        );
    }

    /// Get provider for a specific model name
    pub async fn get_provider_for_model(&self, model: &str) -> Option<Provider> {
        let model_map = self.model_to_provider.read().await;
        let providers = self.providers.read().await;

        model_map
            .get(model)
            .and_then(|name| providers.get(name).cloned())
    }

    /// Get a provider by name
    pub async fn get_provider(&self, name: &str) -> Option<Provider> {
        self.providers.read().await.get(name).cloned()
    }

    /// Get all enabled providers
    pub async fn all_providers(&self) -> Vec<Provider> {
        self.providers.read().await.values().cloned().collect()
    }

    /// Get all available models across all providers
    pub async fn all_models(&self) -> Vec<String> {
        self.model_to_provider
            .read()
            .await
            .keys()
            .cloned()
            .collect()
    }

    /// Build the full endpoint URL for a provider based on endpoint style
    pub fn build_endpoint_url(provider: &Provider, endpoint: &str) -> String {
        let base = provider.base_url.trim_end_matches('/');

        match provider.endpoint_style {
            EndpointStyle::OpenaiV1 => {
                format!("{}/{}", base, endpoint)
            }
            EndpointStyle::OpenaiDirect => {
                let clean_endpoint = endpoint.strip_prefix("v1/").unwrap_or(endpoint);
                format!("{}/{}", base, clean_endpoint)
            }
            EndpointStyle::Anthropic => {
                format!("{}/v1/messages", base)
            }
        }
    }

    /// Get the default endpoint for a provider based on its style
    pub fn get_default_endpoint(provider: &Provider, endpoint_type: EndpointType) -> &'static str {
        match provider.endpoint_style {
            EndpointStyle::OpenaiV1 => match endpoint_type {
                EndpointType::ChatCompletions => "v1/chat/completions",
                EndpointType::Messages => "v1/messages",
            },
            EndpointStyle::OpenaiDirect => match endpoint_type {
                EndpointType::ChatCompletions => "chat/completions",
                EndpointType::Messages => "v1/messages",
            },
            EndpointStyle::Anthropic => "v1/messages",
        }
    }
}

/// Type of endpoint being requested
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointType {
    /// OpenAI chat completions endpoint
    ChatCompletions,
    /// Anthropic messages endpoint
    Messages,
}

use std::collections::HashSet;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_provider(
        name: &str,
        base_url: &str,
        style: EndpointStyle,
        models: Vec<&str>,
    ) -> Provider {
        Provider {
            name: name.to_string(),
            base_url: base_url.to_string(),
            api_key: Some("test-key".to_string()),
            endpoint_style: style,
            models: models.iter().map(|s| s.to_string()).collect(),
            enabled: true,
        }
    }

    #[tokio::test]
    async fn test_registry_creation() {
        let providers = vec![
            create_test_provider(
                "zai-credit",
                "https://api.example.com/api/paas/v4",
                EndpointStyle::OpenaiDirect,
                vec!["glm-5", "glm-4.7"],
            ),
            create_test_provider(
                "aperture",
                "http://100.100.100.100",
                EndpointStyle::OpenaiV1,
                vec!["openrouter/free"],
            ),
        ];

        let registry = ProviderRegistry::new(providers);

        assert!(registry.get_provider("zai-credit").await.is_some());
        assert!(registry.get_provider("aperture").await.is_some());
        assert!(registry.get_provider("unknown").await.is_none());
    }

    #[tokio::test]
    async fn test_model_to_provider_mapping() {
        let providers = vec![create_test_provider(
            "test-provider",
            "https://api.example.com/api/paas/v4",
            EndpointStyle::OpenaiDirect,
            vec!["glm-5", "glm-4.7"],
        )];

        let registry = ProviderRegistry::new(providers);

        let provider = registry.get_provider_for_model("glm-5").await;
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().name, "test-provider");

        let provider = registry.get_provider_for_model("unknown-model").await;
        assert!(provider.is_none());
    }

    #[tokio::test]
    async fn test_update_from_discovery() {
        let registry = ProviderRegistry::new(vec![]);

        let mut discovered = HashMap::new();
        discovered.insert(
            "glm".to_string(),
            vec!["GLM-5".to_string(), "glm-4.7".to_string()],
        );
        discovered.insert(
            "glm2".to_string(),
            vec!["GLM-5".to_string(), "glm-4.7-flash".to_string()],
        );

        registry
            .update_from_discovery(&discovered, "http://100.100.100.100")
            .await;

        assert!(registry.get_provider("glm").await.is_some());
        assert!(registry.get_provider("glm2").await.is_some());
        assert!(registry.get_provider_for_model("GLM-5").await.is_some());
    }

    #[test]
    fn test_build_endpoint_url_openai_v1() {
        let provider = create_test_provider(
            "aperture",
            "http://100.100.100.100",
            EndpointStyle::OpenaiV1,
            vec!["test"],
        );

        let url = ProviderRegistry::build_endpoint_url(&provider, "v1/chat/completions");
        assert_eq!(url, "http://100.100.100.100/v1/chat/completions");
    }

    #[test]
    fn test_build_endpoint_url_anthropic() {
        let provider = create_test_provider(
            "test-anthropic",
            "https://api.example.com/api/anthropic",
            EndpointStyle::Anthropic,
            vec!["test"],
        );

        let url = ProviderRegistry::build_endpoint_url(&provider, "v1/chat/completions");
        assert_eq!(url, "https://api.example.com/api/anthropic/v1/messages");
    }

    #[tokio::test]
    async fn test_disabled_provider_not_included() {
        let mut provider = create_test_provider(
            "disabled",
            "https://api.example.com",
            EndpointStyle::OpenaiV1,
            vec!["model-x"],
        );
        provider.enabled = false;

        let registry = ProviderRegistry::new(vec![provider]);

        assert!(registry.get_provider("disabled").await.is_none());
        assert!(registry.get_provider_for_model("model-x").await.is_none());
    }

    #[tokio::test]
    async fn test_all_models() {
        let providers = vec![
            create_test_provider(
                "provider1",
                "https://api1.example.com",
                EndpointStyle::OpenaiV1,
                vec!["model-a", "model-b"],
            ),
            create_test_provider(
                "provider2",
                "https://api2.example.com",
                EndpointStyle::OpenaiV1,
                vec!["model-c"],
            ),
        ];

        let registry = ProviderRegistry::new(providers);
        let mut models = registry.all_models().await;
        models.sort();

        assert_eq!(models, vec!["model-a", "model-b", "model-c"]);
    }
}
