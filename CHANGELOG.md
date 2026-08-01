# Changelog

All notable changes to aperture-router will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.2] - 2026-08-01

### Fixed
- **UTF-8 stream buffering** - Multi-byte UTF-8 characters (emojis, international text) split across TCP chunks caused `Stream interrupted` errors. Fixed by buffering partial UTF-8 sequences across chunks using a carry-over buffer in `make_utf8_stream()`.

### Added
- **Complete documentation** - Diátaxis framework docs structure (21 files): tutorials, how-to guides, reference, explanation, troubleshooting.
- **AGENTS.md** - Context file for AI agents working on the repo.
- **CONTRIBUTING.md** - Contribution guide.

### Changed
- **Docs audit fixes** - Corrected env var names (`APERTURE_HOST`/`APERTURE_PORT` not `APERTURE_ROUTER_*`), removed non-existent `[server]`/`[logging]` config tables, fixed health endpoint response format, fixed `openai_direct` messages URL, fixed `endpoint_style` casing, removed misleading `${ENV_VAR}` interpolation examples.

## [0.3.1] - 2026-07-31

### Fixed
- **Configurable `max_messages`** - `MAX_MESSAGES` was hardcoded to 1000 in all three route handlers (`/v1/chat/completions`, `/v1/messages`, `/v1/proxy`), causing `400 too_many_messages` errors for long agentic sessions that never cooled off. Now configurable via `security.max_messages` in config (default 10000).
- **RateLimiter config wired** - `RateLimiter` now uses `config.rate_limit.burst_size` and `requests_per_second` instead of hardcoded 100/60s.
- **`validate_max_tokens` no longer dead code** - Wired into all 3 route handlers; limit constant moved to `validation.rs`.
- **Discovery uses config-aware HTTP client** - `ModelDiscovery` now builds its own `reqwest::Client` from `HttpConfig` instead of using `SHARED_CLIENT` with hardcoded timeouts.
- **Streaming endpoint returns JSON error bodies** - Replaced bare `StatusCode` returns with structured JSON error responses.
- **Auth header on discovery** - `x-api-key` header now sent to Aperture `/v1/models` when `aperture.api_key` is configured.
- **Discovery merges models** - `update_from_discovery` now merges discovered models with existing configured models instead of replacing; manually-configured models preserved in both `provider.models` and `model_to_provider` routing map.
- **Conversion: tool_result images preserved** - Structured/image content in tool results is now passed through instead of flattened to text.
- **Conversion: stop_sequence propagated** - Now reads `usage.stop_sequence` from upstream instead of hardcoding null.
- **Conversion: output_tokens uses upstream value** - Streaming converter reads `completion_tokens` when available instead of using a per-delta heuristic.
- **Conversion: metadata.user_id mapped** - Mapped to OpenAI's `user` field instead of being stripped.
- **Conversion: cache_control stripped** - Removed from both top-level and individual content blocks to prevent upstream rejection.
- **Security: quinn-proto updated** - 0.11.14 → 0.11.16 (CVE: remote memory exhaustion).

### Added
- **Anthropic-direct SSE streaming** - When `stream:true` and an Anthropic-style provider exists, requests now use true SSE passthrough instead of buffering the full response.
- **Discovery retry with backoff** - 3 attempts with exponential backoff on server errors and connection failures.

### Changed
- **Admin keys valid for regular endpoints** - `is_enabled()` returns true when only admin keys are configured; `validate_api_key()` checks both regular and admin keys.
- **Model alias resolution** - `resolve_model_alias()` in all 3 routes before validation.

### Performance
- **RSS reduced 35%** - From 6.4MB to 4.2MB by removing `SHARED_CLIENT`/`once_cell` and using `Arc<Config>`.
- **5 dead dependencies removed** - `config`, `once_cell`, `tokio-stream`, `tokio-test`, `mockall`.

## [0.3.0] - 2026-06-12

### Added
- **Anthropic ↔ OpenAI Format Conversion** - `/v1/messages` now converts Anthropic requests to OpenAI format and responses back, enabling any OpenAI-compatible provider to serve Anthropic clients
- **Stream Converter** - Real-time SSE conversion from OpenAI streaming chunks to Anthropic streaming events
- **Per-Client Rate Limiter** - Separate request rate limiting middleware (`RateLimiter`) with configurable limits and memory cap (10K IPs)
- **Session ID Tracking** - `X-Session-ID` header for grouping requests across a client session
- **RouterHandles Struct** - Clean return type from `create_router()` replacing complex tuple
- **Background Task Shutdown** - All cleanup tasks now respect `CancellationToken` for graceful shutdown
- **CLI API Key Warning** - Warns user before displaying generated API key
- **Referrer-Policy Header** - `strict-origin-when-cross-origin`
- **Permissions-Policy Header** - `camera=(), microphone=(), geolocation=()`
- **`is_internal_ip_strict_host`** - String-based strict IP check for provider URL validation

### Changed
- **SSRF Functions Consolidated** - `src/security/mod.rs` is now the single source of truth; duplicates removed from `proxy/client.rs`, `config.rs`, and `cli/security.rs`
- **`is_blocked_host` Enhanced** - Now includes trailing dot normalization (RFC 1034) and broader Kubernetes metadata DNS patterns
- **`max_tokens` Type Changed** - Anthropic `MessageRequest.max_tokens` changed from `u32` (defaulting to 0) to `Option<u32>` — absent is now valid
- **Auth State Refactored** - `check_and_record_failure()` split into `is_banned()` + `record_failure()` for clearer flow
- **Dependencies Updated** - axum 0.8.9, tokio 1.52.3, tower-http 0.6.11, and 40+ other crates updated
- **Stream Buffer Overflow Protection** - 1MB line buffer limit in stream converter

