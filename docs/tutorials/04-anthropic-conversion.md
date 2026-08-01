# Tutorial 4: Anthropic ↔ OpenAI Format Conversion

> **Difficulty:** Beginner–Intermediate | **Time needed:** ~10 min | **OS:** Linux

## What you'll learn

By the end of this tutorial you will understand:

- Why conversion is needed and when the router does it
- How Anthropic requests are translated into OpenAI requests
- How OpenAI responses are translated back into Anthropic format
- How streaming (SSE) conversion works, event by event
- How to send Anthropic-format requests to an OpenAI-only provider

## Prerequisites

- [ ] Completed [Tutorial 1: Getting Started](01-getting-started.md)
- [ ] Completed [Tutorial 2: Configuring Providers](02-configuring-providers.md)
- [ ] aperture-router running with at least one `openai_v1` or `openai_direct` provider
- [ ] `curl` for testing
- [ ] Basic familiarity with JSON request bodies (no deep expertise needed)

> **Why does this matter?** Claude Code and OpenCode speak the **Anthropic**
> `/v1/messages` format. But many providers (Aperture's OpenAI gateway, Z.ai v4,
> OpenRouter) speak the **OpenAI** `/v1/chat/completions` format. Without
> conversion, an Anthropic client can't talk to an OpenAI model. aperture-router
> bridges this gap transparently — you point your Anthropic tool at the router
> and it handles the rest.

---

## Step 1: The problem conversion solves

Here's an Anthropic-format request (what Claude Code sends):

```json
{
  "model": "glm-4.7",
  "max_tokens": 256,
  "system": "You are concise.",
  "messages": [
    { "role": "user", "content": "Say hello" }
  ]
}
```

And here's the OpenAI-format request an OpenAI provider expects:

```json
{
  "model": "glm-4.7",
  "max_completion_tokens": 256,
  "messages": [
    { "role": "system", "content": "You are concise." },
    { "role": "user", "content": "Say hello" }
  ]
}
```

The differences aren't cosmetic — they affect field names
(`max_tokens` → `max_completion_tokens`), message structure (Anthropic's
top-level `system` becomes an OpenAI `system` message), content blocks, tool
schemas, and the streaming event protocol. Doing this by hand for every request
is impractical.

> **What the router does:** When a request arrives at `/v1/messages` (Anthropic
> endpoint) and the matching provider is **not** `anthropic`-style, the router
> automatically converts the request to OpenAI format, forwards it, and converts
> the response back to Anthropic format. Your Anthropic client never knows the
> difference.

✅ **Checkpoint:** You understand that conversion is about making an Anthropic
client and an OpenAI provider agree without either side changing.

---

## Step 2: See the routing decision in logs

Start the router with debug logging so you can watch conversion happen:

```bash
aperture-router --debug
```

> **What this does:** Enables `debug`-level tracing for the router, HTTP layer,
> and Axum. You'll see detailed logs about which provider is chosen and whether
> conversion is applied.

Now send an Anthropic-format request for a model backed by an
`openai_v1`/`openai_direct` provider:

```bash
curl -X POST http://127.0.0.1:8765/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: your-api-key" \
  -d '{
    "model": "glm-4.7",
    "max_tokens": 64,
    "messages": [{"role": "user", "content": "Say hello in one word"}]
  }'
```

Watch the router logs:

```
INFO  No Anthropic-style provider for model 'glm-4.7', converting to OpenAI format
DEBUG Converted OpenAI request fields: ["model", "messages", "max_completion_tokens"]
INFO  Routing Anthropic request for model 'glm-4.7' to provider 'zai-credit'
```

> **What this does:** The `/v1/messages` handler (`src/routes/messages.rs`)
> first looks for providers with `endpoint_style == Anthropic`. If none match,
> it logs the "converting to OpenAI format" line, calls
> `anthropic_request_to_openai`, and forwards the converted body to the OpenAI
> provider's URL.

✅ **Checkpoint:** The logs show the "converting to OpenAI format" message and
you still get a valid Anthropic-shaped response.

---

## Step 3: Understand the request conversion

The function `anthropic_request_to_openai` (in `src/types/conversion.rs`) does
the forward translation. Here's what it maps:

