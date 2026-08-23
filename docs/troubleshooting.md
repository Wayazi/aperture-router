# Troubleshooting

Common issues, their causes, and fixes. Error messages are quoted from the source code.

---

## Startup Errors

### "No config file found and APERTURE_BASE_URL not set"

**Cause:** The server can't find a config file at the expected path, and the
`APERTURE_BASE_URL` environment variable is not set. The full error includes quick-start
suggestions.

**Fix:** Set the environment variable:

```bash
export APERTURE_BASE_URL=http://your-aperture-gateway:8080
aperture-router
```

Or generate a config file:

```bash
aperture-router config generate --url http://your-aperture-gateway:8080
```

Or run the wizard:

```bash
aperture-router config wizard
```

**Where:** `src/main.rs:204`

---

### "Production mode requires authentication but no API keys configured"

**Cause:** Running a release build with `require_auth_in_prod = true` (the default) and no
API keys configured. This is a safety gate to prevent running an open proxy in production.

**Fix (pick one):**

1. **Add an API key** (recommended):
   ```bash
   export APERTURE_CLIENT_API_KEYS=$(openssl rand -hex 24)
   ```
   Or generate a config with a key:
   ```bash
   aperture-router config generate --url http://gateway --generate-key
   ```
   Or add keys to the config file:
   ```toml
   [security]
   api_keys = ["your-key-at-least-32-chars"]
   ```

2. **Disable auth** (development only, not recommended):
   ```bash
   export APERTURE_ALLOW_NO_AUTH=1
   ```

**Note:** In debug builds (`cargo run`), this is a warning, not an error.

**Where:** `src/main.rs:242`, `src/config.rs:587`

---

### "Failed to read config file" / "Failed to parse config file"

**Cause:** The config file exists but can't be read (permissions) or parsed (invalid TOML).

**Fix:** Check permissions:
```bash
ls -la /etc/aperture-router/config.toml
# Should be: -rw------- (600) or -rw-r----- (640)
sudo chmod 640 /etc/aperture-router/config.toml
sudo chown root:aperture-router /etc/aperture-router/config.toml
```

Validate the TOML syntax:
```bash
aperture-router config validate
```

**Where:** `src/config.rs:369-373`

---

### "API key too short: N characters (minimum 32)"

**Cause:** A key in `security.api_keys` or `security.admin_api_keys` is shorter than 32
characters.

**Fix:** Generate a longer key:
```bash
openssl rand -base64 24  # produces ~32 chars
```

**Where:** `src/config.rs:448`

---

### "API key has insufficient entropy (minimum 20 unique characters)"

**Cause:** A key is long enough but has too few unique characters (e.g.
`"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"`).

**Fix:** Use a randomly generated key, not a repeated pattern.

**Where:** `src/config.rs:456`

---

### "API key contains placeholder value or is empty"

**Cause:** The Aperture API key is literally `"your-api-key-here"` or empty.

**Fix:** Set a real key.

**Where:** `src/config.rs:436`

---

### "HTTPS required for non-Tailscale Aperture gateway when API key is configured"

**Cause:** An API key is configured for the Aperture gateway, but the `base_url` uses HTTP
(not HTTPS), and the host is not Tailscale (`100.100.*` or `*.tsnet.*`) or localhost.

