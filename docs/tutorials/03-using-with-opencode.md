# Tutorial 3: Using aperture-router with OpenCode

> **Difficulty:** Beginner | **Time needed:** ~5 min | **OS:** Linux

## What you'll learn

By the end of this tutorial you will have:

- Exported an OpenCode-compatible config from aperture-router
- Pointed OpenCode at the router
- Run an OpenCode session backed by your Aperture models

## Prerequisites

- [ ] Completed [Tutorial 1: Getting Started](01-getting-started.md) (router installed and working)
- [ ] Completed [Tutorial 2: Configuring Providers](02-configuring-providers.md) **or** have a working `config.toml`
- [ ] aperture-router running and `/health` returns `{"status":"ok","service":"aperture-router","version":"0.3.1"}`
- [ ] [OpenCode](https://opencode.ai) installed (`opencode --version` works)

> **What is OpenCode?** OpenCode is an open-source AI coding assistant (similar
> to Claude Code). It reads a config file at
> `~/.config/opencode/opencode.json` that defines which providers and models it
> can use. aperture-router can generate that file for you automatically.

---

## Step 1: Make sure the router is running

In one terminal:

```bash
aperture-router
```

Confirm it's up in another terminal:

```bash
curl http://127.0.0.1:8765/health
```

Expected:

```json
{"status":"ok","service":"aperture-router","version":"0.3.1"}
```

> **What this does:** OpenCode will send all its requests to the router, so the
> router must be running before you launch OpenCode. Keep this terminal open.

✅ **Checkpoint:** `/health` returns ok.

---

## Step 2: Export the OpenCode config

aperture-router can read your `config.toml`, fetch the current model list from
Aperture, and emit an `opencode.json` with everything wired up:

```bash
aperture-router config export --opencode
```

> **What this does:** Connects to Aperture to get model metadata, then writes
> `opencode.json` in the current directory. The exported config defines a single
> provider named `router` whose `baseURL` is `http://127.0.0.1:8765/v1`, with
> every discovered model listed. The `apiKey` is set to `"-"` because the router
> itself handles upstream auth — OpenCode doesn't need a real key.

To write it straight to OpenCode's config location:

```bash
aperture-router config export --opencode --output ~/.config/opencode/opencode.json
```

> **What this does:** Same export, but targets the default path OpenCode reads
> on startup. If an `opencode.json` already exists, the router **merges** its
> provider/model settings into it and preserves your other settings (MCP
> servers, theme, autoupdate, etc.). File permissions are set to `0600`
> (owner-only) for safety.

If your router listens on a non-default URL, pass it:

```bash
aperture-router config export --opencode \
  --router-url http://127.0.0.1:9000 \
  --output ~/.config/opencode/opencode.json
```

✅ **Checkpoint:** `~/.config/opencode/opencode.json` exists and contains a
`"router"` provider.

---

## Step 3: Inspect the exported config

```bash
cat ~/.config/opencode/opencode.json
```

It looks like this (abbreviated):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "model": "router/glm-4.7",
  "small_model": "router/glm-4.7-flash",
  "provider": {
    "router": {
      "name": "Aperture Router",
      "npm": "@ai-sdk/anthropic",
      "models": {
        "glm-4.7": { "name": "glm-4.7 [glm]" },
        "glm-4.7-flash": { "name": "glm-4.7-flash [glm]" }
      },
      "options": {
        "apiKey": "-",
        "baseURL": "http://127.0.0.1:8765/v1"
      }
    }
  }
}
```

A few things to notice:

- **`model` / `small_model`** — The router picks sensible defaults: the first
  non-`flash`/non-`haiku` model becomes `model`, and the first
  `flash`/`haiku` model becomes `small_model`. You can change these by hand.
- **`apiKey: "-"`** — A placeholder. The router validates this itself; OpenCode
  just needs *a* non-empty string.
- **Model ids are prefixed with `router/`** — This tells OpenCode which provider
  to route to. Under the hood OpenCode sends `model: "glm-4.7"` (without the
  prefix) to the router's `/v1/messages` endpoint.

> **What this does:** OpenCode loads this file on startup and presents all the
> listed models in its model picker. Selecting one sends requests to
> `http://127.0.0.1:8765/v1`.

