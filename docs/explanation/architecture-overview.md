# Architecture Overview

> Why this project exists, and why it is built the way it is.

## The Problem

[Tailscale Aperture](https://tailscale.com/blog/aperture) is a gateway that exposes AI models
(OpenAI, Anthropic, OpenRouter, etc.) through a single endpoint on your Tailnet. It handles
authentication, billing, and routing upstream. But Aperture speaks one wire format — the
OpenAI `chat/completions` schema — and many of the best AI developer tools speak a different
one: the Anthropic `messages` schema.

This mismatch forces a choice: use only tools that happen to speak the gateway's format, or
run a translation layer. Aperture Router is that translation layer. It sits between your tools
and Aperture, accepting requests in **either** OpenAI or Anthropic format and forwarding them
in whatever format the upstream provider expects.

## Where It Fits

```
┌──────────────┐   ┌──────────────────┐   ┌──────────────┐
│ Claude Code  │──▶│                  │   │              │
│ OpenCode     │──▶│  Aperture Router │──▶│  Aperture    │──▶ upstream models
│ Pi / any SDK │──▶│  (this project)  │   │  Gateway     │
└──────────────┘   └──────────────────┘   └──────────────┘
   OpenAI or            Converts +           Tailscale
   Anthropic format     proxies              network
```

Aperture Router is **not** a model server. It holds no model weights, runs no inference, and
stores no conversation history. It is a stateless HTTP reverse proxy with two jobs:

1. **Translate** between the OpenAI and Anthropic wire formats when the client and provider
   disagree (see [Format Conversion](format-conversion.md)).
2. **Route** requests to the correct provider, with failover when multiple providers serve the
   same model (see [Multi-Provider Routing](multi-provider-routing.md)).

## Design Decisions

### Rust

The proxy sits on the hot path of every inference request. A garbage-collected runtime adds
latency spikes and unpredictable memory growth under streaming load. Rust gives:

- **Deterministic memory** — no GC pauses during long SSE streams.
- **Small footprint** — release binary RSS is ~4.2 MB, down from 6.4 MB after removing
  `once_cell`/`SHARED_CLIENT` in v0.3.1 (see `CHANGELOG.md`).
- **No runtime dependency** — a single statically-linked binary with no JVM, no Node, no
  Python interpreter to version-manage.

The tradeoff is development velocity: the format conversion logic in
`src/types/conversion.rs` would be terser in a dynamic language, but the type safety prevents
whole classes of runtime panics that would be unacceptable in a proxy.

### Axum

Axum was chosen over Actix-web and Rocket for three reasons:

1. **Tower middleware ecosystem** — `tower-http` provides `RequestBodyLimitLayer`,
   `CorsLayer`, `TraceLayer`, and `SetResponseHeaderLayer` as composable layers. The security
   headers, body size cap, and CORS configuration in `src/server.rs:325-364` are all
   declarative layers, not hand-rolled checks.
2. **`from_fn_with_state`** — Axum's middleware extractor pattern lets `auth_middleware`
   (`src/middleware/auth.rs:265`) receive typed state `(Arc<Config>, Arc<AuthState>,
   RateLimiter)` without global variables.
3. **No macros** — route definitions are plain Rust function calls (`Router::new().route(...)`),
   which makes them greppable and refactor-safe.

The alternative, Actix-web, uses an actor model that adds indirection for a stateless proxy.
Rocket's macro-heavy routing was rejected for the same reason.

### Single Binary, No Database

There is no database, no Redis, no on-disk cache. All state lives in memory:

- **Model list** — `ModelDiscovery` (`src/discovery/models.rs`) holds a `HashMap<String, Model>`
  behind an `Arc<RwLock<>>`. It refreshes from Aperture's `/v1/models` on an interval
  (default 300 s) and on startup.
- **Provider registry** — `ProviderRegistry` (`src/provider/registry.rs`) holds the
  model-to-provider routing map, synced from discovery on each refresh.
- **Auth state** — `AuthState` (`src/middleware/auth.rs`) tracks failed login attempts per IP
  in a `HashMap<IpAddr, Vec<Instant>>`, capped at 10 000 entries to bound memory.
- **Rate limiter** — `RateLimiter` (`src/middleware/rate_limit.rs`) tracks request timestamps
  per IP, same 10 000-IP cap.

This means **restart loses all ban state and rate-limit counters**. That is intentional: a
proxy that needs persistent state is a different class of system (an API gateway, not a
router). If you need durable bans or usage tracking, put a reverse proxy (nginx, Caddy) in
front and log there.

The absence of a database also eliminates an entire attack surface: no SQL injection, no
connection-pool exhaustion, no migration failures, no disk I/O on the request path.

### `Arc<Config>` Instead of `Clone<Config>`

The `Config` struct is relatively large (it contains `Vec<Provider>`, `SecurityConfig`,
`HttpConfig`, etc.). In v0.3.0 it was cloned per-handler. In v0.3.1 it was wrapped in
`Arc` once in `create_router` (`src/server.rs:220`) and cheaply reference-counted to each
handler via `AppState`. This single change accounted for most of the 35% RSS reduction.

### `panic = "abort"` in Release

`Cargo.toml:79` sets `panic = "abort"` for the release profile. This means a panic kills the
process immediately instead of unwinding. Rationale:

- **Smaller binary** — no unwind tables.
- **Fail-fast** — a panicking thread in a proxy likely indicates corrupted state; continuing
  to serve requests on top of that is dangerous.
- **systemd restarts** — the service unit restarts the process, so abort is recoverable.

The tradeoff is that a single malformed request must not be able to panic the server. The
format conversion code uses `serde_json::Value` (dynamic JSON) precisely to avoid
deserialization panics on unexpected fields — everything is `Option`-based with `.unwrap_or`
fallbacks.

### Two Branches: `dev` and `main`

The git workflow (`docs/WORKFLOW.md`) uses `dev` for integration and `main` for releases.
`main` is branch-protected and requires passing CI. This is documented separately because it
affects how contributors should branch (see [AGENTS.md](../../AGENTS.md)).

## What This Project Is Not

- **Not a load balancer** — it does not distribute traffic across instances. Use nginx/HAProxy
  for that (see `DEPLOYMENT.md` "High Availability").
- **Not an API gateway** — no usage quotas, no billing, no per-key rate limits beyond
  brute-force protection. Aperture handles those.
- **Not a cache** — responses are streamed through unchanged (when formats match) or
  converted in-flight. Nothing is stored.
- **Not a model aggregator** — it does not merge responses from multiple models. Failover is
  sequential, not parallel.

## Further Reading

- [Format Conversion](format-conversion.md) — how Anthropic↔OpenAI translation works
- [Security Layers](security-layers.md) — defense-in-depth design
- [Multi-Provider Routing](multi-provider-routing.md) — failover and discovery
- [Troubleshooting](../troubleshooting.md) — common issues