**Fix (pick one):**
1. Use HTTPS: `export APERTURE_BASE_URL=https://your-gateway`
2. Use Tailscale: the `100.64.0.0/10` CGN range is allowed over HTTP
3. Use localhost: `http://127.0.0.1:8080` is allowed
4. Remove the API key (if the gateway doesn't require auth)

**Where:** `src/proxy/client.rs:64`

---

### "Port already in use"

**Cause:** Another process is listening on the configured port (default 8765).

**Fix:** Change the port:
```bash
export APERTURE_PORT=8766
```
Or in config:
```toml
port = 8766
```

Or find and stop the conflicting process:
```bash
ss -tlnp | grep 8765
```

**Where:** TCP bind fails in `src/main.rs:261`

---

### "Invalid server address"

**Cause:** The `host:port` combination is malformed (e.g. invalid IP or port out of range).

**Fix:** Check `host` and `port` in config. Host must be a valid IP; port must be 1–65535.

**Where:** `src/config.rs:411`

---

## Request Errors

### "Model 'X' not found"

**Cause:** The requested model doesn't exist in the provider registry or discovery. This
only triggers when `multi_provider_enabled = true`. When disabled, all models are forwarded
to Aperture (which returns its own error).

**Fix:**
1. List available models: `curl http://127.0.0.1:8765/v1/models`
2. Check the model ID spelling (case-sensitive)
3. Check if the model was discovered: look for `"Discovered N models"` in logs
4. If using aliases, verify `model_aliases` in config

**Where:** `src/routes/chat.rs:151`, `src/routes/messages.rs:644`

---

### Claude Code says "Request too large (max 32MB)"

**Error (shown by Claude Code):**
```text
Request too large (max 32MB). Accumulated images and attachments in the conversation
pushed the request over the limit. Run /compact, or double press esc to go back and
remove attachments.
```

**Cause:** Claude Code shows this message for *any* `413` response when
`ANTHROPIC_BASE_URL` points at a gateway — including this router's own body limit.
The "32MB" figure is Anthropic's first-party API cap and is misleading here: the
router rejects requests larger than `security.max_body_size_bytes` (default 10 MB)
with `413 Payload Too Large`, and Claude Code replaces that response with its canned
32 MB text. Typical trigger: pasted screenshots accumulate as base64 in conversation
history until the serialized request exceeds the configured limit. Check the journal:
`status=413 Payload Too Large` on `POST /v1/messages` confirms the router rejected it.

**Fix:** Raise the inbound limit (validation caps it at 100 MB):
```toml
[security]
max_body_size_bytes = 104857600  # 100MB
```
Then restart the service (`sudo systemctl restart aperture-router`). If requests still
fail above 32 MB against an upstream that enforces Anthropic's real 32 MB cap,
`/compact` remains the only fix — that payload genuinely cannot be sent.

**Where:** `src/server.rs:365`, `src/config.rs:557`

---

### "too_many_messages"

**Error:**
```json
{"error":{"message":"Too many messages (max 10000)","type":"invalid_request_error","code":"too_many_messages"}}
```

**Cause:** The request has more messages than `security.max_messages` (default 10 000). Before
v0.3.1, this was hardcoded to 1000, which broke long agentic sessions.

**Fix:** Increase the limit in config:
```toml
[security]
max_messages = 50000
```

**Where:** `src/routes/chat.rs:67`, `src/routes/messages.rs:552`, `src/routes/streaming.rs:78`

---

### "invalid_model_name"

**Cause:** The model name fails validation. Reasons:
- Empty string
- Longer than 128 characters
- Contains `..` (path traversal)
- Contains characters other than `a-z A-Z 0-9 - _ . /`

**Fix:** Use a valid model identifier.

**Where:** `src/types/validation.rs:4`, `src/routes/mod.rs:25`

---

### "invalid_max_tokens"

**Cause:** `max_tokens` is 0 or exceeds 1 000 000.

**Fix:** Set a value between 1 and 1000000.

**Where:** `src/types/validation.rs:53`

---

### "invalid_role"

**Error:**
```json
{"error":{"message":"Invalid role in message 3: Invalid role 'developer'. Must be 'system', 'user', 'assistant', or 'tool'","type":"invalid_request_error","code":"invalid_role"}}
```

**Cause:** A message has a role other than `system`, `user`, `assistant`, or `tool`. Note:
OpenAI's `developer` role is not accepted; map it to `system`.

**Where:** `src/types/validation.rs:30`

---

### "invalid_content"

**Cause:** A message's text content exceeds 1 MB (1 000 000 characters).

**Fix:** Reduce the message size or split the request.

**Where:** `src/types/validation.rs:40`

---

### "too_many_fields"

**Error:**
```json
{"error":{"message":"Too many extra fields (max 50)","type":"invalid_request_error","code":"too_many_fields"}}
```

**Cause:** The request JSON has more than 50 unrecognized fields (stored in the `other`
HashMap). This is a DoS protection.

**Fix:** Remove unnecessary fields from the request body.

**Where:** `src/routes/chat.rs:124`, `src/routes/messages.rs:606`

---

### "too_many_content_blocks"

**Cause:** A streaming request message has more than 100 content blocks in its content array.

**Where:** `src/routes/streaming.rs:133`

---

## Authentication Errors

### 401 Unauthorized

**Cause:** Missing or invalid API key. The proxy checks both `Authorization: Bearer <key>`
and `x-api-key: <key>` headers.

**Fix:** Send a valid key:
```bash
curl -H "Authorization: Bearer your-key" http://127.0.0.1:8765/v1/models
```

**Where:** `src/middleware/auth.rs:313`

---

### 429 Too Many Requests

**Cause:** Either:
- The client IP is banned (too many failed auth attempts), or
- The per-client rate limit was exceeded (`burst_size` requests in the window).

**Fix:** Wait for the ban/window to expire (default 300s ban, 60s rate-limit window). If
persistent, increase `burst_size` in config or reduce request frequency.

**Where:** `src/middleware/auth.rs:297`, `src/middleware/rate_limit.rs:49`

---

## Connection and Upstream Errors

### 502 Bad Gateway

**Cause:** The upstream provider returned an error or was unreachable. Possible reasons:
- Aperture gateway is down or unreachable
- All providers failed (with multi-provider enabled)
- The provider returned a non-success status
- Connection error (DNS, timeout, refused)

**Fix:**
1. Verify Aperture is running: `curl http://your-aperture-gateway:8080/v1/models`
2. Check Tailscale: `tailscale status`
3. Check firewall allows outbound connections
4. Look in logs for `"Provider 'X' returned Y"` or `"Provider 'X' connection error"`

**Where:** `src/routes/proxy.rs:189`, `src/routes/messages.rs:174`

---

### "Service temporarily unavailable"

**Cause:** Generic error returned to the client when an upstream request fails. The detailed
error is logged internally (to avoid leaking upstream internals).

**Real-world example (gateway migration):** Claude Code showed `API Error: 502` with no
detail, while the journal held the actual cause:
```text
ERROR ... Upstream streaming request to http://<new-gateway-ip>/v1/chat/completions failed
with status: 404 Not Found body: {"error":{"message":"no route found for model
\"<model-id>\" for user \"<tailnet-user>\""},"source":"aperture"}
```
The configured gateway address had been changed to a node that only routes a subset of
models; the requested model existed only on the original node. The 502 was correct behavior
— the diagnosis path is always: read the ERROR line directly above the WARN/502 in the
journal, then probe the upstream yourself:
```bash
journalctl -u aperture-router --since "10 min ago" | grep -E "ERROR|failed"
curl -s http://<upstream-ip>/v1/models | jq -r '.data[].id'   # does the model exist there?
```

**Fix:** Check server logs with `journalctl -u aperture-router -p warn` for the actual
upstream error.

**Where:** `src/proxy/client.rs:118`

---

### "Failed to fetch models" / "Failed to fetch models after 3 attempts"

**Cause:** The discovery background task couldn't reach Aperture's `/v1/models` endpoint
after 3 retries with backoff.

**Fix:**
1. Verify `APERTURE_BASE_URL` is correct and reachable
2. Check if Aperture requires an API key (`aperture.api_key` in config)
3. Check Tailscale connectivity
4. Look for `"Connection error fetching models"` in logs

**Where:** `src/discovery/models.rs:253`

---

## Streaming Errors

### "Stream buffer overflow"

**Cause:** The SSE converter's line buffer exceeded 1 MB. This happens when an upstream
sends data without newlines, or sends extremely long lines.

**Fix:** This is likely an upstream bug. Check if the provider returns properly formatted
SSE (lines terminated with `\n`).

**Where:** `src/types/conversion.rs:613`

---

### "Streaming response size limit exceeded"

**Cause:** The cumulative streaming response exceeded `max_streaming_size_bytes` (default
100 MB).

**Fix:** Increase the limit (if legitimate) or reduce the response size:
```toml
[security]
max_streaming_size_bytes = 209715200  # 200MB
```

**Where:** `src/proxy/client.rs:414`

---

### "Streaming request to X timed out"

**Cause:** The streaming request to a provider didn't establish a connection within
`request_timeout_secs` (default 300s).

**Fix:** Increase the timeout or check provider connectivity:
```toml
[http]
request_timeout_secs = 600
```

**Where:** `src/proxy/client.rs:399`

---

### "Stream interrupted"

**Cause:** An error occurred mid-stream (chunk error or UTF-8 decode failure). The stream
emits an error event and terminates.

**Fix:** Check logs for `"Stream chunk error"` or `"Stream error"`. Likely an upstream
issue or network instability.

**Where:** `src/routes/messages.rs:392`

---

## SSRF Protection Errors

### "Access to metadata endpoint 'X' is blocked (SSRF protection)"

**Cause:** A provider's `base_url` points to a cloud metadata endpoint
(`169.254.169.254`, `metadata.google.internal`, etc.).

**Fix:** Use a legitimate provider URL. This block cannot be overridden.

**Where:** `src/proxy/client.rs:249`

---

### "Access to internal IP 'X' is blocked (SSRF protection)"

**Cause:** A provider's `base_url` points to a private/loopback/link-local IP (not in the
CGN range `100.64.0.0/10`, which is allowed for Tailscale).

