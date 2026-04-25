# Aperture Router

> Universal AI router for Tailscale Aperture

Aperture Router is a lightweight, high-performance Rust proxy that enables **any AI tool** to work with models available on your Tailscale Aperture gateway.

## Features

- ✅ **Universal Compatibility** - Works with Claude Code, OpenCode, Pi, and any OpenAI/Anthropic-compatible tool
- ✅ **Dynamic Model Discovery** - Auto-discovers models from Aperture at runtime, no hardcoded providers
- ✅ **Auto-Refresh** - Model list refreshes automatically with configurable interval
- ✅ **Dual API Support** - OpenAI (`/v1/chat/completions`) and Anthropic (`/v1/messages`) formats
- ✅ **One-Command Setup** - Environment variables or wizard, no manual config editing needed
- ✅ **Fast & Lightweight** - Written in Rust for maximum performance
- ✅ **Secure** - Zeroizing API keys, timing-safe comparison, SSRF protection

## Installation

### Option 1: AUR (Arch Linux) - Recommended

```bash
yay -S aperture-router
# or
paru -S aperture-router
```

### Option 2: Cargo

```bash
cargo install aperture-router
```

### Option 3: From Source

```bash
git clone https://github.com/Wayazi/aperture-router
cd aperture-router
cargo build --release
sudo cp target/release/aperture-router /usr/local/bin/
```

## Quick Start

### Method 1: Environment Variables (Fastest)

No config file needed! Just set your Aperture URL:

```bash
export APERTURE_BASE_URL=http://your-aperture-gateway:8080
aperture-router
```

With authentication:

```bash
export APERTURE_BASE_URL=http://your-aperture-gateway:8080
export APERTURE_API_KEY=your-api-key-here-at-least-32-characters
aperture-router
```

### Method 2: Generate Config

Quick config generation with auto-generated API key:

```bash
aperture-router config generate --url http://your-aperture-gateway:8080 --generate-key
```

Or with your own API key:

```bash
aperture-router config generate --url http://your-aperture-gateway:8080 \
  --output /etc/aperture-router/config.toml
```

### Method 3: Interactive Wizard

Full interactive setup with model selection:

```bash
aperture-router config wizard
```

The wizard will:
1. Connect to your Aperture gateway
2. Discover available models and providers
3. Let you select which providers/models to use
4. Optionally generate OpenCode config
5. Save everything to `config.toml`

### Method 4: Systemd Service (Arch Linux)

```bash
# Install
yay -S aperture-router

# Set your Aperture URL
echo "APERTURE_BASE_URL=http://your-aperture-gateway:8080" | sudo tee /etc/sysconfig/aperture-router

# Start service
sudo systemctl enable --now aperture-router

# Check status
sudo systemctl status aperture-router
```

## CLI Commands

```bash
# Start the router (default)
aperture-router

# Start with debug logging
aperture-router --debug

# Use specific config file
aperture-router --config /path/to/config.toml

# Configuration commands
aperture-router config wizard              # Interactive setup
aperture-router config generate --url URL  # Quick config generation
aperture-router config fetch --url URL     # List available models
aperture-router config list                # Show current config
aperture-router config validate            # Validate config

# Export for OpenCode
aperture-router config export --opencode
```

## Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `APERTURE_BASE_URL` | Aperture gateway URL (required if no config) | `http://100.100.100.100:8080` |
| `APERTURE_API_KEY` | API key for authentication | `your-32-char-key-here` |
| `APERTURE_ALLOW_NO_AUTH` | Disable auth requirement (dev only) | `1` |
| `RUST_LOG` | Logging level | `debug`, `info` |

## Config File

Default location: `config.toml` in current directory, or `/etc/aperture-router/config.toml` for systemd.

```toml
host = "127.0.0.1"
port = 8765

[aperture]
base_url = "http://your-aperture-gateway:8080"
model_refresh_interval_secs = 300

[security]
api_keys = ["your-api-key-at-least-32-characters"]
admin_api_keys = ["admin-key-at-least-32-characters"]
require_auth_in_prod = true

[cors]
allowed_origins = ["http://localhost:3000"]

[rate_limit]
requests_per_second = 10
burst_size = 30
```

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/v1/models` | GET | List available models |
| `/v1/chat/completions` | POST | OpenAI-compatible endpoint |
| `/v1/messages` | POST | Anthropic-compatible endpoint |
| `/v1/proxy` | POST | Generic streaming proxy |
| `/admin/stats` | GET | Server statistics (admin key) |
| `/admin/refresh-models` | POST | Force model refresh (admin key) |

### Session Tracking

All requests are logged with both a `request_id` and `session_id` for easy log grouping:

- **`X-Session-ID`** header: Send this to group multiple requests in one session
- If not provided, server generates a new session ID and returns it in response headers
- Use the returned session ID in subsequent requests to maintain session continuity

**Example:**

```bash
# First request - server generates session ID
curl -X POST http://127.0.0.1:8765/v1/chat/completions \
  -H "x-api-key: your-key" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "Hello"}]}'

# Response includes X-Session-ID header
# X-Session-ID: 550e8400-e29b-41d4-a716-446655440000

# Subsequent requests - reuse session ID for log grouping
curl -X POST http://127.0.0.1:8765/v1/chat/completions \
  -H "x-api-key: your-key" \
  -H "x-session-id: 550e8400-e29b-41d4-a716-446655440000" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "Follow-up"}]}'
```

Logs will show both IDs:
```
request_id=abc-123 session_id=550e8400... "Request started"
request_id=abc-123 session_id=550e8400... "Request completed"
request_id=def-456 session_id=550e8400... "Request started"
request_id=def-456 session_id=550e8400... "Request completed"
```

Filter logs by session_id to see all requests from a single user session.

## Security Features

- ✅ **Zeroizing API Keys** - Keys securely wiped from memory
- ✅ **Timing-Safe Auth** - Constant-time comparison prevents timing attacks
- ✅ **SSRF Protection** - Blocks access to metadata endpoints (169.254.169.254, etc.)
- ✅ **Secure File Permissions** - Config files created with `0o600` (owner read/write only)
- ✅ **Rate Limiting** - Built-in authentication rate limiting
- ✅ **Security Headers** - CSP, X-Frame-Options, HSTS

## Using with AI Tools

### Claude Code

```bash
# In Claude Code settings, set API base:
http://127.0.0.1:8765
```

### OpenCode

The wizard can auto-generate OpenCode config:

```bash
aperture-router config wizard
# Select "Yes" when asked about OpenCode export
```

Or manually:

```bash
aperture-router config export --opencode --output ~/.config/opencode/opencode.json
```

### Any OpenAI-Compatible Tool

Set the API base URL to `http://127.0.0.1:8765`

## Documentation

- [INSTALL.md](INSTALL.md) - Detailed installation guide
- [DEPLOYMENT.md](DEPLOYMENT.md) - Production deployment guide
- [CHANGELOG.md](CHANGELOG.md) - Version history

## Development

```bash
# Build
cargo build

# Run tests (154 tests)
cargo test

# Run with debug logging
cargo run -- --debug
```

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Acknowledgments

Inspired by:
- [LMRouter](https://github.com/LMRouter/lmrouter)
- [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)
- [claude-code-proxy](https://github.com/nielspeter/claude-code-proxy)
- [anthropic-proxy-rs](https://github.com/m0n0x41d/anthropic-proxy-rs)
