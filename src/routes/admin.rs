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
    models: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
struct ModelInfo {
    id: String,
    owned_by: String,
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
        Ok(models) => {
            info!(
                request_id = %request_id,
                operation = "admin_refresh_models",
                status = "success",
                models_count = models.len(),
                model_ids = ?models.iter().map(|m| &m.id).collect::<Vec<_>>(),
                "Admin operation: Model refresh completed successfully"
            );

            let model_info: Vec<ModelInfo> = models
                .iter()
                .map(|m| ModelInfo {
                    id: m.id.clone(),
                    owned_by: m.owned_by.clone(),
                })
                .collect();

            let response = Json(RefreshResponse {
                success: true,
                message: format!("Successfully refreshed {} models", models.len()),
                models_count: models.len(),
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
            // Return generic error message to client (don't leak internal details)
            let error_response = Json(RefreshResponse {
                success: false,
                message: "Failed to refresh models from Aperture gateway".to_string(),
                models_count: 0,
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
    models: Vec<ModelInfo>,
    version: String,
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

    let discovery = &state.discovery;
    let models = discovery.get_models().await;

    let model_info: Vec<ModelInfo> = models
        .iter()
        .map(|m| ModelInfo {
            id: m.id.clone(),
            owned_by: m.owned_by.clone(),
        })
        .collect();

    info!(
        request_id = %request_id,
        operation = "admin_get_stats",
        status = "success",
        models_count = models.len(),
        "Admin operation: Stats retrieved successfully"
    );

    Json(StatsResponse {
        models_count: models.len(),
        models: model_info,
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
