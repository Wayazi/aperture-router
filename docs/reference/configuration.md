# Configuration reference

> Complete reference for `config.toml`. Field names, types, defaults, and validation rules.

Source: `src/config.rs`, `config.example.toml`.

## File locations

| Priority | Path | Notes |
|----------|------|-------|
| `--config PATH` | arbitrary | Highest priority |
| `--system` or running as root | `/etc/aperture-router/config.toml` | System service |
| default | `./config.toml` | Current directory |

If no config file exists and `APERTURE_BASE_URL` is set, a minimal config is built from environment variables.

## Environment variable overrides

Loaded after the config file; override the corresponding fields.

| Variable | Overrides |
|----------|-----------|
| `APERTURE_HOST` | `host` |
| `APERTURE_PORT` | `port` |
| `APERTURE_BASE_URL` | `aperture.base_url` |
| `APERTURE_API_KEY` | `aperture.api_key` (then removed from env) |
| `APERTURE_CLIENT_API_KEYS` | `security.api_keys` (comma-separated; env-only bootstrap and `config generate`) |
| `APERTURE_ALLOW_NO_AUTH` | sets `security.require_auth_in_prod = false` |
| `RUST_LOG` | log filter |

`APERTURE_API_KEY` populates the Aperture gateway key (used when forwarding to Aperture), **not** `security.api_keys`. For inbound client auth keys without a config file, set `APERTURE_CLIENT_API_KEYS=key1,key2`.

## Top-level fields

| Field | Type | Default | Validation |
|-------|------|---------|------------|
| `host` | string | `"127.0.0.1"` | Must parse as IP |
| `port` | u16 | `8765` | `!= 0` |
| `aperture` | table | see below | — |
| `http` | table | see below | — |
| `cors` | table | see below | — |
| `rate_limit` | table | see below | — |
| `security` | table | see below | — |
| `model_aliases` | map<string,string> | `{}` | — |
| `multi_provider_enabled` | bool | `false` | — |
| `providers` | array of `[[providers]]` | `[]` | see Provider |

## `[aperture]`

| Field | Type | Default | Validation |
|-------|------|---------|------------|
| `base_url` | string | `"http://localhost:8080"` | Non-empty; `http`/`https` scheme; not a metadata endpoint; not an internal IP (CGN `100.64.0.0/10` blocked here) |
| `api_key` | option<string> | `None` | Must not contain `your-api-key-here`; if set and `base_url` is non-HTTPS/non-Tailscale/non-localhost → startup fails |
| `model_refresh_interval_secs` | u64 | `300` | — |

## `[http]`

| Field | Type | Default (secs) | Validation |
|-------|------|----------------|------------|
| `connect_timeout_secs` | u64 | `10` | — |
| `request_timeout_secs` | u64 | `300` | — |
| `sse_keep_alive_secs` | u64 | `15` | — |
| `upstream_retry_attempts` | u32 | `2` | ≤ 5 |
| `upstream_retry_base_delay_ms` | u64 (ms) | `2000` | > 0, ≤ 10 000 |

The HTTP client (`src/http_client.rs`) also enforces: `pool_max_idle_per_host = 5`, `pool_idle_timeout = 60s`, redirects disabled (`Policy::none`, SSRF protection).

**Upstream 429 retries:** when an upstream answers `429 Too Many Requests` (typical of
shared model pools), the router waits and retries before surfacing the error. Attempt *n*
(0-based) waits `base_delay × 2ⁿ × jitter(0.7–1.3)`; a server `Retry-After` header overrides
the computed delay, capped at 4× base delay. With defaults the backoff sleeps add roughly
4.2–7.8 s (≈6 s on average) on top of the retried upstream requests, and a server
`Retry-After` can push each wait up to 8 s (4× the default base).

**Quota-exhaustion carveout:** if a 429 response carries `x-ratelimit-remaining: 0`, the
retry loop short-circuits immediately — exponential backoff would only delay a hard
quota wall (e.g. an OpenRouter daily cap) that retries cannot drain.
Set `upstream_retry_attempts = 0` to disable and pass 429s through immediately.

## `[cors]`

| Field | Type | Default | Validation |
|-------|------|---------|------------|
| `allowed_origins` | vec<string> | `["http://localhost:3000"]` | Warns on `"*"` or empty |
| `trusted_proxies` | set<IpAddr> | `{}` | Parsed as IP addresses |

Allowed request headers: `Content-Type`, `Authorization`, `Accept`, `x-api-key`, `x-session-id`. Methods: `GET`, `POST`, `OPTIONS`. Credentials mode: always on.

## `[rate_limit]`

| Field | Type | Default | Validation |
|-------|------|---------|------------|
| `requests_per_second` | u64 | `10` | `!= 0` |
| `burst_size` | u64 | `30` | `!= 0` |
| `health_requests_per_second` | u64 | `20` | `!= 0` |
| `health_burst_size` | u64 | `50` | `!= 0` |

