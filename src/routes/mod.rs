// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

pub mod admin;
pub mod chat;
pub mod error;
pub mod health;
pub mod messages;
pub mod models;
pub mod proxy;
pub mod streaming;

pub use error::error_response;

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use tracing::warn;

/// Validates model name and returns error response if invalid
pub fn validate_model_or_error<T: proxy::HasModel>(
    request: &T,
) -> Result<(), Box<axum::response::Response>> {
    let model = request.model();

    if let Err(e) = crate::types::validation::validate_model_name(model) {
        warn!("Invalid model name format: {}", e);
        return Err(Box::new(
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": e,
                        "type": "invalid_request_error",
                        "code": "invalid_model_name"
                    }
                })),
            )
                .into_response(),
        ));
    }

    Ok(())
}
