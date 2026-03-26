// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use tracing::{info, warn};
use uuid::Uuid;

use crate::server::AppState;

#[derive(Debug, Serialize)]
struct RefreshResponse {
    success: bool,
    message: String,
    models_count: usize,
    providers_count: usize,
    providers: Vec<String>,
    models: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
struct ModelInfo {
    id: String,
    provider: Option<String>,
}

/// POST /admin/refresh-models
///
/// Manually refresh the model cache from the Aperture gateway.
/// This is useful when models are added/removed on the gateway
/// and you want to update the router without restarting.
///
/// **Authentication:** Requires admin API key
///
/// **Response:** JSON with success status, message, and updated model list
#[must_use]
pub async fn refresh_models(State(state): State<AppState>) -> impl IntoResponse {
    let request_id = Uuid::new_v4();

    info!(
        request_id = %request_id,
        operation = "admin_refresh_models",
        status = "started",
        "Admin operation: Model refresh requested"
    );

    let discovery = &state.discovery;

    match discovery.fetch_models().await {
        Ok(snapshot) => {
            info!(
                request_id = %request_id,
                operation = "admin_refresh_models",
                status = "success",
                models_count = snapshot.models.len(),
                providers_count = snapshot.providers.len(),
                "Admin operation: Model refresh completed successfully"
            );

            // Update provider registry with discovered models
            state.provider_registry
                .update_from_discovery(&snapshot.models_by_provider, &state.config.aperture.base_url)
                .await;

            let model_info: Vec<ModelInfo> = snapshot.models
                .iter()
                .map(|m| ModelInfo {
                    id: m.id.clone(),
                    provider: m.provider_id.clone(),
                })
                .collect();

            let response = Json(RefreshResponse {
                success: true,
                message: format!(
                    "Successfully refreshed {} models from {} providers",
                    snapshot.models.len(),
                    snapshot.providers.len()
                ),
                models_count: snapshot.models.len(),
                providers_count: snapshot.providers.len(),
                providers: snapshot.providers,
                models: model_info,
            });
            (StatusCode::OK, response).into_response()
        }
        Err(e) => {
            warn!(
                request_id = %request_id,
                operation = "admin_refresh_models",
                status = "failed",
                error = ?e,
                "Admin operation: Model refresh failed"
            );
            let error_response = Json(RefreshResponse {
                success: false,
                message: "Failed to refresh models from Aperture gateway".to_string(),
                models_count: 0,
                providers_count: 0,
                providers: Vec::new(),
                models: Vec::new(),
            });
            (StatusCode::INTERNAL_SERVER_ERROR, error_response).into_response()
        }
    }
}

/// GET /admin/stats
///
/// Get statistics about the router including model count and uptime.
///
/// **Authentication:** Requires admin API key
///
/// **Response:** JSON with router statistics
#[derive(Debug, Serialize)]
struct StatsResponse {
    models_count: usize,
    providers_count: usize,
    providers: Vec<ProviderStats>,
    version: String,
    refresh_interval_secs: u64,
}

#[derive(Debug, Serialize)]
struct ProviderStats {
    name: String,
    models_count: usize,
}

#[must_use]
pub async fn get_stats(State(state): State<AppState>) -> impl IntoResponse {
    let request_id = Uuid::new_v4();

    info!(
        request_id = %request_id,
        operation = "admin_get_stats",
        status = "started",
        "Admin operation: Stats requested"
    );

    let snapshot = state.discovery.get_snapshot().await;

    let provider_stats: Vec<ProviderStats> = snapshot.models_by_provider
        .iter()
        .map(|(name, models)| ProviderStats {
            name: name.clone(),
            models_count: models.len(),
        })
        .collect();

    info!(
        request_id = %request_id,
        operation = "admin_get_stats",
        status = "success",
        models_count = snapshot.models.len(),
        "Admin operation: Stats retrieved successfully"
    );

    Json(StatsResponse {
        models_count: snapshot.models.len(),
        providers_count: snapshot.providers.len(),
        providers: provider_stats,
        version: env!("CARGO_PKG_VERSION").to_string(),
        refresh_interval_secs: state.config.aperture.model_refresh_interval_secs,
    })
}
