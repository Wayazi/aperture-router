// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::ApertureConfig;
use crate::http_client::SHARED_CLIENT;

/// Model with provider metadata from Aperture
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
    /// Provider ID from Aperture (e.g., "glm", "glm2", "openrouter", or custom)
    #[serde(default)]
    pub provider_id: Option<String>,
}

/// API response from Aperture /v1/models
#[derive(Debug, Clone, Deserialize)]
struct ModelsResponse {
    data: Vec<ApiModel>,
}

/// Model from Aperture API
#[derive(Debug, Clone, Deserialize)]
struct ApiModel {
    id: String,
    #[serde(default)]
    object: Option<String>,
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    owned_by: Option<String>,
    /// Provider metadata from Aperture
    #[serde(default)]
    metadata: Option<ModelMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelMetadata {
    provider: Option<ProviderInfo>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderInfo {
    id: Option<String>,
}

/// Snapshot of current discovery state
#[derive(Debug, Clone, Serialize)]
pub struct DiscoverySnapshot {
    /// All available models
    pub models: Vec<Model>,
    /// Unique provider IDs detected (from Aperture, not hardcoded)
    pub providers: Vec<String>,
    /// Models grouped by provider
    pub models_by_provider: HashMap<String, Vec<String>>,
    /// When this snapshot was taken
    pub refreshed_at: u64,
}

/// Dynamic model discovery with auto-refresh
/// No hardcoded plans - everything comes from Aperture
pub struct ModelDiscovery {
    aperture_config: ApertureConfig,
    /// Current models indexed by ID
    models: Arc<RwLock<HashMap<String, Model>>>,
    /// Models grouped by provider
    models_by_provider: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Known provider IDs (dynamic, from Aperture)
    providers: Arc<RwLock<HashSet<String>>>,
    /// Last refresh timestamp (unix epoch seconds)
    last_refresh: Arc<RwLock<u64>>,
    /// Refresh interval in seconds
    refresh_interval_secs: u64,
}

impl ModelDiscovery {
    pub fn new(aperture_config: ApertureConfig) -> anyhow::Result<Self> {
        let refresh_interval_secs = aperture_config.model_refresh_interval_secs;

        Ok(Self {
            aperture_config,
            models: Arc::new(RwLock::new(HashMap::new())),
            models_by_provider: Arc::new(RwLock::new(HashMap::new())),
            providers: Arc::new(RwLock::new(HashSet::new())),
            last_refresh: Arc::new(RwLock::new(0)),
            refresh_interval_secs,
        })
    }

    /// Fetch models from Aperture and update internal state
    pub async fn fetch_models(&self) -> anyhow::Result<DiscoverySnapshot> {
        let base_url = Url::parse(&self.aperture_config.base_url)
            .map_err(|e| anyhow::anyhow!("Invalid base_url: {}", e))?;

        if !matches!(base_url.scheme(), "http" | "https") {
            return Err(anyhow::anyhow!(
                "Invalid URL scheme: {}. Only http and https are allowed.",
                base_url.scheme()
            ));
        }

        let url = base_url
            .join("v1/models")
            .map_err(|e| anyhow::anyhow!("Failed to build URL: {}", e))?;

        debug!("Fetching models from {}", url);

        // Use shared HTTP client for memory efficiency
        let response = SHARED_CLIENT.get(url.clone()).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error body".to_string());
            return Err(anyhow::anyhow!(
                "Failed to fetch models from {}: {} - {}",
                url,
                status,
                error_body
            ));
        }

        let models_response: ModelsResponse = response.json().await?;

        // Get model count for capacity hints (reduces HashMap reallocations)
        let model_count = models_response.data.len();

        // Process models and extract provider metadata from Aperture
        // Pre-allocate with capacity to avoid resize churn
        let mut new_models: HashMap<String, Model> = HashMap::with_capacity(model_count);
        let mut new_models_by_provider: HashMap<String, Vec<String>> = HashMap::new();
        let mut new_providers: HashSet<String> = HashSet::new();

        for api_model in models_response.data {
            // Extract provider ID from Aperture metadata (no hardcoded plans!)
            let provider_id = api_model
                .metadata
                .as_ref()
                .and_then(|m| m.provider.as_ref())
                .and_then(|p| p.id.clone())
                .unwrap_or_else(|| "default".to_string());

            let model = Model {
                id: api_model.id.clone(),
                object: api_model.object.unwrap_or_else(|| "model".to_string()),
                created: api_model.created.unwrap_or(0),
                owned_by: api_model.owned_by.unwrap_or_else(|| provider_id.clone()),
                provider_id: Some(provider_id.clone()),
            };

            new_models.insert(model.id.clone(), model);
            new_models_by_provider
                .entry(provider_id.clone())
                .or_default()
                .push(api_model.id);
            new_providers.insert(provider_id);
        }

        // Detect changes
        let (added, removed, provider_changes) = {
            let old_models = self.models.read().await;
            let old_providers = self.providers.read().await;

            let added: Vec<_> = new_models
                .keys()
                .filter(|k| !old_models.contains_key(*k))
                .cloned()
                .collect();

            let removed: Vec<_> = old_models
                .keys()
                .filter(|k| !new_models.contains_key(*k))
                .cloned()
                .collect();

            let added_providers: Vec<_> = new_providers
                .difference(&old_providers)
                .cloned()
                .collect();

            let removed_providers: Vec<_> = old_providers
                .difference(&new_providers)
                .cloned()
                .collect();

            (added, removed, (added_providers, removed_providers))
        };

        // Log changes
        if !added.is_empty() {
            info!("✨ New models detected: {:?}", added);
        }
        if !removed.is_empty() {
            info!("🗑️  Models removed: {:?}", removed);
        }
        if !provider_changes.0.is_empty() {
            info!("📦 New providers detected: {:?}", provider_changes.0);
        }
        if !provider_changes.1.is_empty() {
            info!("📦 Providers removed: {:?}", provider_changes.1);
        }

        // Update state
        {
            let mut models = self.models.write().await;
            *models = new_models;
        }
        {
            let mut by_provider = self.models_by_provider.write().await;
            *by_provider = new_models_by_provider;
        }
        {
            let mut providers = self.providers.write().await;
            *providers = new_providers;
        }
        {
            let mut last_refresh = self.last_refresh.write().await;
            *last_refresh = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }

        info!("Discovered {} models from {} providers",
            model_count,
            self.providers.read().await.len()
        );

        Ok(self.get_snapshot().await)
    }

    /// Get current snapshot of discovery state
    pub async fn get_snapshot(&self) -> DiscoverySnapshot {
        let models = self.models.read().await;
        let providers = self.providers.read().await;
        let models_by_provider = self.models_by_provider.read().await;
        let last_refresh = self.last_refresh.read().await;

        DiscoverySnapshot {
            models: models.values().cloned().collect(),
            providers: providers.iter().cloned().collect(),
            models_by_provider: models_by_provider.clone(),
            refreshed_at: *last_refresh,
        }
    }

    /// Get all models as a vector
    pub async fn get_models(&self) -> Vec<Model> {
        self.models.read().await.values().cloned().collect()
    }

    /// Get all provider IDs (dynamic, from Aperture)
    pub async fn get_providers(&self) -> Vec<String> {
        self.providers.read().await.iter().cloned().collect()
    }

    /// Get models for a specific provider
    pub async fn get_models_for_provider(&self, provider_id: &str) -> Vec<String> {
        self.models_by_provider
            .read()
            .await
            .get(provider_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Check if a model ID is valid
    pub async fn is_valid_model(&self, model_id: &str) -> bool {
        self.models.read().await.contains_key(model_id)
    }

    /// Get model by ID
    pub async fn get_model(&self, model_id: &str) -> Option<Model> {
        self.models.read().await.get(model_id).cloned()
    }

    /// Get the number of discovered models
    pub async fn model_count(&self) -> usize {
        self.models.read().await.len()
    }

    /// Start background refresh task with optional ProviderRegistry sync
    /// When registry is provided, it will be updated on each refresh
    /// Uses CancellationToken for graceful shutdown
    pub fn start_refresh_task(
        self: Arc<Self>,
        registry: Option<Arc<crate::ProviderRegistry>>,
        shutdown_token: tokio_util::sync::CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let interval_secs = self.refresh_interval_secs;
        let aperture_url = self.aperture_config.base_url.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            // Fire immediately on startup, don't wait for first interval
            interval.tick().await;

            loop {
                tokio::select! {
                    _ = shutdown_token.cancelled() => {
                        info!("Model discovery refresh task shutting down gracefully");
                        break;
                    }
                    _ = interval.tick() => {
                        match self.fetch_models().await {
                            Ok(snapshot) => {
                                debug!(
                                    "Auto-refreshed {} models from {} providers",
                                    snapshot.models.len(),
                                    snapshot.providers.len()
                                );

                                // Sync with ProviderRegistry if provided
                                if let Some(ref reg) = registry {
                                    reg.update_from_discovery(&snapshot.models_by_provider, &aperture_url)
                                        .await;
                                }
                            }
                            Err(e) => {
                                warn!("Failed to auto-refresh models: {}", e);
                            }
                        }
                    }
                }
            }
        })
    }
}
