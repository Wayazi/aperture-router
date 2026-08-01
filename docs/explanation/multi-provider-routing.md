# Multi-Provider Routing

> How the proxy decides which upstream to send a request to, and what happens when it fails.

## The Routing Problem

A single Aperture gateway can front multiple model providers: GLM, OpenRouter, Anthropic,
local Ollama, etc. Each provider:

- Has its own base URL (possibly with its own API key)
- Speaks a specific wire format (OpenAI v1, OpenAI direct, or Anthropic)
- Serves a specific set of models
- May be temporarily unavailable

The routing layer must answer two questions for every request:

1. **Which provider(s) serve the requested model?**
2. **In what order and format should the request be sent?**

When multi-provider routing is disabled (`multi_provider_enabled = false`, the default), the
answer is simple: send everything to the Aperture gateway. This section covers the
multi-provider case.

## Endpoint Styles

`EndpointStyle` (`src/config.rs:207`) classifies how a provider expects URLs and auth:

| Style | URL pattern | Auth header | Example |
|---|---|---|---|
| `OpenaiV1` | `{base}/v1/chat/completions` | `Authorization: Bearer {key}` | Standard OpenAI-compatible |
| `OpenaiDirect` | `{base}/chat/completions` | `Authorization: Bearer {key}` | GLM's `/api/paas/v4` |
| `Anthropic` | `{base}/v1/messages` | `x-api-key: {key}` | Anthropic API |

`ProviderRegistry::build_endpoint_url` (`src/provider/registry.rs:223`) constructs the full
URL from the provider's `base_url` and the endpoint name, stripping a `v1/` prefix for
`OpenaiDirect` providers.

`add_auth_header` (`src/proxy/client.rs:511`) chooses the header based on style:
`x-api-key` for Anthropic, `Authorization: Bearer` for everything else.

## Model-to-Provider Mapping

`ProviderRegistry` (`src/provider/registry.rs:17`) maintains two maps behind an
`Arc<RwLock<RegistryInner>>`:

- `providers: HashMap<String, Provider>` — provider config by name
- `model_to_provider: HashMap<String, String>` — model ID → provider name

### Construction

`ProviderRegistry::new` (`src/provider/registry.rs:23`) builds the maps from the config's
`providers` list. Only `enabled: true` providers are included. If a model is mapped to
multiple providers, the last one wins (with a warning logged).

### Lookup

`get_provider_for_model` (`src/provider/registry.rs:167`) does a direct map lookup. This is
the fast path used by route handlers.

`get_providers_for_model` (`src/provider/registry.rs:179`) returns **all** providers that
serve the model — first the mapped provider (if enabled), then any other enabled provider
whose `models` list contains the model. This is used for failover.

### Why Two Lookup Methods?

The routing map (`model_to_provider`) is authoritative — it reflects discovery and manual
config. But a model might appear in multiple providers' model lists (e.g. `gpt-4` served by
both an OpenAI provider and an OpenRouter provider). `get_providers_for_model` enumerates
all candidates for failover, while `get_provider_for_model` returns the preferred one for
non-failover routing.

## Discovery Sync

`ModelDiscovery` (`src/discovery/models.rs:72`) fetches the model list from Aperture's
`/v1/models` endpoint. The response includes `metadata.provider.id` for each model, which
Aperture populates. The discovery layer groups models by provider ID.

### Refresh Task

`start_refresh_task` (`src/discovery/models.rs:367`) runs a background tokio task that:

1. Fires immediately on startup (via `interval.tick()` before the loop)
2. Polls every `model_refresh_interval_secs` (default 300s)
3. Calls `fetch_models()` and, on success, calls
   `registry.update_from_discovery(&snapshot.models_by_provider, &aperture_url)`
4. Respects `CancellationToken` for graceful shutdown

### Retry with Backoff

`fetch_models` (`src/discovery/models.rs:111`) retries up to 3 times on server errors (5xx)
and connection failures, with exponential backoff (2s, 4s). 4xx errors fail immediately
(configuration problem, not transient). This was added in v0.3.1.

The discovery HTTP client sends the `x-api-key` header to Aperture when
`aperture.api_key` is configured, so authenticated gateways return the full model list.

### Registry Update

`update_from_discovery` (`src/provider/registry.rs:60`) merges discovered models into the
registry:

1. **Remove stale providers** — providers not in the discovery snapshot are removed from both
   maps.
2. **Add new providers** — providers in the snapshot but not in the registry are added with
   `EndpointStyle::OpenaiDirect` and the Aperture base URL.
3. **Merge models** — for existing providers, discovered models are added to the provider's
   model list without removing manually-configured models.
4. **Rebuild routing map** — discovered models are inserted into `model_to_provider`,
   overwriting any previous mapping.
