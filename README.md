# Aperture Router

> Universal AI router for Tailscale Aperture

Aperture Router is a lightweight, high-performance Rust proxy that enables **any AI tool** to work with models available on your Tailscale Aperture gateway.

## Features

- ✅ **Universal Compatibility** - Works with Claude Code, OpenCode, Pi, and any OpenAI/Anthropic-compatible tool
- ✅ **Model Discovery** - Automatically discovers all available models from your Aperture gateway
- ✅ **Dual API Support** - OpenAI (`/v1/chat/completions`) and Anthropic (`/v1/messages`) formats
- ✅ **Zero Configuration** - Works out of the box with sensible defaults
- ✅ **Fast & Lightweight** - Written in Rust for maximum performance
- ✅ **Secure** - Uses Tailscale identity, no API keys needed on client

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

Create a `config.toml` file:

```toml
[server]
host = "127.0.0.1"
port = 8080

[aperture]
base_url = "http://100.100.100.100"  # Your Aperture gateway
model_refresh_interval_secs = 300

[logging]
level = "info"
```

Or use environment variables:

```bash
export APERTURE_BASE_URL=http://100.100.100.100
export APERTURE_ROUTER_HOST=127.0.0.1
export APERTURE_ROUTER_PORT=8080
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

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/v1/models` | GET | List available models |
| `/v1/chat/completions` | POST | OpenAI-compatible endpoint |
| `/v1/messages` | POST | Anthropic-compatible endpoint |
| `/v1/proxy` | POST | Generic streaming proxy endpoint |

## Advanced Features

- ✅ **SSE Streaming** - Full Server-Sent Events support for streaming responses
- ✅ **Tool/Function Calling** - Supports OpenAI tool_calls and Anthropic tool_use
- ✅ **Extended Thinking** - Filters or includes Claude's thinking blocks
- ✅ **Model Validation** - Validates requested models against available ones
- ✅ **Rate Limiting** - Built-in authentication rate limiting
- ✅ **SSRF Protection** - Blocks access to internal endpoints

## Documentation

- [INSTALL.md](INSTALL.md) - Detailed installation guide
- [DEPLOYMENT.md](DEPLOYMENT.md) - Production deployment guide
- [CHANGELOG.md](CHANGELOG.md) - Version history and changes

## Development

```bash
# Build
cargo build

# Run tests
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