✅ **Checkpoint:** The JSON has a `provider.router` block and your models are
listed.

---

## Step 4: Start OpenCode

From any project directory:

```bash
opencode
```

> **What this does:** Launches the OpenCode TUI. It reads
> `~/.config/opencode/opencode.json`, registers the `router` provider, and makes
> your Aperture models available.

If you want to override the default model for this session, OpenCode's model
picker (usually `/` or the model menu) will list entries like
`router/glm-4.7`. Pick one.

✅ **Checkpoint:** OpenCode starts without config errors and shows your models.

---

## Step 5: Send a message

Type a prompt in OpenCode, for example:

```
Write a one-line haiku about Rust.
```

Press Enter. You should see the model stream a response.

> **What this does:** OpenCode formats the request in Anthropic `/v1/messages`
> shape and POSTs it to `http://127.0.0.1:8765/v1/messages`. The router receives
> it, finds the right provider for the model, and streams the response back. If
> the upstream model is OpenAI-format only, the router transparently converts
> (see Tutorial 4).

Watch the router terminal — you'll see log lines like:

```
INFO Routing Anthropic request for model 'glm-4.7' to provider 'glm'
```

✅ **Checkpoint:** You got a streamed response inside OpenCode.

---

## Alternative: generate config during the wizard

If you haven't configured yet, the wizard can do it all in one pass:

```bash
aperture-router config wizard
```

At the end it asks:

```
? Export OpenCode configuration as well? (Y/n)
```

Say `Y`. The wizard writes both `config.toml` **and** merges the OpenCode config
into `~/.config/opencode/opencode.json`, preserving any existing settings.

✅ **Checkpoint:** After the wizard finishes, `opencode.json` exists.

---

## Manual OpenCode config (no export)

If you prefer to write the file yourself or want to add the router as an
**additional** provider alongside existing ones, add this block to your
`opencode.json` under `provider`:

```json
"router": {
  "name": "Aperture Router",
  "npm": "@ai-sdk/anthropic",
  "models": {
    "glm-4.7": { "name": "GLM 4.7 via Aperture" }
  },
  "options": {
    "apiKey": "-",
    "baseURL": "http://127.0.0.1:8765/v1"
  }
}
```

Then set the top-level `"model"` to `"router/glm-4.7"`.

> **What this does:** Manually registers the router provider. This is useful if
> you already have an `opencode.json` with other providers (e.g. a direct
> Anthropic key) and just want to add Aperture as an option.

✅ **Checkpoint:** OpenCode starts and lists the `router/...` models.

---

## You're done!

OpenCode is now backed by your Aperture models through aperture-router.

### Quick recap

| Step | What we did |
|------|-------------|
| 1 | Confirmed the router is running |
| 2 | Exported `opencode.json` with `config export --opencode` |
| 3 | Inspected the generated config |
| 4 | Launched OpenCode |
| 5 | Sent a streamed message |
| (alt) | Generated config via the wizard |
| (manual) | Hand-wrote a `router` provider block |

### Tips

- **Re-export after adding models:** If you add providers to `config.toml`,
  re-run `aperture-router config export --opencode` to refresh the model list in
  OpenCode. Existing non-provider settings are preserved.
- **Auth:** If your router has API keys configured (not `APERTURE_ALLOW_NO_AUTH`),
  set the real key in the `options.apiKey` field instead of `"-"`.
- **Multiple machines:** The `baseURL` in the export defaults to
  `http://127.0.0.1:8765/v1`. If OpenCode runs on a different host, pass
  `--router-url http://<router-host>:8765` during export.

### Next steps

- **Tutorial 4** — Learn how the Anthropic↔OpenAI conversion works under the
  hood, so you can debug streaming and format issues.