5. **Preserve manual entries** — manually-configured models (not in discovery) are re-inserted
   via `entry().or_insert()`, so they remain routable even if discovery doesn't list them.
6. **Prune invalid entries** — the routing map is filtered to only contain models that exist
   in some provider's model list, and providers that still exist.

The merge-not-replace behavior was fixed in v0.3.1. Previously, discovery replaced the
entire model list, wiping manually-configured models.

## Request Routing

Each route handler (`chat_completions`, `anthropic_messages`, `handle_proxy_stream`) follows
the same pattern:

1. **Resolve model alias** — `config.resolve_model_alias()` maps aliases (e.g. `gpt-4` →
   `gpt-4-turbo`) before validation.
2. **Validate model name format** — `validate_model_name` checks length, characters, no `..`.
3. **Validate model exists** (only when `multi_provider_enabled`) — checks both
   `provider_registry.get_providers_for_model()` and `discovery.is_valid_model()`.
4. **Get providers** — `get_providers_for_model()` returns the candidate list.
5. **Route** — `proxy_handler_multi()` handles the actual forwarding.

### `proxy_handler_multi`

`src/routes/proxy.rs:149` implements the core routing logic:

- **Multi-provider disabled** → `proxy_to_default_gateway()` sends to the Aperture gateway.
- **No providers found** → same fallback to default gateway.
- **Single provider** → `try_provider()` once. Failure returns `502 Bad Gateway`.
- **Multiple providers** → iterate up to `MAX_FAILOVER_ATTEMPTS` (3) providers, return the
  first successful response.

## Failover

`try_provider` (`src/routes/proxy.rs:110`) attempts one provider:

1. Build the URL via `build_provider_url`.
2. Resolve the API key: if the provider has its own key, use it; otherwise, if the provider's
   base URL matches the Aperture gateway URL, use the default gateway key; otherwise, no key.
3. Call `forward_request_to_url_raw` (no streaming) or `forward_request_stream_to_url`
   (streaming).
4. **On 5xx server error** → return `Err(status)`, signaling the caller to try the next
   provider.
5. **On 4xx client error** → return the error response directly (the request was bad, not
   the provider — retrying won't help).
6. **On connection error** → return `Err(502)`, try next provider.

The key insight: failover only happens on **server-side failures** (5xx, connection errors).
A 400 Bad Request from the first provider is returned to the client immediately, because
retrying the same invalid request against another provider would produce the same error.

`MAX_FAILOVER_ATTEMPTS = 3` caps the failover chain to prevent unbounded latency. If all
attempts fail, the last error status is returned with `"All providers failed"`.

### Streaming Failover

Streaming requests failover differently. In `handle_streaming_conversion`
(`src/routes/messages.rs:304`) and `handle_anthropic_direct_streaming`
(`src/routes/messages.rs:409`), the proxy tries providers sequentially until one returns a
successful stream connection (2xx status). Once streaming begins, failover stops — there's
no way to failover mid-stream without duplicating tokens.

### Anthropic-Direct vs Converted

For `/v1/messages` requests, the handler first checks for Anthropic-style providers
(`EndpointStyle::Anthropic`). If found:

- **Streaming** → `handle_anthropic_direct_streaming` — true SSE passthrough, no conversion.
- **Non-streaming** → `proxy_handler_multi` with the Anthropic body unchanged.

If no Anthropic-style provider exists, the request is converted to OpenAI format and sent
via `handle_non_streaming_conversion` or `handle_streaming_conversion` (with the stream
converter). See [Format Conversion](format-conversion.md).

## API Key Resolution

`safe_api_key` (`src/routes/messages.rs:95`) and `get_provider_api_key`
(`src/routes/proxy.rs:59`) implement the same logic:

1. If the provider has its own `api_key`, use it.
2. Else, if the provider's `base_url` matches the Aperture gateway URL (the proxy's
   configured default), use the gateway's API key.
3. Else, no key (the provider must be open or use a different auth mechanism).

This allows mixing authenticated Aperture providers with direct open providers.

## Model Validation

When `multi_provider_enabled` is true, the handler validates that the requested model exists
before forwarding:

```rust
let provider_has_model = state.provider_registry.get_providers_for_model(&request.model)
    .await.iter().any(|p| p.enabled);
let discovery_has_model = state.discovery.is_valid_model(&request.model).await;
if !provider_has_model && !discovery_has_model {
    return error("Model '{}' not found");
}
```

When multi-provider is disabled, validation is skipped — all models go to Aperture, which
will return its own error if the model is unknown.

## Further Reading

- `src/provider/registry.rs` — routing maps and endpoint URL construction
- `src/discovery/models.rs` — model discovery and refresh
- `src/routes/proxy.rs` — failover logic
- `src/routes/messages.rs` — Anthropic-specific routing (direct vs converted)
- [Format Conversion](format-conversion.md) — what happens when provider and client formats differ
