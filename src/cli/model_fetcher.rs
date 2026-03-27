// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! Model fetching with provider metadata
//!
//! Fetches models from Aperture gateway with provider info.
//! No hardcoded plans - everything is dynamic from Aperture.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Enriched model with provider metadata (dynamic, no hardcoded plans)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedModel {
    /// Original model ID from Aperture
    pub id: String,
    /// Provider ID from Aperture (dynamic)
    pub provider_id: String,
    /// Canonical ID with provider prefix (e.g., "glm/GLM-5")
    pub canonical_id: String,
    /// Display name for UI
    pub display_name: String,
}

/// Model response from Aperture /v1/models
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ApertureModel>,
}

#[derive(Debug, Deserialize)]
struct ApertureModel {
    id: String,
    #[serde(default)]
    metadata: Option<ModelMetadata>,
}

#[derive(Debug, Deserialize)]
struct ModelMetadata {
    provider: Option<ProviderInfo>,
}

#[derive(Debug, Deserialize)]
struct ProviderInfo {
    id: Option<String>,
}

/// Generate display name for UI
fn generate_display_name(model_id: &str, provider_id: &str) -> String {
    format!("{} [{}]", model_id, provider_id)
}

/// Fetch models from Aperture and enrich with metadata
pub async fn fetch_models(base_url: &str) -> anyhow::Result<Vec<EnrichedModel>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch models: {}", e))?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch models: HTTP {}",
            response.status()
        ));
    }

    let models_response: ModelsResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse models response: {}", e))?;

    // Enrich models with provider metadata (dynamic, no hardcoded plans)
    let enriched: Vec<EnrichedModel> = models_response
        .data
        .into_iter()
        .map(|model| {
            let provider_id = model
                .metadata
                .as_ref()
                .and_then(|m| m.provider.as_ref())
                .and_then(|p| p.id.clone())
                .unwrap_or_else(|| "default".to_string());

            let canonical_id = format!("{}/{}", provider_id, model.id);
            let display_name = generate_display_name(&model.id, &provider_id);

            EnrichedModel {
                id: model.id,
                provider_id,
                canonical_id,
                display_name,
            }
        })
        .collect();

    Ok(enriched)
}

/// Group models by provider
pub fn group_by_provider(models: &[EnrichedModel]) -> HashMap<String, Vec<&EnrichedModel>> {
    let mut grouped: HashMap<String, Vec<&EnrichedModel>> = HashMap::new();

    for model in models {
        grouped
            .entry(model.provider_id.clone())
            .or_default()
            .push(model);
    }

    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_display_name() {
        assert_eq!(generate_display_name("GLM-5", "glm"), "GLM-5 [glm]");
        assert_eq!(
            generate_display_name("glm-4.7", "custom-provider"),
            "glm-4.7 [custom-provider]"
        );
    }
}
