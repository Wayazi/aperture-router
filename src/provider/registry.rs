// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use crate::config::{EndpointStyle, Provider};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[derive(Debug)]
struct RegistryInner {
    providers: HashMap<String, Provider>,
    model_to_provider: HashMap<String, String>,
    /// Providers defined in the user's config file. They must survive
    /// discovery refreshes even when the gateway snapshot doesn't mention
    /// them, and their model mappings always take precedence over
    /// discovered ones.
    static_providers: HashSet<String>,
    /// Config providers explicitly disabled with `enabled = false`. Their
    /// names are reserved: discovery must never auto-add an enabled provider
    /// under a name the user deliberately turned off.
    disabled_static_providers: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    inner: Arc<RwLock<RegistryInner>>,
    aperture_base_url: String,
}

impl ProviderRegistry {
    pub fn new(providers: Vec<Provider>) -> Self {
        let mut provider_map = HashMap::new();
        let mut model_map = HashMap::new();
        let mut static_providers = HashSet::new();
        let mut disabled_static_providers = HashSet::new();

        for provider in providers {
            let name = provider.name.clone();
            if provider.enabled {
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
                static_providers.insert(name.clone());
                provider_map.insert(name, provider);
            } else {
                disabled_static_providers.insert(name);
            }
        }

        Self {
            inner: Arc::new(RwLock::new(RegistryInner {
                providers: provider_map,
                model_to_provider: model_map,
                static_providers,
                disabled_static_providers,
            })),
            aperture_base_url: String::new(),
        }
    }

    pub fn with_aperture_url(providers: Vec<Provider>, aperture_url: String) -> Self {
        let mut registry = Self::new(providers);
        registry.aperture_base_url = aperture_url;
        registry
    }

    pub async fn update_from_discovery(
        &self,
        models_by_provider: &HashMap<String, Vec<String>>,
        aperture_url: &str,
    ) {
        let mut inner = self.inner.write().await;

        // Track which providers are in this update
        let active_providers: std::collections::HashSet<_> =
            models_by_provider.keys().cloned().collect();

        // Remove stale discovered providers, but never remove providers that
        // came from the user's config file — the gateway snapshot may not
        // mention them (e.g. custom endpoint-style overrides), yet they must
        // keep serving their configured models.
        let static_providers = inner.static_providers.clone();
        let previous_count = inner.providers.len();
        inner
            .providers
            .retain(|name, _| active_providers.contains(name) || static_providers.contains(name));
        inner.model_to_provider.retain(|_, provider| {
            active_providers.contains(provider) || static_providers.contains(provider)
        });

        let removed_count = previous_count - inner.providers.len();
        if removed_count > 0 {
            info!("Removed {} stale providers from registry", removed_count);
        }

        for (provider_id, model_ids) in models_by_provider {
            if inner.disabled_static_providers.contains(provider_id) {
                // The user explicitly disabled a config provider with this
                // name — never resurrect it as an enabled auto-provider.
                debug!(
                    "Skipping discovery group '{}' — disabled in config",
                    provider_id
                );
                continue;
            }

            let provider_exists = inner.providers.contains_key(provider_id);

            if !provider_exists {
                // Infer endpoint style for auto-added providers: namespaced
                // model IDs ("vendor/model") indicate aggregator upstreams
                // (OpenRouter-style) that serve standard /v1/ paths, while
                // bare IDs (e.g. glm-5.x behind Aperture) expect the direct
                // no-prefix style. Majority wins for mixed groups; explicit
                // [[providers]] config always overrides this default.
                let namespaced =
                    model_ids.iter().filter(|m| m.contains('/')).count() * 2 > model_ids.len();
                let endpoint_style = if namespaced {
                    EndpointStyle::OpenaiV1
                } else {
                    EndpointStyle::OpenaiDirect
                };

                let new_provider = Provider {
                    name: provider_id.clone(),
                    base_url: aperture_url.to_string(),
                    api_key: None,
                    endpoint_style,
                    models: model_ids.clone(),
                    enabled: true,
                };

                inner.providers.insert(provider_id.clone(), new_provider);
                info!(
                    "Auto-added provider {:?} with {} models",
                    provider_id,
                    model_ids.len()
                );
            } else if !inner.static_providers.contains(provider_id) {
                // Auto-added provider seen again: replace its model list with
                // the current snapshot so removed models stop routing instead
                // of accumulating forever. (Static providers are never
                // touched here — their curated lists are config-owned.)
                if let Some(provider) = inner.providers.get_mut(provider_id) {
                    provider.models = model_ids.clone();
                }
            } else {
                debug!(
                    "Discovery group {:?} collides with static provider — leaving its model list untouched",
                    provider_id
                );
            }

            // Insert discovered models into the routing map. Mappings that
            // already exist come from the user's config file (discovery runs
            // after startup) and always win — an auto-added provider uses a
            // default endpoint style that may not suit every upstream, so a
            // deliberate static route must never be silently replaced.
            for model_id in model_ids {
                match inner.model_to_provider.get(model_id) {
                    Some(existing) if existing != provider_id => {
                        debug!(
                            "Keeping static route for model '{}' -> '{}' (discovery suggested '{}')",
                            model_id, existing, provider_id
                        );
                    }
                    Some(_) => {}
                    None => {
                        inner
                            .model_to_provider
                            .insert(model_id.clone(), provider_id.clone());
                    }
                }
            }
        }

        // Ensure manually-configured models (not in discovery) are also routable
        let manual_entries: Vec<(String, String)> = inner
            .providers
            .iter()
            .flat_map(|(provider_id, provider)| {
                provider
                    .models
                    .iter()
                    .filter(|m| !inner.model_to_provider.contains_key(*m))
                    .map(|m| (m.clone(), provider_id.clone()))
            })
            .collect();

        for (model_id, provider_id) in manual_entries {
            inner.model_to_provider.insert(model_id, provider_id);
        }

        let all_valid_models: std::collections::HashSet<String> = inner
            .providers
            .values()
            .flat_map(|p| p.models.iter().cloned())
            .collect();

        let known_providers: std::collections::HashSet<String> =
            inner.providers.keys().cloned().collect();

        inner.model_to_provider.retain(|model, provider_id| {
            all_valid_models.contains(model) && known_providers.contains(provider_id)
        });

        let total_models = inner.model_to_provider.len();
        let total_providers = inner.providers.len();

        info!(
            "Registry updated: {} providers, {} models",
            total_providers, total_models
        );
    }

