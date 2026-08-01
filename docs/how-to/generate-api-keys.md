# Generate and manage API keys

> Create strong API keys and manage regular vs admin key scopes.

## Key types

| Type | Config field | Scope |
|------|--------------|-------|
| Regular | `security.api_keys` | `/v1/*` routes (models, chat, messages, proxy) |
| Admin | `security.admin_api_keys` | `/admin/*` routes **and** all regular routes |

Admin keys are **not** a fallback for regular keys — they are a separate set. However, an admin key is accepted by the regular auth middleware too (it is checked against both lists). A regular key is **never** accepted for admin endpoints.

If `admin_api_keys` is empty, `/admin/*` endpoints return `401` and a warning is logged at startup.

## Strength requirements

Validated in `Config::validate()` (`src/config.rs`):

| Rule | Regular | Admin |
|------|---------|-------|
| Minimum length | 32 characters | 32 characters |
| Minimum unique characters | 20 | 20 |
| Placeholder values (`your-api-key-here`, etc.) | Rejected | Rejected |

A key shorter than 32 chars or with fewer than 20 unique characters fails validation and the server will not start.

## Generating a key

### Option 1: Built-in generator

```bash
aperture-router config generate --url http://100.100.100.100 --generate-key
```

Produces a key prefixed with `apr_`, base62-encoded from two UUIDv4 values (≥36 chars, well over the 32-char minimum). The key is printed once to stdout.

### Option 2: openssl

```bash
openssl rand -base64 32
# Example output: 7vK3k9w2Np8qX4zR6mB1cY5tF0hJ2sL7
```

Base64 of 32 random bytes yields ~44 chars with >20 unique characters.

### Option 3: /dev/urandom

```bash
head -c 32 /dev/urandom | base64
```

### Option 4: Python

```bash
python3 -c 'import secrets; print(secrets.token_urlsafe(32))'
```

## Adding keys to config

```toml
[security]
api_keys = [
  "apr_7vK3k9w2Np8qX4zR6mB1cY5tF0hJ2sL7dA8",
  "apr_anotherKeyWith32PlusCharsAnd20Uniq",
]
admin_api_keys = [
  "apr_adminKeyWith32PlusCharsAnd20Unique",
]
```

Multiple keys are allowed in each list. Any key in `api_keys` grants regular access; any key in `admin_api_keys` grants admin access.

## Via environment variable

For a single key without a config file:

```bash
export APERTURE_BASE_URL=http://100.100.100.100
export APERTURE_API_KEY=apr_your-strong-key
aperture-router
```

`APERTURE_API_KEY` populates `security.api_keys` (regular key only). It does **not** set an admin key. The variable is removed from the process environment after loading to prevent leakage via `/proc/[pid]/environ`.

## Distributing keys

- Store keys only in `/etc/aperture-router/config.toml` (mode `0600`) or `/etc/sysconfig/aperture-router` (mode `0644`, root-only readable).
- Never commit keys to version control.
- Rotate keys by editing the config and restarting: `sudo systemctl restart aperture-router`.
- To revoke access for a client, remove its key from the list and restart.

## Key comparison

Keys are compared with `subtle::ConstantTimeEq` (constant-time, no short-circuit). All keys in both lists are compared on every request to avoid timing oracles. See [security-model](../reference/security-model.md).

Keys in memory are wrapped in `Zeroizing<String>` (`zeroize` crate) and wiped on drop.

## Verifying a key

```bash
curl -X POST http://127.0.0.1:8765/v1/chat/completions \
  -H "Authorization: Bearer apr_your-key" \
  -H "content-type: application/json" \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"ping"}]}'
```

Admin key test:

```bash
curl http://127.0.0.1:8765/admin/stats \
  -H "x-api-key: apr_your-admin-key"
```

A `401` means the key is missing, invalid, or not in the correct list. A `429` means the IP is banned from too many failed attempts.

## Logging

Keys are never logged in plaintext. On successful admin auth, a SHA-256 hash truncated to 8 bytes (`key_[hex]`) is logged for correlation. Regular auth successes log only the client IP.
