# Tutorial 2: Configuring Multiple Providers

> **Difficulty:** Beginner | **Time needed:** ~10 min | **OS:** Linux

## What you'll learn

By the end of this tutorial you will have:

- Used the interactive config wizard to discover models from Aperture
- Selected which providers and models to enable
- Understood the three endpoint styles (`openai_v1`, `openai_direct`, `anthropic`)
- Manually configured a custom provider in `config.toml`
- Verified multi-provider routing and failover

## Prerequisites

- [ ] Completed [Tutorial 1: Getting Started](01-getting-started.md)
- [ ] aperture-router installed and `--version` works
- [ ] A reachable Tailscale Aperture gateway
- [ ] (Optional) Credentials for a second provider, e.g. Z.ai, OpenRouter, or any OpenAI/Anthropic-compatible API

> **Why multiple providers?** Aperture already gives you access to several
> models, but sometimes you also have direct API keys for other providers (Z.ai,
> OpenRouter, a local LLM server). aperture-router can fan out across all of them
> from a single endpoint, with automatic failover when a provider is down.

---

## Step 1: Discover what Aperture offers

Before configuring, let's see what models your Aperture gateway exposes. The
`config fetch` command contacts Aperture and lists everything it finds:

```bash
aperture-router config fetch --url http://your-aperture-gateway:8080
```

> **What this does:** Connects to Aperture, calls its model-list endpoint, and
> prints a grouped table of providers and their models. This is read-only — it
> changes nothing.

Expected output (abbreviated):

```
╔══════════════════════════════════════════════════════════════╗
║               AVAILABLE MODELS FROM APERTURE                 ║
╚══════════════════════════════════════════════════════════════╝

┌─ Provider: openrouter ──────────────────────────────────────
│  openrouter/auto → openrouter/openrouter/auto
│
┌─ Provider: glm ─────────────────────────────────────────────
│  glm-4.7 → glm/glm-4.7
│  glm-4.7-flash → glm/glm-4.7-flash
│
Total: 8 models from 3 providers
```

Note down the provider names and model ids you care about — you'll select them
in the wizard next.

✅ **Checkpoint:** You see at least one provider with models listed.

---

## Step 2: Run the interactive wizard

The wizard walks you through building a `config.toml` without editing files by
hand:

```bash
aperture-router config wizard
```

> **What this does:** Launches an interactive prompt that (1) asks for your
> Aperture URL, (2) fetches models live, (3) lets you pick providers and models,
> (4) optionally collects an API key, (5) saves `config.toml`, and (6) can also
> generate an OpenCode config.

You'll see something like:

```
╔══════════════════════════════════════════════════════════════╗
║         APERTURE ROUTER CONFIGURATION WIZARD                 ║
╚══════════════════════════════════════════════════════════════╝

? Aperture gateway URL: › http://100.100.100.100

📡 Fetching available models from Aperture...
✓ Found 8 models

? Select providers to enable (models will be fetched dynamically at runtime):
 ❯ openrouter
   glm
   glm2
```

Use the arrow keys and spacebar to toggle providers, then press Enter.

✅ **Checkpoint:** You selected at least one provider.

---

## Step 3: Select models per provider

For each provider you enabled, the wizard shows its models and asks which to
include:

```
? Select models for provider 'glm':
 ❯ glm-4.7 → glm/glm-4.7
   glm-4.7-flash → glm/glm-4.7-flash
```

Again, space to toggle, Enter to confirm. All are selected by default.

> **What this does:** Records which model ids the router should route. Models
> you don't select won't show up in `/v1/models` and will be rejected if
> requested. You can always re-run the wizard or edit `config.toml` later.

✅ **Checkpoint:** You've chosen models for each selected provider.

---

## Step 4: API key and router settings

The wizard then asks:

```
? Configure API key for Aperture? (y/N)
? Router port: › 8765
```

- **API key:** Say yes only if your Aperture gateway requires auth. The input is
  hidden as you type.
- **Port:** `8765` is the default and avoids common conflicts. Change it if
  something else is using that port.

Finally you'll see a summary and a confirmation prompt:

```
│ CONFIGURATION SUMMARY
│ Router: http://127.0.0.1:8765
│ Aperture: http://100.100.100.100
│ Providers (auto-discovered from Aperture):
│   openrouter (1 models)
│     - openrouter/auto
│   glm (2 models)
│     - glm-4.7
│     - glm-4.7-flash

? Save this configuration? (Y/n)
? Export OpenCode configuration as well? (Y/n)
```

Answer `Y` to both. The wizard writes `config.toml` and (optionally)
`~/.config/opencode/opencode.json`.

✅ **Checkpoint:** You see `✅ Configuration ready to save!` and a `config.toml`
file exists in your current directory.

---

## Step 5: Inspect the generated config

```bash
aperture-router config list
```

> **What this does:** Loads `config.toml` (or the system path) and prints a
 readable summary of server settings and each provider's status, URL, endpoint
 style, models, and whether an API key is set.

You can also view the raw file:

```bash
cat config.toml
```

A wizard-generated config looks roughly like this:

```toml
host = "127.0.0.1"
port = 8765

[aperture]
base_url = "http://100.100.100.100"
model_refresh_interval_secs = 300

[[providers]]
name = "glm"
base_url = "http://100.100.100.100"
endpoint_style = "anthropic"
models = ["glm-4.7", "glm-4.7-flash"]
enabled = true
```

✅ **Checkpoint:** `config list` shows your providers as `enabled`.