| Anthropic field | OpenAI field | Notes |
|-----------------|--------------|-------|
| `system` (top-level string/blocks) | `messages[0]` with `role: "system"` | Inserted at the start of the messages array |
| `messages[].role` | `messages[].role` | Same values (`user`/`assistant`) |
| `messages[].content` (string or content blocks) | `messages[].content` | Blocks are flattened to text/tool parts |
| `max_tokens` | `max_completion_tokens` | Renamed |
| `temperature` | `temperature` | Pass-through |
| `top_p` | `top_p` | Pass-through |
| `stop_sequences` | `stop` | Renamed |
| `stream` | `stream` | Pass-through |
| `tools` | `tools` | Schema converted (see below) |
| `tool_choice` | `tool_choice` | Converted |
| `metadata.user_id` | `user` | Flattened |
| `thinking`, `top_k` | *(removed)* | No OpenAI equivalent; dropped |

> **What this does:** Builds a fresh OpenAI JSON object field by field, then
> strips Anthropic-only keys (`thinking`, `top_k`, `metadata`, `system`,
> `stop_sequences`, `cache_control`) so the upstream OpenAI provider doesn't
> reject unknown fields.

**Tool conversion example:**

Anthropic tool definition:

```json
{
  "name": "get_weather",
  "description": "Get weather",
  "input_schema": { "type": "object", "properties": { "city": { "type": "string" } } }
}
```

becomes the OpenAI shape:

```json
{
  "type": "function",
  "function": {
    "name": "get_weather",
    "description": "Get weather",
    "parameters": { "type": "object", "properties": { "city": { "type": "string" } } }
  }
}
```

✅ **Checkpoint:** You can name at least four field mappings (system→system
message, max_tokens→max_completion_tokens, stop_sequences→stop, tools schema
wrap).

---

## Step 4: Understand the response conversion

When the OpenAI provider replies, `openai_response_to_anthropic` translates it
back. An OpenAI response:

```json
{
  "id": "chatcmpl-123",
  "model": "glm-4.7",
  "choices": [{
    "message": {
      "role": "assistant",
      "content": "Hello"
    },
    "finish_reason": "stop"
  }],
  "usage": { "prompt_tokens": 10, "completion_tokens": 1 }
}
```

becomes an Anthropic response:

```json
{
  "id": "msg_...",
  "type": "message",
  "role": "assistant",
  "model": "glm-4.7",
  "content": [{ "type": "text", "text": "Hello" }],
  "stop_reason": "end_turn",
  "usage": { "input_tokens": 10, "output_tokens": 1 }
}
```

Key mappings:

| OpenAI | Anthropic | Notes |
|--------|-----------|-------|
| `choices[0].message.content` (string) | `content: [{ type: "text", text: ... }]` | Wrapped in a content block array |
| `choices[0].message.reasoning_content` | `content: [{ type: "thinking", thinking: ... }]` | Placed *before* the text block |
| `choices[0].message.tool_calls` | `content: [{ type: "tool_use", ... }]` | Converted to tool_use blocks |
| `choices[0].finish_reason` | `stop_reason` | `stop`→`end_turn`, `tool_calls`→`tool_use`, `length`→`max_tokens` |
| `usage.prompt_tokens` | `usage.input_tokens` | Renamed |
| `usage.completion_tokens` | `usage.output_tokens` | Renamed |

> **What this does:** Lets Claude Code / OpenCode parse the response as if it
> came from a native Anthropic API. The `reasoning_content` → `thinking` mapping
> is especially important for reasoning models (like GLM-5) so the client can
> display the model's chain-of-thought.

✅ **Checkpoint:** You can explain why `choices[0].message.content` ends up
inside a `content` array with `type: "text"`.

---

## Step 5: Try a non-streaming conversion yourself

Send a richer Anthropic request with a system prompt and tool:

```bash
curl -X POST http://127.0.0.1:8765/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: your-api-key" \
  -d '{
    "model": "glm-4.7",
    "max_tokens": 128,
    "system": "You are a helpful assistant.",
    "messages": [
      { "role": "user", "content": "What is 2+2? Use the calculator tool." }
    ],
    "tools": [{
      "name": "calculator",
      "description": "Evaluate a math expression",
      "input_schema": {
        "type": "object",
        "properties": { "expression": { "type": "string" } },
        "required": ["expression"]
      }
    }]
  }'
```

