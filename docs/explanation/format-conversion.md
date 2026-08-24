# Format Conversion: Anthropic ↔ OpenAI

> How the proxy translates between two incompatible API schemas without losing information.

## Why Conversion Is Needed

The OpenAI and Anthropic APIs disagree on almost everything:

| Concept | OpenAI | Anthropic |
|---|---|---|
| System prompt | A `system` message in the `messages` array | A top-level `system` field |
| Content | A string or array of typed parts | Always an array of content blocks |
| Tool calls | `tool_calls` array on the assistant message | `tool_use` content blocks |
| Tool results | A `tool` role message with `tool_call_id` | `tool_result` content blocks |
| Streaming | `delta.content` / `delta.tool_calls` | `content_block_delta` events |
| Max tokens | `max_completion_tokens` | `max_tokens` |
| Stop reason | `finish_reason: "stop"` | `stop_reason: "end_turn"` |
| Thinking | `reasoning_content` on delta | `thinking` content block |

When a tool that speaks Anthropic (e.g. Claude Code) sends a request to a provider that only
speaks OpenAI (e.g. a GLM endpoint behind Aperture), the proxy must translate the request on
the way in and the response on the way back. All conversion logic lives in
`src/types/conversion.rs`.

## Request Conversion: Anthropic → OpenAI

`anthropic_request_to_openai` (`src/types/conversion.rs:15`) transforms the incoming
Anthropic `MessageRequest` JSON into an OpenAI `chat/completions` JSON body.

### System Prompt

The Anthropic top-level `system` field is extracted via `extract_text_from_content` and
prepended as a `{"role": "system"}` message. If the system field contains an array of text
blocks, they are joined with newlines.

### Message Expansion

Anthropic packs multiple logical messages into a single user turn using content blocks. For
example, a user message containing both text and `tool_result` blocks is actually *two*
OpenAI messages: a `user` message with the text, followed by `tool` role messages for each
result.

`convert_anthropic_message_expanded` (`src/types/conversion.rs:239`) handles this:

- **Assistant messages with `tool_use` blocks** become a single OpenAI assistant message with
  a `tool_calls` array. Each `tool_use` block's `input` object is serialized to a JSON string
  for the `function.arguments` field (OpenAI requires a string, Anthropic uses an object).
- **User messages with `tool_result` blocks** become separate `tool` role messages, each with
  `tool_call_id` matching the original `tool_use_id`.
- **Mixed content** (text + tool results in one user turn) splits into a user message
  followed by tool messages.

### Tool Definition and Choice

Anthropic tools use `input_schema`; OpenAI uses `parameters` inside a `function` wrapper.
`convert_anthropic_tool_to_openai` (`src/types/conversion.rs:466`) does the rename and wrap.

`tool_choice` mapping (`src/types/conversion.rs:484`):

| Anthropic | OpenAI |
|---|---|
| `"auto"` | `"auto"` |
| `"any"` | `"required"` |
| `"none"` | `"none"` |
| `{"type":"tool","name":"x"}` | `{"type":"function","function":{"name":"x"}}` |

### Field Mapping

| Anthropic | OpenAI | Notes |
|---|---|---|
| `max_tokens` | `max_completion_tokens` | Renamed; OpenAI's newer field name |
| `stop_sequences` | `stop` | Renamed |
| `temperature` | `temperature` | Passthrough |
| `top_p` | `top_p` | Passthrough |
| `metadata.user_id` | `user` | Mapped in v0.3.1 |
| `top_k` | — | Stripped (no OpenAI equivalent) |
| `thinking` | — | Stripped; reasoning is provider-specific |
| `cache_control` | — | Stripped from top-level and per-block (v0.3.1) |

`cache_control` is stripped because upstream OpenAI-style providers reject unknown fields. The
Anthropic prompt-caching feature has no OpenAI equivalent.

### Tool Result Images

In v0.3.1, `convert_tool_result_content_to_openai` (`src/types/conversion.rs:196`) preserves
structured content in tool results instead of flattening to text. An `image` block inside a
`tool_result` becomes an `image_url` part with a `data:` URI. Previously these were lost.

## Response Conversion: OpenAI → Anthropic

`openai_response_to_anthropic` (`src/types/conversion.rs:95`) transforms the upstream OpenAI
JSON response into an Anthropic `Message` object.

### Content Blocks

The function walks the OpenAI `choices[0].message` and builds a content array:

1. **Thinking block** — if `reasoning_content` is present and non-empty, a
   `{"type":"thinking","thinking":...}` block is emitted first.
2. **Text block** — if `content` is present and non-empty, a `{"type":"text","text":...}` block.
3. **Tool use blocks** — each entry in `tool_calls` becomes a `tool_use` block. The
   `function.arguments` string is parsed back into a JSON object for the `input` field.

If all three are absent, a single empty text block is emitted (Anthropic requires non-empty
`content`).

### Stop Reason

| OpenAI `finish_reason` | Anthropic `stop_reason` |
|---|---|
| `"stop"` | `"end_turn"` |
| `"length"` | `"max_tokens"` |
| `"tool_calls"` | `"tool_use"` |
| anything else | `"end_turn"` |

