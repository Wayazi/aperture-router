# Changelog

All notable changes to aperture-router will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- **Axum's built-in 2 MB body limit silently capped requests** - `security.max_body_size_bytes` was enforced via a tower-http layer, but axum's extractors also apply their own hardcoded 2 MB `DefaultBodyLimit`. Any `/v1/messages` request over 2 MB — typical once base64 screenshots accumulate in Claude Code sessions — was rejected with `413` long before the configured limit, and clients showed misleading errors (e.g. Claude Code's "Request too large (max 32MB)" for any 413 from a custom gateway). The built-in default is now disabled so the configured limit is the single source of truth.
- **OpenRouter reasoning preserved** - The Anthropic↔OpenAI converter only recognized `reasoning_content` (DeepSeek-style). OpenRouter reasoning models (e.g. `stealth/ox-alpha`) return their chain-of-thought in a `reasoning` field, which was silently dropped in both non-streaming responses and streaming deltas when clients used `/v1/messages`. Both field names are now accepted and mapped to Anthropic `thinking` blocks.
- **Static providers no longer dropped by discovery refresh** - `ProviderRegistry::update_from_discovery()` retained only providers present in the current gateway snapshot, silently deleting user-configured `[[providers]]` blocks (and their custom `endpoint_style`) on the first model refresh. Static providers now survive all refreshes.
- **Discovery no longer clobbers static model routes** - Discovered models unconditionally overwrote `model_to_provider`, replacing deliberate config-file routes with auto-added providers using a default endpoint style. A static route for a model now always wins; discovery only fills unmapped models.
- **SSE lines split across TCP chunks** - `/v1/proxy` and Anthropic-direct streaming parsed each network chunk independently: a chunk boundary landing mid-line forwarded truncated JSON as an event and silently dropped the continuation. A cross-chunk line buffer (`SseLineBuffer`) now reassembles complete lines before parsing on both passthrough paths; fully-filtered chunks also no longer emit empty SSE frames.
- **Thinking blocks leaked into outgoing prompts** - Assistant `thinking`/`redacted_thinking` history blocks fell through the conversion catch-all and were serialized as literal JSON text into the upstream prompt, polluting context on every subsequent turn of reasoning-enabled conversations. They are now stripped in all request-conversion paths.
- **Tool results emitted after follow-up user text** - A user turn mixing `tool_result` blocks with text produced `user → tool` ordering, violating OpenAI's requirement that tool messages immediately follow the assistant `tool_calls` message (strict backends return 400). Tool messages are now emitted first.
- **Index-less tool-call stream collisions** - Stream converter defaulted missing tool-call delta `index` to slot 0, so multiple tool calls from Mistral/llama.cpp-style backends collided (duplicate block stops, unclosed blocks); repeated full id+name deltas opened duplicate blocks. Slots are now allocated per new id and repeated ids continue the existing block.
- **Interleaved reasoning collided with open text blocks** - When a reasoning model emitted thinking deltas after text had started (interleaved reasoning), the thinking block opened at the still-open text block's index, corrupting downstream block indices for the rest of the stream. Text blocks are now closed before a new thinking block opens, giving each its own index.
- **Id-less tool-call continuations all landed in slot 0** - Backends that send `id` only on each call's first fragment (llama.cpp-style) had every subsequent id-less, index-less argument fragment routed to slot 0, concatenating parallel calls' arguments into invalid JSON and losing the second invocation. Continuations now follow the most recently active tool call.
- **Null/empty-id tool calls vanished from streams** - The slot-allocation rework treated deltas with null or empty-string `id` as continuations of a block that never existed, so such calls emitted no `content_block_start` at all. An id-less fragment arriving at an empty slot now opens a block with a synthesized id.
- **SSE line-buffer cap panicked on multi-byte characters and corrupted framing** - The 1 MiB overflow cut used `split_off` at a fixed byte offset (panics inside a CJK/emoji character), fabricated a `\n` that truncated a line into a bogus event, and could carry away a genuinely complete line. The cut is now char-boundary-safe and re-feeds the head through the normal path; a `take_remainder()` flush also delivers a final unterminated line at end-of-stream instead of dropping it (e.g. a trailing `data: [DONE]`).
- **Discovery size cap bypassed for chunked responses** - The 16 MiB cap pre-checked `Content-Length`, but chunked/close-delimited responses have none and `.bytes()` buffers the whole body before the post-read check. The body is now read in bounded chunks and the connection dropped mid-body on overflow.
- **MagicDNS Tailscale check never matched and whitelisted lookalikes** - `contains(".ts.net.")` cannot match a real MagicDNS host (`ts.net` is followed by end-of-string, port or path) while matching attacker domains like `evil.ts.net.attacker.com`. The check now tests the parsed host for the `.ts.net`/`.tsnet` suffix.
- **Tailscale detection rejected valid CGNAT gateways** - HTTPS enforcement matched Tailscale via a `100.100.` substring, but Tailscale assigns the whole 100.64.0.0/10 range — a legitimate HTTP gateway like `100.90.x.x` with an API key panicked at startup despite config validation passing with only a warning. Detection now parses the host IP against the real CGNAT range (new `security::is_cgnat`) plus `.ts.net.`/`.tsnet.` hostnames.
- **Rate limiter disabled by burst < rps configs** - Window computed as `burst_size / requests_per_second` integer division; when burst < rps the zero window made every check pass forever. Window is clamped to at least 1 second.
- **OpenAI requests posted to Anthropic-style providers** - `/v1/chat/completions` routed models served by `endpoint_style = "anthropic"` providers straight to `{base}/v1/messages` with an OpenAI-format body. Anthropic-style providers are now excluded from the OpenAI route with a clear error when no OpenAI-style provider serves the model.
- **First discovery refresh delayed** - The refresh task consumed `interval`'s immediate first tick before entering its loop, delaying the first background refresh by a full interval. The initial fetch now happens right away.

### Changed
- **Smarter endpoint style for auto-added providers** - Providers auto-created from gateway discovery now infer their style: namespaced model IDs (`vendor/model`, e.g. OpenRouter-style aggregators) get `openai_v1`; bare IDs (Aperture GLM-style upstreams) keep `openai_direct`. Mixed groups use a majority rule so one aliased ID cannot flip an entire group's path style. Explicit config still overrides everything.
- **Discovered model lists converge** - Auto-added providers now mirror the current snapshot on every refresh instead of only appending, so models the gateway removed stop routing rather than accumulating forever. Static provider model lists are never mutated by discovery.
- **Discovery response hardening** - Model discovery caps response bodies at 16 MiB (pre-checked via content-length, re-checked after read) and snapshots at 10 000 entries, so a runaway gateway cannot exhaust memory on every refresh tick. Gateway-supplied model and provider IDs are validated (`[A-Za-z0-9-_./]`, ≤256 bytes, no traversal) before entering the registry; invalid provider IDs fall back to `default`.
- **Disabled provider names reserved** - A provider explicitly set `enabled = false` in config is never resurrected as an enabled auto-added provider by discovery.

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
