# Security Layers

> Defense-in-depth: why the proxy has so many overlapping protections, and how each layer works.

## Threat Model

Aperture Router accepts untrusted HTTP requests and forwards them to upstream providers. The
primary threats are:

1. **SSRF** — an attacker crafts a request that makes the proxy fetch an internal URL
   (cloud metadata, internal services, localhost services).
2. **Credential theft** — API keys intercepted in transit, leaked in logs, or recovered
   from memory dumps.
3. **Brute force** — automated guessing of API keys.
4. **Resource exhaustion** — huge request bodies, deeply nested JSON, infinite streams, or
   unique-IP flooding used to exhaust memory.
5. **Client spoofing** — forged `X-Forwarded-For` headers to bypass IP-based rate limiting.

Each layer below addresses one or more of these. The design principle is **overlapping
controls**: no single layer is trusted to be sufficient.

## Layer 1: SSRF Protection

Server-Side Request Forgery is the highest-severity risk because the proxy explicitly fetches
arbitrary URLs on behalf of clients. The canonical source of truth is
`src/security/mod.rs`.

### Metadata Endpoint Blocklist

`is_metadata_endpoint` (`src/security/mod.rs:18`) blocks exact matches against known cloud
metadata IPs and hostnames:

- `169.254.169.254` (AWS/GCP)
- `[::ffff:169.254.169.254]` (IPv4-mapped IPv6 variant)
- `100.100.100.200` (Alibaba Cloud)
- `metadata.google.internal` (GCP hostname)
- `metadata.azure.com` (Azure hostname)

The check is **exact match only**. This is deliberate: a substring check would block
legitimate domains like `not-169.254.169.254.example.com`. The test
`test_metadata_endpoint_rejects_subdomains` enforces this.

`is_blocked_host` (`src/security/mod.rs:32`) extends this with hostname patterns for
Kubernetes internal DNS:

- Any `.internal` domain containing `metadata` (catches `metadata.default.svc.cluster.local`,
  `kubernetes-metadata.internal`, etc.)
- `metadata.*.svc.` patterns
- **Trailing-dot normalization** (RFC 1034): `metadata.google.internal.` is treated the same
  as `metadata.google.internal`, preventing bypass via DNS-equivalent names.

### Internal IP Blocking

`is_internal_ip_impl` (`src/security/mod.rs:58`) is the core check, shared across all
validation contexts. It blocks:

