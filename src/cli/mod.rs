// SPDX-License-Identifier: MIT
// Copyright (c) 2026 aperture-router contributors

//! CLI configuration tool for aperture-router
//!
//! Provides interactive wizard and CLI commands for:
//! - Fetching models from Aperture gateway
//! - Configuring providers and models
//! - Exporting to config.toml and opencode.json

pub mod commands;
pub mod model_fetcher;
pub mod opencode_export;
pub mod security;
pub mod wizard;
