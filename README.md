# Aperture Router

> Universal AI router for Tailscale Aperture

Aperture Router is a lightweight, high-performance Rust proxy that enables **any AI tool** to work with models available on your Tailscale Aperture gateway.

## Features

- ✅ **Universal Compatibility** - Works with Claude Code, OpenCode, Pi, and any OpenAI/Anthropic-compatible tool
- ✅ **Dynamic Model Discovery** - Auto-discovers models from Aperture at runtime, no hardcoded providers
- ✅ **Auto-Refresh** - Model list refreshes automatically with configurable interval
- ✅ **Dual API Support** - OpenAI (`/v1/chat/completions`) and Anthropic (`/v1/messages`) formats
- ✅ **Interactive CLI** - Configuration wizard for easy setup
- ✅ **Fast & Lightweight** - Written in Rust for maximum performance
- ✅ **Secure** - Zeroizing API keys, timing-safe comparison, SSRF protection

## Quick Start

### Installation

#### From Source

```bash
git clone https://github.com/Wayazi/aperture-router
cd aperture-router
cargo build --release
sudo cp target/release/aperture-router /usr/local/bin/
```

#### Cargo

```bash
cargo install aperture-router
```

#### AUR (Arch Linux)

```bash
yay -S aperture-router
# or
paru -S aperture-router
```

### Configuration

Run the interactive wizard:

```bash
aperture-router config wizard --url http://your-aperture-gateway
```

Or create a `config.toml` file manually:

```toml
[server]
host = "127.0.0.1"
port = 8765

[aperture]
base_url = "http://100.100.100.100"  # Your Aperture gateway
model_refresh_interval_secs = 300
```

Or use environment variables:

```bash
export APERTURE_BASE_URL=http://100.100.100.100
export APERTURE_ROUTER_HOST=127.0.0.1
export APERTURE_ROUTER_PORT=8765
```

### Usage

Start the server:

```bash
aperture-router
```

Or with debug mode:

```bash
aperture-router --debug
```

## CLI Commands

```bash
# Start the router
aperture-router run

# Interactive configuration wizard
aperture-router config wizard

# Fetch and display models from Aperture
aperture-router config fetch --url http://your-gateway

# List current configuration
aperture-router config list

# Export config for OpenCode
aperture-router config export --opencode

# Validate configuration
aperture-router config validate
```

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/v1/models` | GET | List available models (from Aperture) |
| `/v1/chat/completions` | POST | OpenAI-compatible endpoint |
| `/v1/messages` | POST | Anthropic-compatible endpoint |
| `/v1/proxy` | POST | Generic streaming proxy endpoint |
| `/admin/stats` | GET | Server statistics (requires admin key) |
| `/admin/refresh-models` | POST | Force model refresh (requires admin key) |

## Advanced Features

- ✅ **Dynamic Discovery** - Models and providers discovered at runtime, no hardcoded lists
- ✅ **Auto-Refresh** - Background task keeps model list current
- ✅ **SSE Streaming** - Full Server-Sent Events support for streaming responses
- ✅ **Tool/Function Calling** - Supports OpenAI tool_calls and Anthropic tool_use
- ✅ **Extended Thinking** - Filters or includes Claude's thinking blocks
- ✅ **Graceful Shutdown** - Clean termination with CancellationToken

## Security Features

- ✅ **Zeroizing API Keys** - Keys securely wiped from memory
- ✅ **Timing-Safe Auth** - Constant-time comparison prevents timing attacks
- ✅ **SSRF Protection** - Blocks access to internal endpoints and metadata APIs
- ✅ **Secure File Permissions** - Config files created with 0o600
- ✅ **Rate Limiting** - Built-in authentication rate limiting
- ✅ **Security Headers** - CSP, X-Frame-Options, HSTS, X-Content-Type-Options

## Documentation

- [INSTALL.md](INSTALL.md) - Detailed installation guide
- [DEPLOYMENT.md](DEPLOYMENT.md) - Production deployment guide
- [CHANGELOG.md](CHANGELOG.md) - Version history and changes

## Development

```bash
# Build
cargo build

# Run tests (151 tests)
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
