# Tutorial 1: Getting Started with Aperture Router

> **Difficulty:** Beginner | **Time needed:** ~5 min | **OS:** Linux

## What you'll learn

By the end of this tutorial you will have:

- Installed aperture-router on your machine
- Pointed it at a Tailscale Aperture gateway
- Started the server
- Made your first AI request through the router

## Prerequisites

- [ ] A Linux machine (x86_64 or aarch64)
- [ ] A running Tailscale Aperture gateway you can reach over the network
- [ ] `curl` installed (for testing)
- [ ] `cargo` (Rust toolchain) **or** an AUR helper like `yay`/`paru`

> **What is aperture-router?** It's a small Rust proxy that sits between your AI
> tools (Claude Code, OpenCode, anything OpenAI/Anthropic-compatible) and a
> Tailscale Aperture gateway. Your tools talk to the router; the router forwards
> to Aperture and handles format conversion, model discovery, and failover.

---

## Step 1: Install aperture-router

Pick **one** of the options below.

### Option A — AUR (Arch Linux)

```bash
yay -S aperture-router
```

> **What this does:** Installs the binary to `/usr/bin/aperture-router`, plus
> systemd service files and a system user. Recommended if you're on Arch.

### Option B — Cargo (any Linux)

```bash
cargo install aperture-router
```

> **What this does:** Downloads, compiles, and installs the binary to
> `~/.cargo/bin/aperture-router`. Make sure `~/.cargo/bin` is on your `PATH`.

### Option C — From source

```bash
git clone https://github.com/Wayazi/aperture-router
cd aperture-router
cargo build --release
sudo cp target/release/aperture-router /usr/local/bin/
```

Verify the install:

```bash
aperture-router --version
```

Expected output (your version may be newer):

```
aperture-router 0.3.1
```

✅ **Checkpoint:** `aperture-router --version` prints a version number.

---

## Step 2: Set your Aperture gateway URL

aperture-router needs to know where your Aperture gateway lives. The fastest way
is a single environment variable — no config file required.

```bash
export APERTURE_BASE_URL=http://your-aperture-gateway:8080
```

Replace `your-aperture-gateway:8080` with the IP or hostname of your Aperture
gateway (for example `http://100.100.100.100` or
`http://ai.your-tailnet.ts.net`).

> **What this does:** Tells the router where to discover models and forward
> requests. If your Aperture requires an API key, also set it now:
> `export APERTURE_API_KEY=your-32-character-key-here`.

✅ **Checkpoint:** Run `echo $APERTURE_BASE_URL` and confirm it prints your URL.

---

## Step 3: Start the router

```bash
aperture-router
```

You should see log lines like:

```
INFO Starting Aperture Router v0.3.1
INFO Listening on 127.0.0.1:8765
INFO Discovered 12 models from 3 providers
```

> **What this does:** Starts an HTTP server on `127.0.0.1:8765`, reaches out to
> your Aperture gateway, and auto-discovers the available models. The model list
> refreshes automatically every 5 minutes by default — no restart needed when
> Aperture adds models.

Leave this terminal running. Open a **new terminal** for the next step.

✅ **Checkpoint:** The server logs show `Listening on 127.0.0.1:8765` with no
errors.

---

## Step 4: Check the health endpoint

In your new terminal:

```bash
curl http://127.0.0.1:8765/health
```

Expected output:

```json
{"status":"ok","service":"aperture-router","version":"0.3.1"}
```

> **What this does:** Confirms the router is up and responding. The `/health`
> endpoint is intentionally lightweight so you can use it for uptime checks.

✅ **Checkpoint:** `/health` returns `{"status":"ok","service":"aperture-router","version":"0.3.1"}`.

---

## Step 5: List available models

```bash
curl http://127.0.0.1:8765/v1/models
```

You'll get a JSON list of every model aperture-router discovered from Aperture:

```json
{
  "object": "list",
  "data": [
    { "id": "gpt-4", "object": "model", "owned_by": "aperture" },
    { "id": "claude-3-opus", "object": "model", "owned_by": "aperture" }
  ]
}
```

> **What this does:** Calls Aperture's model list and normalizes it into the
> OpenAI `/v1/models` shape that most tools expect. If you set an API key in
> Step 2, you may need to send it as a header (see below).

If your router requires auth, add the header:

```bash
curl -H "Authorization: Bearer your-api-key" http://127.0.0.1:8765/v1/models
```

✅ **Checkpoint:** You see at least one model in the list.

---

## Step 6: Make your first request

Send a real chat completion through the OpenAI-compatible endpoint:

```bash
curl -X POST http://127.0.0.1:8765/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-api-key" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Say hello in one word."}]
  }'
```

> **What this does:** Your tool (here, `curl`) speaks OpenAI format. The router
> looks up which provider serves `gpt-4`, forwards the request to Aperture, and
> returns the OpenAI-shaped response. If you don't have `gpt-4`, substitute any
> model id from Step 5.

Expected response (abbreviated):

```json
{
  "id": "chatcmpl-...",
  "object": "chat.completion",
  "choices": [
    { "message": { "role": "assistant", "content": "Hello" } }
  ]
}
```

✅ **Checkpoint:** You got a valid response back from a model.

---

## You're done!

You now have a working aperture-router. From here, any OpenAI- or
Anthropic-compatible tool can point at `http://127.0.0.1:8765` and use your
Aperture models.

### Quick recap

| Step | What we did |
|------|-------------|
| 1 | Installed the binary |
| 2 | Set `APERTURE_BASE_URL` |
| 3 | Started the server |
| 4 | Verified with `/health` |
| 5 | Listed models |
| 6 | Made a chat completion request |

### Next steps

- **Tutorial 2** — Configure multiple providers with the interactive wizard.
- **Tutorial 3** — Connect OpenCode to aperture-router.
- **Tutorial 4** — Learn how Anthropic↔OpenAI format conversion works.

### Troubleshooting

**"No config file found and APERTURE_BASE_URL not set"**
You forgot Step 2. Run `export APERTURE_BASE_URL=...` again.

**"Production mode requires authentication but no API keys configured"**
Either set `APERTURE_API_KEY` or, for local dev only, disable auth with
`export APERTURE_ALLOW_NO_AUTH=1`.

**Connection refused to Aperture**
Make sure Tailscale is running (`tailscale status`) and that you can reach the
gateway directly: `curl http://your-aperture-gateway:8080/v1/models`.

**Port 8765 already in use**
Run on a different port by creating a config file (Tutorial 2) or start with a
custom config: see `INSTALL.md`.
