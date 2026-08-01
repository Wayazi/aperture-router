# AGENTS.md

Context file for AI agents (Claude Code, OpenCode, Cursor, Copilot) working on this repository.

## Project Overview

**aperture-router** is a lightweight Rust proxy that lets any OpenAI/Anthropic-compatible AI tool use models behind a Tailscale Aperture gateway. It handles format conversion (Anthropic↔OpenAI), dynamic model discovery, multi-provider failover, and SSE streaming.

- **Language:** Rust 2021 edition
- **Framework:** Axum 0.8
- **Runtime:** Tokio (multi-thread)
- **TLS:** rustls (single backend, no OpenSSL)
- **Version:** 0.3.1
- **Binary size:** ~6.5 MB (stripped)
- **RSS:** ~4.2 MB idle

## Build & Test Commands

```bash
# Build (debug)
cargo build

# Build (release)
cargo build --release

# Build with wizard feature
cargo build --features wizard

# Run tests (230 tests)
cargo test

# Lint
cargo clippy --all-targets -- -D warnings

# Format check
cargo fmt --check

# Format fix
cargo fmt

# Run locally
APERTURE_BASE_URL=http://your-gateway:8080 cargo run

# Run with debug logging
cargo run -- --debug
```

## Architecture Summary

11 modules: `cli`, `config`, `discovery`, `http_client`, `middleware`, `provider`, `proxy`, `routes`, `security`, `server`, `types`.

**Request flow:** TCP → CORS → body limit → security headers → trace → request_id → auth middleware (rate limit → ban check → constant-time key validation) → handler → ProxyClient (SSRF check) → upstream.

**Key design decisions:**
- Single binary, no database, no external state
- `Arc<Config>` shared across all handlers (no cloning)
- Each caller builds its own `reqwest::Client` from `HttpConfig` (no global client)
- `CancellationToken` for graceful shutdown of 3 background tasks

## Key Conventions

- **Error format:** OpenAI errors from `/v1/chat/completions`, Anthropic errors from `/v1/messages`, generic JSON errors from `/v1/proxy`
- **Validation:** All limits are configurable via `SecurityConfig` — `max_messages`, `max_tokens`, `max_body_size_bytes`, `max_json_depth`, `max_streaming_size_bytes`
- **SSRF:** All upstream URLs validated — metadata blocklist, internal IP check, DNS rebinding protection, redirects disabled
- **Auth:** Constant-time comparison (`subtle::ct_eq`), `Zeroizing<String>` for key storage, IP banning with sliding window
- **No comments in code** unless explicitly requested
- **Conventional commits:** `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `perf:`, `ci:`

## Git Workflow

Two-branch workflow (see `docs/WORKFLOW.md`):

```
main    ← Production releases (protected, PR required)
  ↑
  └─ merge
dev     ← Development integration (direct commits allowed)
```

- Feature branches: `feature/your-feature` or `fix/your-fix` from `dev`
- PR target: `dev` (not `main`)
- Audit before pushing: `cargo test && cargo clippy -- -D warnings && cargo fmt --check`
- Release: merge `dev` → `main`, tag `vX.Y.Z`, push tag

## File Structure

```
src/           Source code (11 modules)
tests/         Integration tests (7 files, 230 tests total)
docs/          Documentation (Diátaxis framework)
contrib/       Systemd service files
aur/           AUR package (PKGBUILD, .SRCINFO)
scripts/       Build scripts
.github/       CI/CD workflows
config.example.toml  Example configuration
```

## Security Rules

- NEVER log API keys — use `hash_api_key()` or `SecretString`
- NEVER follow symlinks in config reads — use `safe_read_existing_file()`
- NEVER enable HTTP redirects — `redirect(Policy::none())` is SSRF-critical
- NEVER skip SSRF validation for upstream URLs
- Config files must be `0o600` permissions
- `APERTURE_API_KEY` must be scrubbed from env after loading

## Dependencies (27 direct)

Minimal feature sets. Key deps: `tokio`, `axum`, `reqwest` (rustls), `serde`, `clap`, `zeroize`, `subtle`, `sha2`. No `once_cell` (use std), no external `config` crate.
