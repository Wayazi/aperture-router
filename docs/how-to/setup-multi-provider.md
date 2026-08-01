# Setup multi-provider routing

> Configure multiple AI providers (Z.ai, OpenRouter, Aperture, etc.) with different endpoint styles and automatic failover.

## When to use multi-provider

- Route specific models to direct provider APIs (bypass Aperture)
- Use Anthropic-format providers alongside OpenAI-format ones
- Fail over across providers serving the same model

Enable in `config.toml`:

```toml
multi_provider_enabled = true
```

When `false` (default), all requests go to the Aperture gateway regardless of `[[providers]]`.

## Endpoint styles

| Style | Chat URL | Messages URL | Use for |
|-------|----------|--------------|---------|
| `openai_v1` | `base_url/v1/chat/completions` | `base_url/v1/messages` | Standard OpenAI, Aperture |
| `openai_direct` | `base_url/chat/completions` | `base_url/messages` | Z.ai v4, OpenRouter direct |
| `anthropic` | — | `base_url/v1/messages` | Anthropic API, Z.ai Anthropic endpoint |

`endpoint_style` is serialized in `snake_case`. Default is `openai_v1`.

## Provider block

```toml
[[providers]]
name = "unique-name"                 # Required, must be unique
base_url = "https://api.example.com" # Required, http/https only
endpoint_style = "openai_v1"         # Optional, default openai_v1
models = ["model-a", "model-b"]      # Required, non-empty
api_key = "your-key"                 # Optional
enabled = true                       # Optional, default true
```

## Example: Z.ai + Aperture

```toml
multi_provider_enabled = true

[[providers]]
name = "aperture"
base_url = "http://100.100.100.100"
endpoint_style = "openai_v1"
models = ["openrouter/free", "claude-3-opus", "gpt-4"]
enabled = true

[[providers]]
name = "zai-credit"
base_url = "https://api.z.ai/api/paas/v4"
endpoint_style = "openai_direct"
models = ["glm-5", "glm-4.7", "glm-4.7-flash"]
api_key = "your-zai-api-key"

[[providers]]
name = "zai-coding"
base_url = "https://api.z.ai/api/coding/paas/v4"
endpoint_style = "openai_direct"
models = ["glm-5-coding", "glm-4.7-coding"]
api_key = "your-zai-coding-api-key"

[[providers]]
name = "zai-anthropic"
base_url = "https://api.z.ai/api/anthropic"
endpoint_style = "anthropic"
models = ["GLM-5", "glm-4.7"]
api_key = "your-zai-api-key"
```

## API key resolution

For each request, the provider's API key is selected as follows:

1. If `provider.api_key` is set → use it.
2. Else if `provider.base_url` equals the Aperture gateway URL → use the gateway's `aperture.api_key`.
3. Otherwise → no key sent.

This lets a single Aperture key be reused by the `aperture` provider without duplicating it.

## Anthropic-format routing

When a request hits `/v1/messages` (Anthropic format):

1. If an `anthropic`-style provider serves the model → stream/pass through directly (no conversion).
2. Otherwise → convert to OpenAI format, forward to an `openai_v1`/`openai_direct` provider, convert the response back to Anthropic format.

Streaming Anthropic requests to OpenAI-style providers are converted chunk-by-chunk via `OpenAIToAnthropicStreamConverter`.

## Failover

If a model is served by multiple providers, the router tries them in order:

- Up to `MAX_FAILOVER_ATTEMPTS = 3` providers.
- A provider is retried only if it returns a `5xx` status or a connection error.
- `4xx` responses are returned to the client immediately (no failover).

Providers are tried in the order returned by `ProviderRegistry::get_providers_for_model`, which is: the mapped provider first, then any other enabled provider whose `models` list contains the model.

## Auto-discovered providers

Aperture-discovered providers are merged into the registry at startup and on each refresh. Discovered models are added to existing providers (merge, not replace). Manually configured models that are not in discovery remain routable. Stale providers (no longer in discovery) are removed, but manually configured providers are retained.

Trigger a manual refresh:

```bash
curl -X POST http://127.0.0.1:8765/admin/refresh-models \
  -H "Authorization: Bearer YOUR_ADMIN_KEY"
```

## Enable/disable a provider at runtime

```bash
aperture-router config disable zai-coding
aperture-router config enable zai-coding
```

This edits `config.toml` in place. Restart the service to apply.

## Model aliases

Map a friendly name to an actual model:

```toml
[model_aliases]
fast = "glm-4.7-flash"
smart = "glm-5"
```

Requests for `fast` are rewritten to `glm-4.7-flash` before routing.

## Validation rules

| Rule | Error |
|------|-------|
| Duplicate provider `name` | `Duplicate provider name: X` |
| Empty `base_url` | `Provider X has empty base_url` |
| Empty `models` | `Provider X has no models configured` |
| Non-http(s) scheme | `Provider X has invalid base_url scheme` |
| Internal/metadata IP host | `Provider X has blocked base_url (internal/metadata IP)` |
| API key + HTTP (non-Tailscale) | Warning logged; for Aperture gateway, startup fails |

## SSRF note

Provider `base_url` hosts are validated against internal-IP and metadata-endpoint blocklists. The CGN range `100.64.0.0/10` is **allowed** for providers (Tailscale compatibility) but **blocked** for the default Aperture gateway endpoint validation. Hostnames are resolved and checked at request time (DNS rebinding protection).
