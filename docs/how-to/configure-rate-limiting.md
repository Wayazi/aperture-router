# Configure rate limiting

> Tune request rate limits, authentication attempt limits, and ban duration.

aperture-router enforces three independent limits:

1. **Per-IP request rate limit** — applied to all authenticated `/v1/*` routes before auth.
2. **Authentication failure limit** — per-IP failed-auth attempts within a window; exceeding bans the IP.
3. **Health endpoint rate limit** — separate, more lenient limit for `/health`.

## Configuration

All settings live in `config.toml`.

### Request rate limit

```toml
[rate_limit]
requests_per_second = 10
burst_size = 30
health_requests_per_second = 20
health_burst_size = 50
```

| Field | Default | Min | Description |
|-------|---------|-----|-------------|
| `requests_per_second` | `10` | `1` | Sustained requests/sec per IP |
| `burst_size` | `30` | `1` | Max burst per IP (token-bucket capacity) |
| `health_requests_per_second` | `20` | `1` | Sustained req/sec for `/health` |
| `health_burst_size` | `50` | `1` | Max burst for `/health` |

The per-IP window duration is computed as `burst_size / requests_per_second` seconds. The limiter is a sliding-window counter keyed by `IpAddr` — individual request timestamps expire after the window elapses.

### Auth failure limit and ban

```toml
[security]
max_auth_attempts = 5
auth_window_secs = 60
ban_duration_secs = 300
```

| Field | Default | Min | Description |
|-------|---------|-----|-------------|
| `max_auth_attempts` | `5` | `1` | Failed auth attempts allowed in the window before ban |
| `auth_window_secs` | `60` | `1` | Sliding window length (seconds) |
| `ban_duration_secs` | `300` | `1` | Duration IP is banned after exceeding attempts |

Mechanism (`src/middleware/auth.rs`):

1. Each failed auth (`401`) pushes a timestamp into `failed_attempts[ip]`.
2. Timestamps older than `auth_window_secs` are pruned.
3. If `len(failed_attempts[ip]) >= max_auth_attempts` → `is_banned()` returns true → subsequent requests get `429` until the oldest attempt ages out (i.e. for `ban_duration_secs`).
4. A successful auth clears the IP's entry.

A background task prunes expired entries every 300s and removes empty IP entries.

## Choosing values

| Scenario | `max_auth_attempts` | `auth_window_secs` | `ban_duration_secs` |
|----------|---------------------|--------------------|---------------------|
| Trusted LAN / Tailscale | `10` | `60` | `120` |
| Public-facing | `3` | `60` | `600` |
| Default | `5` | `60` | `300` |

## Memory protection

Both the request limiter and the auth-failure map cap at `MAX_TRACKED_IPS = 10000`. When full, the oldest-seen IP is evicted (LRU-style) before inserting a new one, preventing memory exhaustion from unique-IP floods.

## Bypassing for trusted proxies

If aperture-router is behind a reverse proxy, set `cors.trusted_proxies` so the router reads the real client IP from `X-Forwarded-For`:

```toml
[cors]
trusted_proxies = ["127.0.0.1", "::1"]
```

Only requests from a trusted-proxy IP honor `X-Forwarded-For`; all others use the socket peer IP (unspoofable). See [setup-reverse-proxy](setup-reverse-proxy.md).

## Disabling auth (dev only)

```bash
export APERTURE_ALLOW_NO_AUTH=1
aperture-router
```

Or in config:

```toml
[security]
require_auth_in_prod = false
```

This skips the API-key check but the per-IP request rate limit still applies. In release builds, an empty `api_keys` list with `require_auth_in_prod = true` causes startup to fail.

## Verifying

Send requests and watch logs:

```bash
sudo journalctl -u aperture-router -f | grep -E 'rate|banned|429'
```

Log messages:

- `Rate-limited authentication attempt from banned IP: <ip>`
- `Rate limiter evicting oldest IP <ip>`

## Example: strict public profile

```toml
[rate_limit]
requests_per_second = 5
burst_size = 10
health_requests_per_second = 10
health_burst_size = 20

[security]
max_auth_attempts = 3
auth_window_secs = 60
ban_duration_secs = 600
```