### Security
- **Timing-Safe Comparison Verified** - Bitwise OR with `u8` confirmed constant-time (no short-circuit)
- **Admin Auth Hardened** - Dev mode bypass (`APERTURE_ALLOW_DEV_ADMIN`) removed from admin middleware
- **SSRF Deduplication** - Eliminated 3 copies of SSRF functions with different IPv6 link-local detection logic; canonical version uses `is_unicast_link_local()`
- **Yanked Dependency Fixed** - `unicode-segmentation` updated from v1.13.1 (yanked) to v1.13.3
- **`await_holding_lock` Fixed** - Mutex guards dropped before `.await` points in shutdown handler

### Tests
- **22 Auth Tests** - Including constant-time comparison, admin key separation, per-IP tracking
- **18 Integration Tests** - Session ID, CORS, auth flows, concurrent requests
- **77 Library Tests** - All passing

## [0.2.0] - 2026-03-26

### Added
- **Dynamic Model Discovery** - Models and providers are now auto-discovered from Aperture at runtime
- **Auto-refresh Background Task** - Model list refreshes automatically with configurable interval
- **Interactive CLI Wizard** - Run `aperture-router config wizard` for guided configuration
- **OpenCode Export** - Export config to OpenCode format with `aperture-router config export --opencode`
- **Admin API Endpoints** - `/admin/stats` and `/admin/refresh-models` for monitoring and control
- **Graceful Shutdown** - Proper CancellationToken-based shutdown for all background tasks
- **CancellationToken Architecture** - Clean task termination on SIGINT/SIGTERM
- **5 New Config Save Tests** - Verifying atomic writes and secure file permissions

### Changed
- **Removed Hardcoded ProviderPlan** - No more "coding"/"credit" plan naming, fully dynamic
- **API Keys Use Zeroizing** - Keys are securely wiped from memory on drop
- **Timing-Safe Key Validation** - All keys compared to prevent timing attacks
- **Config File Permissions** - Files created with 0o600 from the start (no race condition)
- **Registry Sync on Refresh** - ProviderRegistry stays in sync with ModelDiscovery
- **Immediate Initial Refresh** - Models available immediately on startup

### Security
- **Fixed cfg!(debug_assertions) Logic** - Production auth check now triggers correctly
- **Secure Temp File Creation** - Uses OpenOptions with mode 0o600 from creation
- **Zeroizing<String> for Secrets** - API keys wiped from memory securely
- **Timing-Attack Resistant Auth** - Compares all keys without short-circuit
- **SSRF Protection Enhanced** - Metadata endpoint blocking for AWS/GCP/Azure

### Tests
- **151 Total Tests** - All passing (up from 114)
- **Config Save Permission Tests** - Verify 0o600 permissions on Unix
- **Atomic Write Tests** - Verify no temp file left after save

## [0.1.0] - 2026-03-23

### Added
- Universal AI router for Tailscale Aperture
- OpenAI `/v1/chat/completions` API compatibility
- Anthropic `/v1/messages` API compatibility
- Model discovery and caching from Aperture gateway
- SSE streaming support for real-time responses
- Tool/function calling support (OpenAI tool_calls and Anthropic tool_use)
- Extended thinking blocks (with filtering option)
- Model validation against available models
- Authentication with API keys
- Rate limiting for authentication attempts
- SSRF protection (blocks internal IPs and metadata endpoints)
- Security headers (CSP, X-Frame-Options, X-XSS-Protection, HSTS)
- Request/response size limits
- JSON depth validation to prevent DoS
- Configuration via TOML file or environment variables
- Health check endpoint
- Comprehensive test suite (114 tests)

### Security
- Constant-time API key comparison using `subtle` crate
- API key strength validation (32 char min, 20 unique chars)
- IP-based rate limiting with automatic cleanup
- Trusted proxy IP validation
- HTTPS enforcement when API keys are configured
- CORS production validation

### Documentation
- README with quick start guide
- Example configuration file
- Systemd service files
- AUR package build files
- GitHub release workflow

## Installation

### Cargo
```bash
cargo install aperture-router
```

### AUR (Arch Linux)
```bash
yay -S aperture-router
```

### Binary
Download from [Releases](https://github.com/Wayazi/aperture-router/releases)

## Configuration

Create a `config.toml`:

```toml
host = "127.0.0.1"
port = 8765

[aperture]
base_url = "http://100.100.100.100"  # Your Aperture gateway

[security]
api_keys = ["your-api-key-here"]  # Optional: Enable authentication
```

Or use environment variables:

```bash
export APERTURE_BASE_URL=http://100.100.100.100
export APERTURE_PORT=8080
export APERTURE_API_KEY=your-api-key-here
```

## Usage

```bash
aperture-router
```

With custom config:

```bash
aperture-router --config /path/to/config.toml
```

With debug logging:

```bash
aperture-router --debug
```

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/v1/models` | GET | List available models |
| `/v1/chat/completions` | POST | OpenAI-compatible endpoint |
| `/v1/messages` | POST | Anthropic-compatible endpoint |
| `/v1/proxy` | POST | Generic streaming proxy endpoint |

## License

MIT License - see [LICENSE](LICENSE) file for details.

[Unreleased]: https://github.com/Wayazi/aperture-router/compare/v0.3.2...HEAD
[0.3.2]: https://github.com/Wayazi/aperture-router/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/Wayazi/aperture-router/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/Wayazi/aperture-router/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Wayazi/aperture-router/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Wayazi/aperture-router/releases/tag/v0.1.0