### Usage

`prompt_tokens` → `input_tokens`, `completion_tokens` → `output_tokens`. The
`stop_sequence` field is read from upstream `usage.stop_sequence` (fixed in v0.3.1; was
previously hardcoded to null).

## Streaming Conversion: OpenAI SSE → Anthropic SSE

This is the most complex part. OpenAI streams `data: {...}` lines with `choices[0].delta`
containing incremental content. Anthropic uses a stateful event sequence:

```
message_start
  content_block_start  (index 0)
    content_block_delta  (repeated)
  content_block_stop   (index 0)
  content_block_start  (index 1)
    content_block_delta  (repeated)
  content_block_stop   (index 1)
message_delta
message_stop
```

`OpenAIToAnthropicStreamConverter` (`src/types/conversion.rs:523`) is a state machine that
tracks:

- Whether `message_start` has been emitted
- Which content blocks are currently open (text, thinking, tool_use)
- A `HashMap<usize, ToolBlockState>` mapping OpenAI's `tool_calls[].index` to Anthropic
  content block indices
- Cumulative `input_tokens` / `output_tokens`

### Delta Processing

Each OpenAI chunk's `delta` is examined for three fields:

1. **`reasoning_content`** — opens a `thinking` block (if not already open) and emits
   `thinking_delta` events. When text or tool content arrives, the thinking block is closed.
2. **`content`** — opens a `text` block and emits `text_delta` events.
3. **`tool_calls`** — for each tool call delta: if it has an `id`, that signals a new tool
   call (emit `content_block_start` with `tool_use`); if it has `arguments`, emit
   `input_json_delta` with the partial JSON string.

The converter allocates each tool call its own slot keyed by the call's `id` (falling back
to `index` when absent); continuation deltas follow the most recently active call. The
`tool_block_order` vector preserves insertion order for block-stop emission.

### Output Token Counting

In v0.3.1, the converter reads `usage.completion_tokens` from upstream when available
(`output_tokens_from_upstream` flag). If the upstream doesn't provide usage in-stream, it
falls back to a per-delta heuristic (increment by 1 per content delta). This is approximate
but prevents reporting zero tokens.

### Line Buffering

SSE chunks can arrive split across TCP frames — a single `data: {...}\n` line may be split
across two `convert_chunk` calls. The converter maintains a `line_buffer` (`String`) and only
processes complete lines (up to `\n`). This is tested in
`test_stream_converter_line_buffer_across_chunks`.

The buffer has a 1 MB cap (`MAX_LINE_BUFFER`, `src/types/conversion.rs:581`). If exceeded
(e.g. a malformed upstream sending without newlines), the stream is closed with an `error`
event to prevent unbounded memory growth.

### Stream Termination

The stream ends when either:

- The OpenAI `data: [DONE]` marker is received → `close_stream()` → `close_with_stop_reason("stop")`
- A `finish_reason` is present on a chunk → `close_with_stop_reason(reason)`
- The line buffer overflows → error event + `close_stream()`

`close_with_stop_reason` first calls `close_open_blocks` to emit `content_block_stop` for any
still-open text/thinking/tool blocks, then emits `message_delta` (with `stop_reason` and
`output_tokens`) and `message_stop`.

## Anthropic-Direct Passthrough

When the upstream provider speaks Anthropic natively (`EndpointStyle::Anthropic`), no
conversion is needed. `handle_anthropic_direct_streaming` (`src/routes/messages.rs:409`)
streams SSE events through with only line-level parsing (`process_sse_chunk_lines_anthropic`,
`src/routes/messages.rs:477`). This preserves event types and avoids the buffering latency of
the converter.

This path was added in v0.3.1 for true SSE passthrough to Anthropic-style providers.

## Known Limitations

1. **`top_k` is dropped** — OpenAI has no equivalent; it is stripped silently.
2. **`thinking` configuration is stripped** — the Anthropic `thinking: {type: "enabled",
   budget_tokens: N}` field is removed. Whether the model actually reasons depends on the
   upstream provider's configuration, not the request.
3. **`cache_control` is stripped** — prompt caching hints are not forwarded.
4. **Token counting is approximate without upstream usage** — the per-delta heuristic
   increments by 1 per content chunk, which undercounts multi-token words.
5. **Tool result `is_error` is flattened** — error tool results get `"Error: "` prefixed to
   their text content, since OpenAI's `tool` role has no error field.
6. **Non-text/image content blocks in tool results** are serialized to JSON strings** —
   unknown block types become `{"type":"text","text":"<json>"}`.
7. **Streaming tool arguments are not validated** — partial JSON is forwarded as-is in
   `input_json_delta`. The client must accumulate and parse.

## Further Reading

- `src/types/conversion.rs` — all conversion logic with inline tests
- `src/types/anthropic.rs` — Anthropic request/response types
- `src/types/openai.rs` — OpenAI request/response types
- [Multi-Provider Routing](multi-provider-routing.md) — how the proxy decides *which*
  provider to send a converted request to
