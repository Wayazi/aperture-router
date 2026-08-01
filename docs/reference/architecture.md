# Architecture

> Internal architecture of aperture-router: modules, request flow, state sharing, background tasks.

Source: `src/lib.rs`, `src/server.rs`, `src/main.rs`.

## Module Layout

```
src/
├── main.rs              Binary entry point, CLI dispatch, shutdown
├── lib.rs               Crate root, module declarations
├── server.rs            Router assembly, AppState, middleware wiring
├── config.rs            Config model, validation, atomic save
├── http_client.rs       HTTP client builder, endpoint allow-list
├── routes/
│   ├── mod.rs           Route barrel, model validation helper
│   ├── chat.rs          POST /v1/chat/completions (OpenAI)
│   ├── messages.rs      POST /v1/messages (Anthropic + conversion)
│   ├── streaming.rs     POST /v1/proxy (SSE streaming)
│   ├── models.rs        GET /v1/models
│   ├── admin.rs         POST /admin/refresh-models, GET /admin/stats
│   ├── health.rs        GET /health
│   ├── proxy.rs         Multi-provider proxy engine with failover
│   └── error.rs         Shared error response builder
├── proxy/
│   └── client.rs        Upstream HTTP forwarding, SSRF defense
├── middleware/
│   ├── auth.rs          AuthState, constant-time auth, IP banning
│   └── rate_limit.rs    Per-IP sliding-window rate limiter
├── security/
│   └── mod.rs           SSRF: metadata blocklist, internal IP checks
├── types/
│   ├── openai.rs        OpenAI request/response structs
│   ├── anthropic.rs     Anthropic request/response structs
│   ├── conversion.rs    Bidirectional format converter + SSE converter
│   └── validation.rs    Model name, role, content, max_tokens validators
├── provider/
│   └── registry.rs      Model→provider map, endpoint URL construction
├── discovery/
│   └── models.rs        Dynamic model discovery from Aperture
└── cli/
    ├── mod.rs           CLI module root, privilege detection
    ├── commands.rs      Config subcommand handlers
    ├── wizard.rs        Interactive wizard (feature-gated)
    ├── model_fetcher.rs Model fetching for CLI
    ├── security.rs      SecretString, URL validation, key strength
    ├── opencode_export.rs  OpenCode config export
    └── openclaw_export.rs  OpenClaw config export
```

## Boot Sequence

1. **CLI parse** (`main.rs:138`) — `clap` resolves flags and subcommand
2. **Env + tracing** (`main.rs:141`) — `.env` loaded, `tracing_subscriber` initialized
3. **Config load** (`main.rs:173`) — TOML file or env-only, then `validate()`
4. **Auth guard** (`main.rs:226`) — release builds reject no-auth
5. **Model discovery** (`main.rs:234`) — `ModelDiscovery::new()` + `fetch_models()`
6. **Router assembly** (`main.rs:255` → `server::create_router`):
   - `ProviderRegistry` built from config
   - `ProxyClient` built with config-aware HTTP client
   - `AuthState` built from security + CORS config
   - `RateLimiter` built from rate_limit config
   - 3 background tasks spawned (auth cleanup, model refresh, rate limiter cleanup)
   - `AppState` wrapped in `Arc`
   - Routes + middleware + security headers wired
7. **Serve** (`main.rs:258`) — `TcpListener::bind` + `axum::serve` with graceful shutdown

## Request Flow

```
Client
  │
  ▼
TCP accept (ConnectInfo<SocketAddr>)
  │
  ▼
CORS layer
  │
  ▼
RequestBodyLimitLayer (max_body_size_bytes, default 10MB)
  │
  ▼
Security headers (CSP, X-Frame-Options, HSTS, etc.)
  │
  ▼
TraceLayer (tower-http)
  │
  ▼
add_request_id (generates request_id + session_id, opens tracing span)
  │
  ▼
Route-group middleware:
  ├── /health          → (no auth)
  ├── /admin/*         → admin_auth_middleware
  └── /v1/*            → auth_middleware (rate limit → ban check → key validation)
  │
  ▼
Handler (chat / messages / streaming / models / admin)
  │
  ▼
ProxyClient (SSRF check → forward to upstream)
  │
  ▼
Upstream (Aperture gateway or custom provider)
  │
  ▼
Response (security headers applied → CORS → client)
```

## State Sharing

`AppState` (`src/server.rs:100`) is the single shared state, wrapped in `Arc<AppState>`:

| Field | Type | Shared via |
|-------|------|-----------|
| `config` | `Arc<Config>` | Arc (cheap clone) |
| `auth_state` | `AuthState` | Clone (internal Arcs) |
| `proxy_client` | `ProxyClient` | Clone (internal Arc) |
| `discovery` | `Arc<ModelDiscovery>` | Arc |
| `provider_registry` | `Arc<ProviderRegistry>` | Arc |
| `shutdown_token` | `CancellationToken` | Clone (cheap) |
| `rate_limiter` | `RateLimiter` | Clone (internal Arc) |
| `*_handle` | `Arc<Mutex<Option<JoinHandle>>>` | Arc + Mutex |

## Background Tasks

Three tasks spawned in `create_router`, all shutdown-aware via `CancellationToken`:

| Task | Interval | Purpose |
|------|----------|---------|
| Auth cleanup | 300s | Prune expired auth failure timestamps, drop empty IP entries |
| Model refresh | `model_refresh_interval_secs` (default 300s) | Fetch models from Aperture, sync to `ProviderRegistry` |
| Rate limiter cleanup | 300s | Prune expired request timestamps, drop empty IP entries |

## Graceful Shutdown

`shutdown_signal()` (`src/main.rs:358`) uses `tokio::select!` over Ctrl+C and SIGTERM. The `CancellationToken` propagates to all background tasks. After `axum::serve` returns, `main` joins each `JoinHandle` sequentially.

## Multi-Provider Failover

```
Request arrives
  │
  ▼
provider_registry.get_providers_for_model(model)
  │
  ├── 0 providers → default Aperture gateway
  ├── 1 provider  → try_provider() → forward_request_to_url_raw()
  └── >1 providers → failover loop (max 3 attempts)
                      ├── Provider A → 5xx/conn error → try next
                      ├── Provider B → 5xx/conn error → try next
                      └── Provider C → success or 4xx (terminal)
```

4xx errors are terminal (no retry). Only 5xx and connection errors trigger failover.

## Anthropic Streaming Paths

Three streaming scenarios for `/v1/messages`:

1. **Anthropic provider + stream:true** → `handle_anthropic_direct_streaming()` — true SSE passthrough, no conversion
2. **No Anthropic provider + stream:true** → `handle_streaming_conversion()` — OpenAI stream converted to Anthropic SSE via `OpenAIToAnthropicStreamConverter`
3. **`/v1/proxy`** → `handle_proxy_stream()` — format-agnostic passthrough with auto-detection