> **What this does:** The router converts the `system` string into a system
> message, renames `max_tokens`, wraps the tool schema in the OpenAI
> `function` envelope, and forwards to the OpenAI provider. The response comes
> back, and if the model invoked the tool, the `tool_calls` array is converted
> into an Anthropic `tool_use` content block.

Inspect the response. If the model decided to call the tool, you'll see:

```json
{
  "content": [
    { "type": "tool_use", "id": "toolu_...", "name": "calculator", "input": { "expression": "2+2" } }
  ],
  "stop_reason": "tool_use"
}
```

✅ **Checkpoint:** You sent an Anthropic tool-use request and got back an
Anthropic-shaped tool_use block from an OpenAI provider.

---

## Step 6: How streaming conversion works

Streaming is where conversion gets interesting. OpenAI streams
`chat.completion.chunk` SSE events; Anthropic expects a sequence of typed
events (`message_start`, `content_block_start`, `content_block_delta`,
`content_block_stop`, `message_delta`, `message_stop`). The router's
`OpenAIToAnthropicStreamConverter` (in `src/types/conversion.rs`) translates
between them in real time.

Send a streaming Anthropic request:

```bash
curl -N -X POST http://127.0.0.1:8765/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: your-api-key" \
  -d '{
    "model": "glm-4.7",
    "max_tokens": 64,
    "stream": true,
    "messages": [{"role": "user", "content": "Count to 3"}]
  }'
```

> **What this does:** The `-N` flag disables curl's buffering so you see events
> as they arrive. The router sets `stream: true` in the converted OpenAI
> request, opens an SSE connection to the provider, and runs each incoming chunk
> through the stream converter, emitting Anthropic-shaped SSE events to your
> client.

You'll see a sequence like this (each `data:` line is one event):

```
event: message_start
data: {"type":"message_start","message":{"id":"msg_...","type":"message","role":"assistant","content":[],"model":"glm-4.7","stop_reason":null,"usage":{"input_tokens":0,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"1"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":", 2"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":", 3"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":8}}

event: message_stop
data: {"type":"message_stop"}
```

✅ **Checkpoint:** You see `message_start`, at least one `content_block_delta`,
and `message_stop` events in order.

---

## Step 7: The streaming event map

Here's exactly how the converter maps OpenAI chunks to Anthropic events:

| Anthropic event | When it's emitted | Source |
|-----------------|--------------------|--------|
| `message_start` | First chunk arrives | Generated once with a fresh `msg_` id |
| `content_block_start` (text) | First text delta in a new block | When OpenAI `delta.content` starts |
| `content_block_delta` (text_delta) | Each text token | OpenAI `delta.content` |
| `content_block_start` (thinking) | First reasoning delta | OpenAI `delta.reasoning_content` |
| `content_block_delta` (thinking_delta) | Each reasoning token | OpenAI `delta.reasoning_content` |
| `content_block_start` (tool_use) | Tool call begins | OpenAI `delta.tool_calls[i]` with a new index |
| `content_block_delta` (input_json_delta) | Tool argument JSON | OpenAI `delta.tool_calls[i].function.arguments` |
| `content_block_stop` | Block ends (text/tool/reasoning closes) | When block type changes or stream ends |
| `message_delta` | Stream finishing | On OpenAI `finish_reason`; carries `stop_reason` + usage |
| `message_stop` | `[DONE]` from provider | Terminal event |

> **What this does:** The converter maintains state (which blocks are open,
> token counts, message id) across chunks. It buffers partial SSE lines to
> handle chunks split across TCP boundaries, and it caps the buffer at 1 MiB to
> prevent memory exhaustion from malformed streams. The `close_stream` method
> flushes any open blocks and emits `message_delta` + `message_stop` so the
> client always sees a well-formed termination.

**Why reasoning becomes `thinking`:** Anthropic's format has a dedicated
`thinking` content block type for chain-of-thought. OpenAI-compatible reasoning
models expose this as `reasoning_content` in the delta. The converter maps one
to the other so OpenCode/Claude Code can render the reasoning trace.

