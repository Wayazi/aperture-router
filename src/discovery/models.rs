// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::config::ApertureConfig;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelsResponse {
    pub object: String,
    pub data: Vec<Model>,
}

pub struct ModelDiscovery {
    client: Client,
    aperture_config: ApertureConfig,
    models: Arc<RwLock<Vec<Model>>>,
}

impl ModelDiscovery {
    pub fn new(aperture_config: ApertureConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client for model discovery"); // OK to panic here, happens at startup

        Self {
            client,
            aperture_config,
            models: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn fetch_models(&self) -> anyhow::Result<Vec<Model>> {
        // Validate and parse base URL
        let base_url = Url::parse(&self.aperture_config.base_url)
            .map_err(|e| anyhow::anyhow!("Invalid base_url: {}", e))?;

        // Validate scheme (allow only http/https)
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

        let response = self.client.get(url.clone()).send().await?;

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

        info!("Discovered {} models", models_response.data.len());

        // Update the models using internal mutability
        let mut models = self.models.write().await;
        *models = models_response.data.clone();
        Ok(models.clone())
    }

    pub async fn get_models(&self) -> Vec<Model> {
        self.models.read().await.clone()
    }

    /// Check if a model ID is valid (exists in discovered models)
    pub async fn is_valid_model(&self, model_id: &str) -> bool {
        let models = self.models.read().await;
        models.iter().any(|m| m.id == model_id)
    }

    /// Get the number of discovered models
    pub async fn model_count(&self) -> usize {
        self.models.read().await.len()
    }
}
