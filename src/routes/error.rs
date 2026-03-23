// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

use axum::http::StatusCode;
use serde::Serialize;

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub fn error_response(status: StatusCode, message: &str) -> (StatusCode, Vec<u8>) {
    let error = ErrorResponse {
        error: message.to_string(),
    };

    let body = serde_json::to_vec(&error);
    let body = match body {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to serialize error response: {}", e);
            b"Internal server error".to_vec()
        }
    };

    (status, body)
}
