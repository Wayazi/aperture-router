// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use crate::config::{EndpointStyle, Provider};
use std::collections::HashMap;
use tracing::warn;

/// Registry for managing multiple providers and model routing
#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    /// All providers indexed by name
    providers: HashMap<String, Provider>,

    /// Mapping from model name to provider name
    model_to_provider: HashMap<String, String>,
}

impl ProviderRegistry {
    /// Create a new provider registry from a list of providers
    pub fn new(providers: Vec<Provider>) -> Self {
        let mut provider_map = HashMap::new();
        let mut model_map = HashMap::new();

        for provider in providers {
            if provider.enabled {
                let name = provider.name.clone();
                for model in &provider.models {
                    // Warn if model is already mapped to a different provider
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
            providers: provider_map,
            model_to_provider: model_map,
        }
    }

    /// Get provider for a specific model name
    pub fn get_provider_for_model(&self, model: &str) -> Option<&Provider> {
        self.model_to_provider
            .get(model)
            .and_then(|name| self.providers.get(name))
    }

    /// Get a provider by name
    pub fn get_provider(&self, name: &str) -> Option<&Provider> {
        self.providers.get(name)
    }

    /// Get all enabled providers
    pub fn all_providers(&self) -> impl Iterator<Item = &Provider> {
        self.providers.values()
    }

    /// Get all available models across all providers
    pub fn all_models(&self) -> Vec<&String> {
        self.model_to_provider.keys().collect()
    }

    /// Build the full endpoint URL for a provider based on endpoint style
    pub fn build_endpoint_url(provider: &Provider, endpoint: &str) -> String {
        let base = provider.base_url.trim_end_matches('/');

        match provider.endpoint_style {
            EndpointStyle::OpenaiV1 => {
                // Standard OpenAI v1 style: base_url/v1/chat/completions
                format!("{}/{}", base, endpoint)
            }
            EndpointStyle::OpenaiDirect => {
                // Direct style without v1 prefix: base_url/chat/completions
                // Strip v1/ prefix from endpoint if present
                let clean_endpoint = endpoint.strip_prefix("v1/").unwrap_or(endpoint);
                format!("{}/{}", base, clean_endpoint)
            }
            EndpointStyle::Anthropic => {
                // Anthropic style: base_url/v1/messages
                // Always use /v1/messages endpoint
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
                EndpointType::Messages => "v1/messages", // Unusual but support it
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

    #[test]
    fn test_registry_creation() {
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

        assert!(registry.get_provider("zai-credit").is_some());
        assert!(registry.get_provider("aperture").is_some());
        assert!(registry.get_provider("unknown").is_none());
    }

    #[test]
    fn test_model_to_provider_mapping() {
        let providers = vec![create_test_provider(
            "test-provider",
            "https://api.example.com/api/paas/v4",
            EndpointStyle::OpenaiDirect,
            vec!["glm-5", "glm-4.7"],
        )];

        let registry = ProviderRegistry::new(providers);

        let provider = registry.get_provider_for_model("glm-5");
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().name, "test-provider");

        let provider = registry.get_provider_for_model("unknown-model");
        assert!(provider.is_none());
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

        let url = ProviderRegistry::build_endpoint_url(&provider, "v1/messages");
        assert_eq!(url, "http://100.100.100.100/v1/messages");
    }

    #[test]
    fn test_build_endpoint_url_openai_direct() {
        let provider = create_test_provider(
            "test-provider",
            "https://api.example.com/api/paas/v4",
            EndpointStyle::OpenaiDirect,
            vec!["test"],
        );

        // Should strip v1/ prefix
        let url = ProviderRegistry::build_endpoint_url(&provider, "v1/chat/completions");
        assert_eq!(url, "https://api.example.com/api/paas/v4/chat/completions");

        // Direct endpoint without v1/ prefix
        let url = ProviderRegistry::build_endpoint_url(&provider, "chat/completions");
        assert_eq!(url, "https://api.example.com/api/paas/v4/chat/completions");
    }

    #[test]
    fn test_build_endpoint_url_anthropic() {
        let provider = create_test_provider(
            "test-anthropic",
            "https://api.example.com/api/anthropic",
            EndpointStyle::Anthropic,
            vec!["test"],
        );

        // Always returns /v1/messages regardless of input endpoint
        let url = ProviderRegistry::build_endpoint_url(&provider, "v1/chat/completions");
        assert_eq!(url, "https://api.example.com/api/anthropic/v1/messages");
    }

    #[test]
    fn test_disabled_provider_not_included() {
        let mut provider =
            create_test_provider("disabled", "https://api.example.com", EndpointStyle::OpenaiV1, vec!["model-x"]);
        provider.enabled = false;

        let registry = ProviderRegistry::new(vec![provider]);

        assert!(registry.get_provider("disabled").is_none());
        assert!(registry.get_provider_for_model("model-x").is_none());
    }

    #[test]
    fn test_all_models() {
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
        let mut models: Vec<_> = registry.all_models();
        models.sort();

        assert_eq!(models, vec!["model-a", "model-b", "model-c"]);
    }

    #[test]
    fn test_get_default_endpoint() {
        let v1_provider = create_test_provider("v1", "http://test", EndpointStyle::OpenaiV1, vec![]);
        let direct_provider =
            create_test_provider("direct", "http://test", EndpointStyle::OpenaiDirect, vec![]);
        let anthropic_provider =
            create_test_provider("anthropic", "http://test", EndpointStyle::Anthropic, vec![]);

        assert_eq!(
            ProviderRegistry::get_default_endpoint(&v1_provider, EndpointType::ChatCompletions),
            "v1/chat/completions"
        );
        assert_eq!(
            ProviderRegistry::get_default_endpoint(&direct_provider, EndpointType::ChatCompletions),
            "chat/completions"
        );
        assert_eq!(
            ProviderRegistry::get_default_endpoint(&anthropic_provider, EndpointType::Messages),
            "v1/messages"
        );
    }
}
