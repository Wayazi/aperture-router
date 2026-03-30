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
pub mod openclaw_export;
pub mod opencode_export;
pub mod security;

#[cfg(feature = "wizard")]
pub mod wizard;

/// System-wide config path (used when running as root/sudo or with --system)
pub const SYSTEM_CONFIG_PATH: &str = "/etc/aperture-router/config.toml";

/// Check if running with elevated privileges (root or sudo)
///
/// This is used to determine whether to use the system-wide config path
/// by default. Uses environment variables for cross-platform compatibility.
///
/// # Returns
/// - `true` if running under sudo (SUDO_USER is set)
/// - `true` if running as root (USER=root)
/// - `false` otherwise
pub fn is_running_elevated() -> bool {
    std::env::var("SUDO_USER").is_ok()
        || std::env::var("USER").map(|u| u == "root").unwrap_or(false)
}
