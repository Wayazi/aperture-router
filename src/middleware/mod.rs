// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

pub mod auth;

pub use auth::{admin_auth_middleware, auth_middleware, AuthState};