- **IPv4**: private (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`), loopback, link-local
- **IPv6**: loopback (`::1`), unique local (`fc00::/7`), link-local (`fe80::/10`), multicast
  (`ff00::/8`)
- **IPv4-mapped IPv6**: `::ffff:10.0.0.1` is detected via `to_ipv4_mapped()` and checked as
  IPv4, preventing bypass via mapped addresses

The function takes a `block_cgn` parameter. Two variants expose it:

- `is_internal_ip(host)` — **blocks** CGN (`100.64.0.0/10`). Used for the default gateway
  endpoint validation in `ProxyClient::validate_endpoint` (`src/proxy/client.rs:462`).
- `is_internal_ip_strict(ip)` — **allows** CGN. Used for provider URL validation
  (`src/proxy/client.rs:258`), because Tailscale deployments legitimately use the CGN range.

This distinction is critical: blocking CGN globally would break Tailscale deployments, which
are the primary use case.

### DNS Rebinding Protection

For hostname-based provider URLs (not IP literals), `validate_resolved_ips`
(`src/proxy/client.rs:549`) resolves the hostname via `tokio::net::lookup_host` and checks
**every** resolved IP against `is_internal_ip_strict` and `is_metadata_ip`. This prevents
DNS rebinding attacks where:

1. Attacker configures a provider with hostname `evil.com`
2. Initial DNS check passes (resolves to a public IP)
3. DNS TTL expires, record changes to `169.254.169.254`
4. Proxy fetches the metadata endpoint

By resolving at request time, the check sees the current DNS answer.

If DNS resolution fails, the request is **allowed to proceed** (and will fail naturally at
connection time). This is intentional: a DNS outage should not make the proxy unusable, and
blocking on DNS failure would create a DoS vector.

### Redirect Disabling

`create_client_with_timeouts` (`src/http_client.rs:24`) sets
`.redirect(reqwest::redirect::Policy::none())`. This is critical: even if all the above checks
pass, an upstream could return an HTTP 301 redirect to `http://169.254.169.254/...`. With
redirects disabled, the proxy returns the redirect response to the client instead of following
it. Following redirects would bypass all IP validation.

### Allowed Endpoint List

`is_allowed_endpoint` (`src/http_client.rs:47`) restricts the default-gateway proxy path to a
static allowlist: `v1/chat/completions`, `v1/messages`, `v1/models`, `v1/embeddings`. This
prevents path injection (e.g. `../../admin`) on the default gateway path. Multi-provider URLs
bypass this (they go through `forward_request_to_url_raw`) but are subject to the full SSRF
checks above.

## Layer 2: Authentication

### Constant-Time Key Comparison

`validate_api_key` (`src/middleware/auth.rs:181`) uses `subtle::ConstantTimeEq` (`ct_eq`) to
compare the provided key against every configured key. The result is accumulated via
**bitwise OR** (`found |= matches`), not boolean OR (`||`):

```rust
let matches: u8 = if bool::from(valid_key.as_bytes().ct_eq(key_bytes)) { 1 } else { 0 };
found |= matches; // Bitwise OR is constant-time, no short-circuit
```

Boolean `||` short-circuits on the first `true`, leaking timing information about *which* key
matched and *where* the first difference is. Bitwise OR on a `u8` always evaluates both
operands. This is verified by `test_constant_time_comparison`.

The same pattern is used for `validate_admin_key` (`src/middleware/auth.rs:213`).

### Zeroizing Keys

API keys are stored as `Vec<Zeroizing<String>>` (`src/middleware/auth.rs:39`). The
`zeroize` crate overwrites the memory with zeros when the value is dropped, preventing
recovery from a memory dump or core dump. The `APERTURE_API_KEY` environment variable is
also removed from the process environment after loading (`src/config.rs:398`,
`src/main.rs:189`), so it does not appear in `/proc/[pid]/environ`.

### Key Strength Validation

`Config::validate` (`src/config.rs:447`) rejects keys shorter than 32 characters or with
fewer than 20 unique characters. This prevents weak keys like `"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"`
(32 chars, 1 unique) from passing the length check. The same applies to admin keys.

### IP Banning

`AuthState` tracks failed authentication attempts per client IP in a
`HashMap<IpAddr, Vec<Instant>>`. After `max_auth_attempts` failures (default 5) within
`auth_window_secs` (default 60s), the IP is banned for `ban_duration_secs` (default 300s).
Banned IPs receive `429 Too Many Requests` without even checking the key — this prevents
further brute-force attempts from reaching the comparison logic.

The ban map is capped at `MAX_TRACKED_IPS = 10 000` (`src/middleware/auth.rs:33`). When full,
the oldest entry is evicted. This prevents memory exhaustion from a unique-IP DDoS (an
attacker rotating across millions of source IPs).

A background cleanup task (`start_cleanup_task`, `src/middleware/auth.rs:147`) runs every
300s, removing expired entries and empty IP records.

### Admin Key Separation

Admin endpoints (`/admin/*`) require `admin_api_keys`, which are validated by
`validate_admin_key` — a separate function that only checks the admin key list. Regular API
keys cannot access admin endpoints, even if they are valid. Admin keys **can** access regular
endpoints (checked by `validate_api_key`), so configuring only admin keys still works for
all endpoints.

The dev-mode bypass (`APERTURE_ALLOW_DEV_ADMIN`) was removed in v0.3.0 for hardening. In
debug builds, the admin middleware logs a helpful message but still returns `401`.

### Trusted Proxy Validation

`extract_client_ip` (`src/middleware/auth.rs:230`) uses `ConnectInfo` (the actual TCP source
IP, populated by `into_make_service_with_connect_info`) as the primary source. The
`X-Forwarded-For` header is **only** consulted if the connection comes from a configured
trusted proxy IP (`cors.trusted_proxies`). This prevents arbitrary clients from spoofing
their IP via the header.

If `ConnectInfo` is unavailable, the request fails with `500 Internal Server Error` — this
indicates server misconfiguration (the server must use
`into_make_service_with_connect_info::<SocketAddr>()`).

## Layer 3: Rate Limiting

### Per-Client Request Rate Limiter

`RateLimiter` (`src/middleware/rate_limit.rs:14`) is a separate middleware from the auth
attempt limiter. It tracks request timestamps per IP in a sliding window. Configuration:

- `burst_size` (default 30) — max requests in the window
- `requests_per_second` (default 10) — determines window duration
  (`window = burst_size / requests_per_second`)

The limiter is checked **before** authentication (`src/middleware/auth.rs:289`), so it
applies to all requests including invalid ones. This prevents both brute-force and
resource-exhaustion attacks.

Same 10 000-IP memory cap and oldest-eviction policy as the auth limiter.

### Per-Endpoint Validation Limits

Beyond rate limiting, each route handler enforces structural limits to prevent abuse:

- **`max_messages`** (default 10 000, configurable via `security.max_messages`) — rejects
  requests with too many messages. This was hardcoded to 1000 before v0.3.1, which broke
  long agentic sessions.
- **`max_other_fields` = 50** — rejects requests with too many unknown JSON fields
  (prevents memory exhaustion from huge `other` HashMaps).
- **`max_tokens` ≤ 1 000 000** (`validation.rs:51`) — prevents absurd generation requests.
- **Message content ≤ 1 MB** (`validation.rs:41`).
- **Content blocks per message ≤ 100** (streaming handler).

## Layer 4: Size and Depth Caps

### Request Body Limit

`RequestBodyLimitLayer::new(config.security.max_body_size_bytes)` is applied as a Tower
layer in `create_router` (`src/server.rs:365-370`). Default is 10 MB, max allowed is 100 MB
(enforced in `Config::validate`). This rejects oversized payloads before they reach the
handler.

### Streaming Response Size Limit

`ProxyClient::forward_request_stream` (`src/proxy/client.rs:126`) tracks cumulative bytes
received using an `AtomicUsize` with a compare-exchange loop:

```rust
loop {
    let current = total_bytes.load(Ordering::SeqCst);
    if current + chunk_size > max_size {
        return Err(...);
    }
    match total_bytes.compare_exchange(current, current + chunk_size, ...) {
        Ok(_) => break,
        Err(_) => continue, // retry with updated value
    }
}
```

The compare-exchange loop prevents a TOCTOU race: if two chunks arrive concurrently, both
check the limit against the *current* value before either updates. Default cap is 100 MB
(`max_streaming_size_bytes`), max 1 GB.

### JSON Depth Limit

`max_json_depth` (default 256, range 16–4096) prevents deeply-nested JSON DoS. Enforced in
`Config::validate`. The `serde_json` deserializer respects this limit.

### Stream Line Buffer

The SSE converter's `line_buffer` (`src/types/conversion.rs:536`) is capped at 1 MB
(`MAX_LINE_BUFFER`). If a malformed upstream sends data without newlines, the buffer is
cleared and an error event is emitted, preventing unbounded growth.

## Layer 5: Transport Security

### HTTPS Enforcement

`ProxyClient::new` (`src/proxy/client.rs:28`) enforces HTTPS when an API key is configured,
**unless** the target is:

- Tailscale (URL contains `100.100.` or `.tsnet.` — network-layer encrypted)
- Localhost (`localhost` hostname or loopback IP)

This prevents API key exposure over plaintext HTTP while allowing legitimate HTTP for
Tailscale and local development.

### Config File Permissions

`Config::save` (`src/config.rs:642`) writes config files with mode `0o600` (owner
read/write only) from creation — using `OpenOptions::mode(0o600)` before writing, not
chmod-after-write. This eliminates the race condition where a file is briefly world-readable
between creation and chmod.

### Security Headers

`create_router` (`src/server.rs:331-358`) sets these response headers on all responses via
`SetResponseHeaderLayer`:

- `Content-Security-Policy: default-src 'self'`
- `X-Frame-Options: DENY`
- `X-Content-Type-Options: nosniff`
- `X-XSS-Protection: 1; mode=block`
- `Strict-Transport-Security: max-age=31536000; includeSubDomains`
- `Referrer-Policy: strict-origin-when-cross-origin`
- `Permissions-Policy: camera=(), microphone=(), geolocation=()`

These are defense-in-depth against XSS and clickjacking on any HTML that might be served
(though the proxy serves only JSON).

## Layer 6: Production Auth Gate

`Config::validate` (`src/config.rs:581`) refuses to start in release builds if
`require_auth_in_prod` is true and no API keys are configured:

```rust
if self.security.require_auth_in_prod
    && self.security.api_keys.is_empty()
    && !cfg!(debug_assertions)
{
    return Err("Production mode requires authentication...");
}
```

The `!cfg!(debug_assertions)` check means this only fires in release builds. In debug builds,
a warning is logged instead. The `APERTURE_ALLOW_NO_AUTH=1` environment variable disables
this check entirely (for testing).

## Why So Many Layers?

No single control is sufficient:

- SSRF checks are useless if an attacker has a valid API key (they can send legitimate
  requests).
- API key validation is useless against SSRF (the key doesn't help if the proxy fetches a
  metadata endpoint).
- Rate limiting is useless if the attacker has valid credentials.
- Size caps are useless against a slow-loris attack (many small requests).

The layers compose: SSRF blocks the redirect-bypass vector, auth blocks unauthorized access,
rate limiting slows brute force, size caps prevent resource exhaustion, and the production
gate prevents accidental open proxies.

## Further Reading

- `src/security/mod.rs` — SSRF blocklists and IP checks
- `src/middleware/auth.rs` — authentication, banning, constant-time comparison
- `src/middleware/rate_limit.rs` — per-client rate limiting
- `src/proxy/client.rs` — SSRF enforcement at request time
- [Troubleshooting](../troubleshooting.md) — security-related errors
