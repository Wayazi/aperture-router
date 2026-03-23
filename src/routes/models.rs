use axum::{extract::State, response::IntoResponse, Json};
use serde_json::json;

use crate::server::AppState;

pub async fn models(State(state): State<AppState>) -> impl IntoResponse {
    let models = state.discovery.get_models().await;
    Json(json!({
        "object": "list",
        "data": models
    }))
}