✅ **Checkpoint:** You can describe what happens when the converter sees
`delta.reasoning_content` for the first time (it opens a `thinking`
`content_block_start`).

---

## Step 8: When conversion does NOT happen

Conversion only triggers when there's no Anthropic-style provider for the
requested model. If an `anthropic`-style provider exists, the router forwards
the request **unchanged** to `/v1/messages` on that provider — no conversion,
no overhead.

The decision logic in `src/routes/messages.rs` is:

1. Find all providers that serve the requested model.
2. Filter to those with `endpoint_style == Anthropic`.
3. If any exist → forward the Anthropic request directly (streaming or not).
4. If none exist → convert to OpenAI, forward to an OpenAI-style provider, convert the response back.

> **What this does:** This means you can mix provider types. A model served by
> both an `anthropic` provider and an `openai_v1` provider will prefer the
> Anthropic path (zero conversion). Only if the Anthropic provider is disabled
> or fails does it fall back to conversion.

You can see this in logs by configuring two providers for the same model — one
`anthropic`, one `openai_v1`. Requests will log "forwarding directly" rather
than "converting to OpenAI format".

✅ **Checkpoint:** You understand the preference order: Anthropic-direct first,
conversion only as fallback.

---

## Step 9: Debugging conversion issues

If a request fails or returns unexpected output:

1. **Run with `--debug`** to see the converted request body:
   ```
   DEBUG Converted OpenAI request fields: ["model", "messages", ...]
   DEBUG Converted non-streaming request body (first 2000 chars): {...}
   ```

2. **Check the upstream provider directly.** Send the *converted* OpenAI body
   straight to the provider's URL with curl. If it fails there too, the problem
   is the provider, not the converter.

3. **Watch for dropped fields.** Anthropic-only fields (`thinking`, `top_k`,
   `cache_control`) are intentionally removed. If a feature depends on one of
   these, it won't survive conversion — use an `anthropic`-style provider
   instead.

4. **Verify streaming termination.** A healthy stream always ends with
   `message_stop`. If your client hangs, the provider may not be sending
   `[DONE]`; the converter's `close_stream` handles this on connection close.

> **What this does:** Debug logging surfaces the exact JSON the router sends
> upstream and the field list after conversion, so you can diff it against what
> the provider expects.

✅ **Checkpoint:** You know where to look (debug logs + direct provider test)
when conversion misbehaves.

---

## You're done!

You now understand how aperture-router makes Anthropic clients and OpenAI
providers interoperate.

### Quick recap

| Step | What we covered |
|------|-----------------|
| 1 | Why conversion is needed |
| 2 | Saw the routing decision in logs |
| 3 | Request field mappings (system, max_tokens, tools, …) |
| 4 | Response field mappings (content blocks, stop_reason, usage) |
| 5 | Tried a tool-use request end to end |
| 6 | Watched streaming conversion live |
| 7 | The full streaming event map |
| 8 | When conversion is skipped (Anthropic provider present) |
| 9 | Debugging tips |

### Key takeaways

- Conversion is **automatic** and **transparent** — clients and providers don't
  change.
- The router prefers **direct Anthropic** forwarding when available; conversion
  is the fallback for OpenAI-only providers.
- Streaming conversion maintains full Anthropic event semantics
  (`message_start` → `content_block_*` → `message_delta` → `message_stop`),
  including `thinking` blocks for reasoning models.
- Debug mode (`--debug`) shows the exact converted payloads for troubleshooting.

### Where to look in the source

| File | What it contains |
|------|------------------|
| `src/types/conversion.rs` | `anthropic_request_to_openai`, `openai_response_to_anthropic`, `OpenAIToAnthropicStreamConverter` |
| `src/routes/messages.rs` | The `/v1/messages` handler, routing decision, failover, streaming wiring |
| `src/provider/registry.rs` | `build_endpoint_url` and provider selection by `endpoint_style` |
| `src/routes/streaming.rs` | SSE plumbing for the `/v1/proxy` streaming endpoint |

### Next steps

- Explore the `/v1/proxy` generic streaming endpoint for raw passthrough.
- Read `config.example.toml` for the full set of security and rate-limit knobs.
- Check `CHANGELOG.md` for the latest conversion improvements.