Per-IP window = `burst_size / requests_per_second` seconds. Memory cap: 10,000 tracked IPs (LRU eviction).

## `[security]`

| Field | Type | Default | Validation |
|-------|------|---------|------------|
| `api_keys` | vec<string> | `[]` | Each: ≥32 chars, ≥20 unique chars, no placeholders |
| `admin_api_keys` | vec<string> | `[]` | Same as `api_keys` |
| `max_body_size_bytes` | usize | `10485760` (10 MB) | `> 0`, `≤ 100 MB` |
| `max_auth_attempts` | usize | `5` | `> 0` |
| `auth_window_secs` | u64 | `60` | `> 0` |
| `ban_duration_secs` | u64 | `300` | `> 0` |
| `require_auth_in_prod` | bool | `true` | If true + empty `api_keys` + release build → startup fails |
| `max_json_depth` | usize | `256` | `16..=4096` |
| `max_streaming_size_bytes` | usize | `104857600` (100 MB) | `> 0`, `≤ 1 GB` |
| `max_messages` | usize | `10000` | `> 0` |

Additional hardcoded limits (not configurable):

| Limit | Value | Where |
|-------|-------|-------|
| Max response size (non-streaming) | 10 MB | `routes/proxy.rs:MAX_RESPONSE_SIZE` |
| Max extra JSON fields per request | 50 | `routes/chat.rs`, `routes/messages.rs` |
| Max content length per message | 1 MB | `routes/streaming.rs:MAX_CONTENT_SIZE` |
| Max content blocks per message | 100 | `routes/streaming.rs` |
| Model name max length | 128 chars | `types/validation.rs` |
| `max_tokens` limit | 1,000,000 | `types/validation.rs` |
| Message content max length | 1,000,000 chars | `types/validation.rs` |
| `temperature` range | 0.0–2.0 | `types/validation.rs` |
| `top_p` range | 0.0–1.0 | `types/validation.rs` |
| Valid roles | `system`, `user`, `assistant`, `tool` | `types/validation.rs` |

## `[model_aliases]`

Free-form map. Keys are aliases; values are real model names. Resolution happens before model validation and routing.

## `[[providers]]`

Array of provider tables. Only used when `multi_provider_enabled = true`.

| Field | Type | Default | Validation |
|-------|------|---------|------------|
| `name` | string | — | Required, non-empty, unique |
| `base_url` | string | — | Required, non-empty, `http`/`https` scheme, not internal/metadata IP |
| `api_key` | option<string> | `None` | — |
| `endpoint_style` | enum | `openai_v1` | One of `openai_v1`, `openai_direct`, `anthropic` |
| `models` | vec<string> | — | Required, non-empty |
| `enabled` | bool | `true` | — |

`endpoint_style` is serialized as `snake_case`.

## Endpoint style → URL

| Style | Chat completions URL | Messages URL |
|-------|----------------------|--------------|
| `openai_v1` | `{base}/v1/chat/completions` | `{base}/v1/messages` |
| `openai_direct` | `{base}/chat/completions` | `{base}/messages` |
| `anthropic` | — | `{base}/v1/messages` |

Trailing slashes in `base_url` are stripped.

## Allowed endpoints (default gateway)

The Aperture gateway proxy only accepts these endpoint suffixes (`src/http_client.rs::ALLOWED_ENDPOINTS`):

- `v1/chat/completions`
- `v1/messages`
- `v1/models`
- `v1/embeddings`

Multi-provider routing bypasses this list (provider URLs are constructed from `base_url` + style).

## Config save behavior

`Config::save` writes atomically: temp file created with mode `0600` (Unix), then renamed over the target. When saving to `/etc/aperture-router/config.toml` as root, ownership is chowned to `aperture-router:aperture-router` via `nix::unistd::chown`.

## Validation order

`Config::validate()` (`src/config.rs`) checks, in order:

1. `port != 0`
2. `aperture.base_url` non-empty
3. Aperture `api_key` not placeholder; HTTPS warning if key + HTTP
4. Each `security.api_keys` strength
5. Each `security.admin_api_keys` strength
6. `rate_limit.*` non-zero
7. `security.max_auth_attempts`, `auth_window_secs`, `ban_duration_secs` non-zero
8. CORS wildcard/empty warnings
9. `max_body_size_bytes` in `1..=100MB`
10. `max_json_depth` in `16..=4096`
11. `max_streaming_size_bytes` in `1..=1GB`
12. `max_messages > 0`
13. `http.upstream_retry_attempts` ≤ 5
14. `http.upstream_retry_base_delay_ms` > 0 and ≤ 10 000
15. Production auth requirement (release builds only)
16. Each provider: unique name, non-empty base_url/models, valid scheme, not internal/metadata IP