---

## Step 6: Understand the three endpoint styles

This is the key concept for multi-provider setup. Each provider has an
`endpoint_style` that tells the router how to build URLs and what request format
to send. There are exactly three:

| Style | URL pattern | Use when |
|-------|-------------|----------|
| `openai_v1` | `base_url` + `/v1/chat/completions` | Standard OpenAI-compatible APIs (Aperture, OpenRouter) |
| `openai_direct` | `base_url` + `/chat/completions` (no `/v1`) | Z.ai v4 and similar APIs that skip the `/v1` prefix |
| `anthropic` | `base_url` + `/v1/messages` | Native Anthropic-format endpoints (Claude API, Z.ai Anthropic mirror) |

> **What this does:** The router's `ProviderRegistry::build_endpoint_url`
> function reads this field to construct the correct upstream URL per provider
> (see `src/provider/registry.rs`). It also decides whether to send OpenAI or
> Anthropic-shaped request bodies.

**Example mappings:**

- Aperture gateway → `openai_v1` (it speaks the standard OpenAI `/v1/` API)
- `https://api.z.ai/api/paas/v4` → `openai_direct` (Z.ai v4 has no `/v1`)
- `https://api.z.ai/api/anthropic` → `anthropic` (Anthropic messages format)

✅ **Checkpoint:** You can name the three styles and when to use each.

---

## Step 7: Add a custom provider manually

The wizard only knows about Aperture-discovered providers. To add an external
provider (e.g. a direct Z.ai key), edit `config.toml` and append a `[[providers]]`
block. Open the file and add:

```toml
[[providers]]
name = "zai-credit"
base_url = "https://api.z.ai/api/paas/v4"
endpoint_style = "openai_direct"
models = ["glm-5", "glm-4.7"]
api_key = "your-zai-api-key"
enabled = true
```

> **What this does:** Registers a second provider. Now when a request asks for
> `glm-4.7`, the router checks both the Aperture-backed `glm` provider and this
> `zai-credit` provider. If the first fails, it tries the next — that's
> automatic failover (up to 3 attempts, see `MAX_FAILOVER_ATTEMPTS` in
> `src/routes/messages.rs`).

You can reference an environment variable for the key instead of hardcoding:

```toml
api_key = "your-zai-api-key"
```

Then `export ZAI_API_KEY=...` before starting the router.

✅ **Checkpoint:** The new `[[providers]]` block is in your `config.toml`.

---

## Step 8: Validate and start

Validate the config before running:

```bash
aperture-router config validate
```

> **What this does:** Parses `config.toml`, checks required fields, verifies
> endpoint styles are valid, and confirms API keys meet the minimum length. It
> prints `✅ Configuration is valid` or a descriptive error.

Expected output:

```
✅ Configuration is valid

Server: 127.0.0.1:8765
Aperture: http://100.100.100.100
Providers: 2 (glm, zai-credit)
```

Now start the server with your config:

```bash
aperture-router
```

If your `config.toml` is not in the current directory, point to it explicitly:

```bash
aperture-router --config /path/to/config.toml
```

✅ **Checkpoint:** The server starts and logs show both providers registered.

---

## Step 9: Verify multi-provider routing

List models — you should now see models from **both** providers:

```bash
curl http://127.0.0.1:8765/v1/models
```

Send a request to a model only the custom provider has:

```bash
curl -X POST http://127.0.0.1:8765/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "glm-5", "messages": [{"role": "user", "content": "Hi"}]}'
```

> **What this does:** The router's `get_providers_for_model` looks up all
> providers that list `glm-5` and tries them in order. Watch the server logs —
> you'll see a line like `Routing request for model 'glm-5' to provider
> 'zai-credit'`.

To simulate failover, temporarily set a bad `base_url` for one provider and
confirm the router falls back to the other.

✅ **Checkpoint:** A request for a custom-provider model returns a valid
response.

---

## Step 10: Enable and disable providers on the fly

You don't need to edit the file to toggle providers. Use the CLI:

```bash
# Disable a provider
aperture-router config disable zai-credit

# Re-enable it
aperture-router config enable zai-credit
```

> **What this does:** Flips the `enabled` field in `config.toml` and saves it.
> Disabled providers are skipped entirely — their models won't appear in
> `/v1/models` and won't be tried during failover. Restart the server (or rely on
> auto-refresh for discovered models) for the change to take full effect.

✅ **Checkpoint:** After disabling, `/v1/models` no longer lists that provider's
models.

---

## You're done!

You now have a multi-provider aperture-router with failover.

### Quick recap

| Step | What we did |
|------|-------------|
| 1 | Discovered models with `config fetch` |
| 2 | Ran the wizard to pick providers |
| 3 | Selected models per provider |
| 4 | Set API key and port |
| 5 | Inspected the generated `config.toml` |
| 6 | Learned the three endpoint styles |
| 7 | Added a custom external provider |
| 8 | Validated and started the server |
| 9 | Verified multi-provider routing |
| 10 | Toggled providers with the CLI |

### Endpoint style cheat sheet

```
openai_v1:     base_url + /v1/chat/completions
openai_direct: base_url + /chat/completions      (no /v1)
anthropic:     base_url + /v1/messages
```

### Next steps

- **Tutorial 3** — Connect OpenCode to aperture-router.
- **Tutorial 4** — Deep dive into Anthropic↔OpenAI format conversion.
- See `config.example.toml` for a fully commented reference of every option,
  including security, rate limiting, CORS, and model aliases.
