# Use aperture-router with Claude Code

> Connect Anthropic's Claude Code CLI to models behind aperture-router.

## Overview

Claude Code speaks the Anthropic Messages API (`/v1/messages`). aperture-router exposes a compatible `/v1/messages` endpoint and can either pass through to an Anthropic-style provider or convert the request to OpenAI format and back.

## 1. Start aperture-router

```bash
aperture-router
```

Ensure it is reachable:

```bash
curl http://127.0.0.1:8765/health
```

## 2. Configure Claude Code

Point Claude Code at the router instead of `api.anthropic.com`. Set the API base URL to your router:

```
http://127.0.0.1:8765
```

Provide an aperture-router API key (any value from `security.api_keys` or `admin_api_keys`) as the API key. The router accepts the key via either the `Authorization: Bearer <key>` header or the `x-api-key` header.

### Environment variables

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:8765
export ANTHROPIC_API_KEY=your-aperture-router-api-key
```

### Settings file

In your Claude Code settings, set the API base to `http://127.0.0.1:8765`.

## 3. Pick a model

List models the router has discovered:

```bash
curl http://127.0.0.1:8765/v1/models \
  -H "x-api-key: your-aperture-router-api-key"
```

Use any returned `id` as the `model` field in Claude Code.

## Streaming

Claude Code uses streaming (`"stream": true`). aperture-router supports SSE streaming on `/v1/messages`:

- **Anthropic-style provider** (e.g. Z.ai Anthropic endpoint): true SSE passthrough, no buffering.
- **OpenAI-style provider**: request is converted to OpenAI format, streamed, and each chunk is converted back to Anthropic SSE events (`message_start`, `content_block_delta`, etc.).

Keep-alive pings are sent every `http.sse_keep_alive_secs` (default 15s).

## Extended thinking

Anthropic "thinking" content blocks are detected in the stream. By default they are **filtered out**. To include them, set `"include_thinking": true` in the request body.

## Tool calling

OpenAI-format `tool_calls` and Anthropic `tool_use` content blocks are passed through unchanged in both directions. The router logs when tool calls are detected.

## Model aliases

If your Claude Code workflow expects a specific model name, alias it:

```toml
[model_aliases]
claude-3-5-sonnet-20241022 = "glm-5"
```

## Verifying the connection

```bash
curl -X POST http://127.0.0.1:8765/v1/messages \
  -H "x-api-key: your-aperture-router-api-key" \
  -H "content-type: application/json" \
  -d '{
    "model": "glm-5",
    "max_tokens": 256,
    "messages": [{"role": "user", "content": "Say hello."}]
  }'
```

A successful response is JSON in Anthropic Messages format with a `content` array.

## Session tracking

Send an `X-Session-ID` header (UUID) to group requests from one Claude Code session in logs. If omitted, the router generates one and returns it in the `X-Session-ID` response header.

## Troubleshooting

| Symptom | Cause / Fix |
|---------|-------------|
| `401 Unauthorized` | Missing/invalid API key; ensure key is in `security.api_keys`. |
| `404` model not found | Run `aperture-router config fetch --url <gateway>` to confirm the model exists. |
| `502 Bad Gateway` | Upstream provider error; check `journalctl` for `Upstream request failed`. |
| `429 Too Many Requests` | Auth rate limit or per-IP rate limit hit; see [configure-rate-limiting](configure-rate-limiting.md). |
| Streaming hangs | Increase `http.request_timeout_secs`; verify `sse_keep_alive_secs`. |
