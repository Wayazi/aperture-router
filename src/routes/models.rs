use axum::{extract::State, response::IntoResponse, Json};
use serde_json::json;

use crate::server::AppState;

pub async fn models(State((_, _, _, discovery, _)): State<AppState>) -> impl IntoResponse {
    let discovery = discovery.read().await;
    let models = discovery.get_models();
    Json(json!({
        "object": "list",
        "data": *models
    }))
}