**Fix:** Use a public IP, a Tailscale IP (`100.64.x.x`), or a hostname that resolves to a
public IP.

**Where:** `src/proxy/client.rs:259`

---

### "DNS rebinding blocked: X resolved to internal IP Y"

**Cause:** A provider hostname resolved to an internal IP at request time. This is a DNS
rebinding attack indicator.

**Fix:** Check the provider's DNS configuration. If legitimate, use the IP directly (it will
be validated as a literal) or fix the DNS record.

**Where:** `src/proxy/client.rs:573`

---

## Conversion Issues

### Tool calls not working with Anthropic clients

**Cause:** The Anthropic `tool_use` / `tool_result` blocks must map correctly to OpenAI's
`tool_calls` / `tool` role messages. If the upstream provider doesn't support OpenAI tool
calling, the converted request will lack `tool_calls`.

**Fix:** Verify the provider supports function calling. Check logs for the converted request
body (debug logging: `aperture-router --debug`).

### Thinking blocks missing in response

**Cause:** The `thinking` field is stripped from the request (no OpenAI equivalent). Whether
the model produces `reasoning_content` depends on the upstream provider, not the proxy.

**Fix:** Check if the upstream provider supports reasoning models. The converter maps
`reasoning_content` → `thinking` blocks when present.

