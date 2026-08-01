# Security Model

> How aperture-router protects against threats. Defense-in-depth design.

Source: `src/security/mod.rs`, `src/middleware/auth.rs`, `src/middleware/rate_limit.rs`, `src/proxy/client.rs`, `src/config.rs`.

## Threat Model

aperture-router sits between untrusted clients (AI tools, scripts) and upstream AI providers. It must prevent:

1. **SSRF** — requests to internal infrastructure (cloud metadata, private networks)
2. **Credential theft** — API keys leaked via logs, memory, or network
3. **Brute force** — automated API key guessing
4. **DoS** — resource exhaustion via large payloads or flooding
5. **Timing attacks** — key validation leaking information

## Security Layers

### 1. SSRF Protection

#### Cloud metadata blocklist

Exact-match blocking of known metadata endpoints (`src/security/mod.rs:18`):

| Endpoint | Provider |
|----------|----------|
| `169.254.169.254` | AWS / GCP (link-local) |
| `[::ffff:169.254.169.254]` | IPv4-mapped IPv6 variant |
| `100.100.100.200` | Alibaba Cloud |
| `metadata.google.internal` | GCP (DNS) |
| `metadata.azure.com` | Azure (DNS) |

#### Pattern-based blocking

- Trailing-dot normalization (RFC 1034) — `metadata.internal.` is blocked
- Any `.internal` domain containing `metadata` — catches k8s service DNS
- `metadata.*.svc.*` Kubernetes patterns

#### Internal IP detection

- Private ranges: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
- Loopback: `127.0.0.0/8`, `::1`
- Link-local: `169.254.0.0/16`, `fe80::/10`
- IPv4-mapped IPv6: recursively unwrapped via `to_ipv4_mapped()`

Two policies:
- `is_internal_ip` — blocks CGN `100.64.0.0/10` (for default gateway validation)
- `is_internal_ip_strict` — allows CGN (for provider URLs, since Tailscale uses it)

#### DNS rebinding protection

`validate_resolved_ips()` (`src/proxy/client.rs:549`) resolves the host at request time and validates every resolved IP. Prevents DNS rebinding where a hostname initially resolves to a public IP but later to an internal one.

#### Redirect disabled

The HTTP client uses `redirect(Policy::none())` — prevents SSRF via HTTP redirects to internal hosts.

#### Provider URL validation

Config-time validation (`src/config.rs:604`) blocks provider `base_url` pointing to internal/metadata IPs.

### 2. Credential Handling

#### Zeroizing storage

API keys are stored as `Vec<Zeroizing<String>>` (`src/middleware/auth.rs:39`). When `AuthState` is dropped, key material is overwritten in memory.

CLI-side secrets use `SecretString` with `#[derive(Zeroize)]` (`src/cli/security.rs:19`).

#### Log redaction

- `hash_api_key()` (`src/middleware/auth.rs:23`) — SHA-256 truncated to 8 bytes for log correlation
- `SecretString` Debug/Display print `****` / `[HIDDEN]`
- `safe_config_summary()` reports `key=set|none`, never the value
- `list_config` command shows `configured`/`none`, never the key

#### Environment scrubbing

`APERTURE_API_KEY` is removed from the process environment after loading (`src/main.rs:189`, `src/config.rs:389`). Prevents leakage via `/proc/[pid]/environ`.

#### HTTPS enforcement

`ProxyClient::new()` (`src/proxy/client.rs:33`) rejects HTTP + API key combinations unless the URL is Tailscale (`100.100.` or `.tsnet.`) or localhost.

### 3. Authentication

#### Constant-time comparison

`validate_api_key()` (`src/middleware/auth.rs:181`) iterates ALL keys using `subtle::ConstantTimeEq` (`ct_eq`). Results are accumulated with bitwise OR on a `u8` — no short-circuit evaluation, preventing timing attacks.

#### Admin key separation

- Admin keys work for regular endpoints too
- Regular keys CANNOT access admin endpoints
- `validate_admin_key()` only checks `admin_api_keys`

#### Key strength validation

- Minimum 32 characters
- Minimum 20 unique characters
- Placeholder detection (`"your-api-key-here"`, etc.)

### 4. Rate Limiting & Brute Force

#### Pre-auth rate limiting

`RateLimiter` (`src/middleware/rate_limit.rs`) applies to ALL requests before credential checks. Sliding window with configurable `burst_size` and `requests_per_second`.

#### IP ban tracking

`AuthState.failed_attempts` tracks per-IP failure history. After `max_auth_attempts` (default 5) failures within `auth_window_secs` (default 60s), the IP is banned for `ban_duration_secs` (default 300s).

#### Memory caps

Both `RateLimiter` and `AuthState` cap tracked IPs at 10,000 with LRU eviction to prevent memory exhaustion.

### 5. Request Size Limits

| Limit | Default | Configurable |
|-------|---------|-------------|
| Request body | 10 MB | `security.max_body_size_bytes` |
| JSON nesting depth | 256 | `security.max_json_depth` |
| Streaming response | 100 MB | `security.max_streaming_size_bytes` |
| Messages per request | 10,000 | `security.max_messages` |
| Extra JSON fields | 50 | (hardcoded) |

### 6. Filesystem Security

#### Config file permissions

`Config::save()` writes with `0o600` (owner read/write only) on Unix. Uses atomic write (temp file + rename).

#### Symlink attack prevention

`safe_read_existing_file()` (`src/cli/commands.rs:267`) uses `symlink_metadata` and refuses to follow symlinks.

#### Exported config permissions

OpenCode/OpenClaw config exports are written with `0o600` permissions.

#### Ownership fix

When running elevated, `fix_system_config_ownership()` uses native `chown` syscalls (no shell) to set the config file owner to the `aperture-router` service user.

### 7. Security Headers

All responses include:

| Header | Value |
|--------|-------|
| `Content-Security-Policy` | `default-src 'self'` |
| `X-Frame-Options` | `DENY` |
| `X-Content-Type-Options` | `nosniff` |
| `X-XSS-Protection` | `1; mode=block` |
| `Strict-Transport-Security` | `max-age=31536000; includeSubDomains` |
| `Referrer-Policy` | `strict-origin-when-cross-origin` |
| `Permissions-Policy` | `camera=(), microphone=(), geolocation=()` |

### 8. Client IP Extraction

`extract_client_ip()` (`src/middleware/auth.rs:230`) uses the unspoofable `ConnectInfo<SocketAddr>` from the TCP connection. Only trusts `X-Forwarded-For` when the peer IP is in `trusted_proxies` (from CORS config). Takes the leftmost (original client) entry.
