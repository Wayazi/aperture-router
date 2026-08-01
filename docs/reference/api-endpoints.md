# API Endpoints Reference

> All HTTP endpoints exposed by aperture-router, with request/response examples.

Source: `src/routes/`, `src/server.rs`.

## Base URL

Default: `http://127.0.0.1:8765`

## Authentication

All endpoints except `/health` require an API key. Send it via either:

```
Authorization: Bearer <your-api-key>
```

or

```
x-api-key: <your-api-key>
```

Admin endpoints (`/admin/*`) require an admin API key (configured separately via `security.admin_api_keys`).

---

## Health

### `GET /health`

Unauthenticated liveness probe.

**Response:**
```json
{
  "status": "ok",
  "service": "aperture-router",
  "version": "0.3.1"
}
```

---

## Models

### `GET /v1/models`

List all models discovered from the Aperture gateway.

**Response (OpenAI format):**
```json
{
  "object": "list",
  "data": [
    {
      "id": "gpt-4",
      "object": "model",
      "created": 0,
      "owned_by": "default"
    }
  ]
}
```

---

## Chat Completions (OpenAI)

### `POST /v1/chat/completions`

OpenAI-compatible endpoint. Forwards to the Aperture gateway or a configured OpenAI-style provider.

**Request:**
```json
{
  "model": "gpt-4",
  "messages": [
    {"role": "user", "content": "Hello"}
  ],
  "max_tokens": 100,
  "temperature": 0.7,
  "stream": false
}
```

**Response (non-streaming):**
```json
{
  "id": "chatcmpl-...",
  "object": "chat.completion",
  "created": 1719000000,
  "model": "gpt-4",
  "choices": [
    {
      "index": 0,
      "message": {"role": "assistant", "content": "Hi there!"},
      "finish_reason": "stop"
    }
  ],
  "usage": {"prompt_tokens": 1, "completion_tokens": 3, "total_tokens": 4}
}
```

**Validation limits:**

| Field | Limit | Error code |
|-------|-------|------------|
| `messages` | max 10000 (configurable) | `too_many_messages` |
| `max_tokens` | 1–1,000,000 | `invalid_max_tokens` |
| Message `role` | `system`, `user`, `assistant`, `tool` | `invalid_role` |
| Message content | max 1,000,000 chars | `invalid_content` |
| Extra fields | max 50 | `too_many_fields` |

---

## Messages (Anthropic)

### `POST /v1/messages`

Anthropic-compatible endpoint. Supports two modes:

1. **Direct forwarding** — if an Anthropic-style provider exists, the request is forwarded unconverted.
2. **Format conversion** — if no Anthropic provider exists, the request is converted to OpenAI format, forwarded, and the response is converted back to Anthropic format.

**Request (Anthropic format):**
```json
{
  "model": "claude-3-opus",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "Hello"}
  ],
  "system": "You are helpful.",
  "stream": false
}
```

**Response (non-streaming):**
```json
{
  "id": "msg_...",
  "type": "message",
  "role": "assistant",
  "content": [
    {"type": "text", "text": "Hi there!"}
  ],
  "model": "claude-3-opus",
  "stop_reason": "end_turn",
  "stop_sequence": null,
  "usage": {"input_tokens": 10, "output_tokens": 5}
}
```

**Streaming response** (`stream: true`):

SSE events in Anthropic format:
```
event: message_start
data: {"type":"message_start","message":{"id":"msg_...","type":"message","role":"assistant","content":[],"model":"...","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":5}}

event: message_stop
data: {"type":"message_stop"}
```

---

## Streaming Proxy

### `POST /v1/proxy`

Generic streaming proxy. Auto-detects format (OpenAI or Anthropic) based on request shape.

**Request:** Must include `"stream": true`.

```json
{
  "model": "gpt-4",
  "messages": [{"role": "user", "content": "Tell me a story"}],
  "stream": true,
  "max_tokens": 500
}
```

**Response:** SSE stream. Preserves upstream event types. Supports `include_thinking` flag to control whether Anthropic thinking blocks are forwarded.

**Endpoint auto-detection:**
- If `max_tokens` is present → forwards to `v1/messages` (Anthropic)
- Otherwise → forwards to `v1/chat/completions` (OpenAI)

---

## Admin

### `POST /admin/refresh-models`

Force a model refresh from the Aperture gateway. Requires admin API key.

**Response:**
```json
{
  "success": true,
  "models_count": 15,
  "providers_count": 3,
  "providers": ["glm", "openrouter", "default"],
  "models": [
    {"id": "gpt-4", "provider": "default"},
    {"id": "glm-5", "provider": "glm"}
  ]
}
```

### `GET /admin/stats`

Server statistics. Requires admin API key.

**Response:**
```json
{
  "models_count": 15,
  "providers_count": 3,
  "providers": [
    {"name": "default", "models_count": 5},
    {"name": "glm", "models_count": 7},
    {"name": "openrouter", "models_count": 3}
  ],
  "version": "0.3.1",
  "refresh_interval_secs": 300
}
```

---

## Session Tracking

All requests include a `request_id` and `session_id` in tracing logs. The server accepts and returns the `X-Session-ID` header:

```bash
# First request — server generates session ID
curl -X POST http://127.0.0.1:8765/v1/chat/completions \
  -H "x-api-key: your-key" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"Hi"}]}'

# Response headers include: X-Session-ID: 550e8400-...

# Subsequent requests — reuse session ID for log grouping
curl -X POST http://127.0.0.1:8765/v1/chat/completions \
  -H "x-api-key: your-key" \
  -H "x-session-id: 550e8400-..." \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"Follow-up"}]}'
```

---

## Error Formats

### OpenAI errors (from `/v1/chat/completions`)

```json
{
  "error": {
    "message": "Too many messages (max 10000)",
    "type": "invalid_request_error",
    "code": "too_many_messages"
  }
}
```

### Anthropic errors (from `/v1/messages`)

```json
{
  "type": "error",
  "error": {
    "type": "invalid_request_error",
    "message": "max_tokens must be greater than 0"
  }
}
```

### Generic errors (from `/v1/proxy`)

```json
{
  "error": {
    "message": "Stream must be set to true for this endpoint",
    "type": "invalid_request_error",
    "code": "invalid_request"
  }
}
```

### HTTP status codes

| Status | Meaning |
|--------|---------|
| 200 | Success |
| 400 | Bad request (validation error) |
| 401 | Unauthorized (missing/invalid API key) |
| 429 | Too many requests (rate limited or banned) |
| 500 | Internal server error |
| 502 | Bad gateway (upstream failure) |