    pub async fn get_provider_for_model(&self, model: &str) -> Option<Provider> {
        let inner = self.inner.read().await;
        inner
            .model_to_provider
            .get(model)
            .and_then(|name| inner.providers.get(name).cloned())
    }

    pub async fn get_provider(&self, name: &str) -> Option<Provider> {
        self.inner.read().await.providers.get(name).cloned()
    }

    pub async fn get_providers_for_model(&self, model: &str) -> Vec<Provider> {
        let inner = self.inner.read().await;
        let mut result = Vec::new();

        if let Some(name) = inner.model_to_provider.get(model) {
            if let Some(provider) = inner.providers.get(name) {
                if provider.enabled {
                    result.push(provider.clone());
                }
            }
        }

        for provider in inner.providers.values() {
            if provider.enabled
                && provider.models.iter().any(|m| m == model)
                && !result.iter().any(|r| r.name == provider.name)
            {
                result.push(provider.clone());
            }
        }

        result
    }

    pub async fn all_providers(&self) -> Vec<Provider> {
        self.inner
            .read()
            .await
            .providers
            .values()
            .cloned()
            .collect()
    }

    pub async fn all_models(&self) -> Vec<String> {
        self.inner
            .read()
            .await
            .model_to_provider
            .keys()
            .cloned()
            .collect()
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointType {
    ChatCompletions,
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

    #[tokio::test]
    async fn test_static_provider_survives_discovery_refresh() {
        // A config-file provider whose models the gateway snapshot doesn't
        // mention must NOT be dropped on refresh.
        let registry = ProviderRegistry::new(vec![create_test_provider(
            "custom-override",
            "https://api.example.com",
            EndpointStyle::OpenaiV1,
            vec!["vendor/model-x"],
        )]);

        let mut discovered = HashMap::new();
        discovered.insert("GLM".to_string(), vec!["glm-5.2".to_string()]);

        registry
            .update_from_discovery(&discovered, "http://gateway.example")
            .await;

        assert!(registry.get_provider("custom-override").await.is_some());
        assert!(registry
            .get_provider_for_model("vendor/model-x")
            .await
            .is_some());
    }

    #[tokio::test]
    async fn test_static_model_mapping_beats_discovery() {
        // Static route: vendor/model-x -> my-provider (OpenaiV1).
        // Discovery reports the same model under group 'agg' (auto-added).
        // The static mapping must win so the custom endpoint style is used.
        let registry = ProviderRegistry::new(vec![create_test_provider(
            "my-provider",
            "https://api.example.com",
            EndpointStyle::OpenaiV1,
            vec!["vendor/model-x"],
        )]);

        let mut discovered = HashMap::new();
        discovered.insert("agg".to_string(), vec!["vendor/model-x".to_string()]);

        registry
            .update_from_discovery(&discovered, "http://gateway.example")
            .await;

        let provider = registry
            .get_provider_for_model("vendor/model-x")
            .await
            .expect("model must remain routable");
        assert_eq!(provider.name, "my-provider");
        assert_eq!(provider.endpoint_style, EndpointStyle::OpenaiV1);
    }

    #[tokio::test]
    async fn test_auto_added_provider_endpoint_style_inference() {
        // Namespaced model IDs (aggregator upstreams) auto-add with
        // OpenaiV1; bare IDs (Aperture GLM-style) with OpenaiDirect.
        let mut discovered = HashMap::new();
        discovered.insert(
            "Openrouter".to_string(),
            vec![
                "stealth/ox-alpha".to_string(),
                "openrouter/free".to_string(),
            ],
        );
        discovered.insert("GLM".to_string(), vec!["glm-5.2".to_string()]);

        let registry =
            ProviderRegistry::with_aperture_url(vec![], "http://gateway.example".to_string());
        registry
            .update_from_discovery(&discovered, "http://gateway.example")
            .await;

        let agg = registry.get_provider("Openrouter").await.unwrap();
        assert_eq!(agg.endpoint_style, EndpointStyle::OpenaiV1);

        let glm = registry.get_provider("GLM").await.unwrap();
        assert_eq!(glm.endpoint_style, EndpointStyle::OpenaiDirect);
    }
}