### `cache_control` not working

**Cause:** `cache_control` is stripped from both top-level and per-block content (v0.3.1)
because upstream OpenAI-style providers reject unknown fields.

**Fix:** This is by design. Prompt caching is not supported through the conversion path.

---

## Admin Endpoint Issues

### Admin endpoints return 401

**Cause:** No `admin_api_keys` are configured, or the wrong key was provided. Admin endpoints
require explicit admin keys — regular `api_keys` are not accepted.

**Fix:** Configure admin keys:
```toml
[security]
admin_api_keys = ["your-admin-key-at-least-32-chars"]
```

**Where:** `src/middleware/auth.rs:340`

---

### "No admin API keys configured. Admin endpoints (/admin/*) will be inaccessible."

**Cause:** Warning logged at startup when `admin_api_keys` is empty.

**Fix:** Add admin keys to config if you need admin endpoints.

**Where:** `src/middleware/auth.rs:60`

---

## Performance Issues

### High memory usage

**Fix:** Reduce limits:
```toml
[security]
max_streaming_size_bytes = 52428800  # 50MB
max_body_size_bytes = 5242880        # 5MB
```

Check for leaked background tasks (shouldn't happen after v0.3.0's CancellationToken
architecture). Restart the service if memory grows unbounded.

### Slow responses

**Fix:**
1. Check upstream latency: `time curl http://gateway/v1/models`
2. Increase timeouts: `request_timeout_secs = 600`
3. Check if rate limiting is too aggressive
4. Enable debug logging to see where time is spent

---

## Getting Help

- **Logs:** `journalctl -u aperture-router -f` (systemd) or run with `--debug`
- **Health:** `curl http://127.0.0.1:8765/health`
- **Models:** `curl -H "Authorization: Bearer key" http://127.0.0.1:8765/v1/models`
- **Stats:** `curl -H "x-api-key: admin-key" http://127.0.0.1:8765/admin/stats`
- **GitHub Issues:** https://github.com/Wayazi/aperture-router/issues
