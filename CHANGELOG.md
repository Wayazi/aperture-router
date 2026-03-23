# Changelog

All notable changes to aperture-router will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial release preparation

## [0.1.0] - 2026-03-23

### Added
- Universal AI router for Tailscale Aperture
- OpenAI `/v1/chat/completions` API compatibility
- Anthropic `/v1/messages` API compatibility
- Model discovery and caching from Aperture gateway
- SSE streaming support for real-time responses
- Tool/function calling support (OpenAI tool_calls and Anthropic tool_use)
- Extended thinking blocks (with filtering option)
- Model validation against available models
- Authentication with API keys
- Rate limiting for authentication attempts
- SSRF protection (blocks internal IPs and metadata endpoints)
- Security headers (CSP, X-Frame-Options, X-XSS-Protection, HSTS)
- Request/response size limits
- JSON depth validation to prevent DoS
- Configuration via TOML file or environment variables
- Health check endpoint
- Comprehensive test suite (114 tests)

### Security
- Constant-time API key comparison using `subtle` crate
- API key strength validation (32 char min, 20 unique chars)
- IP-based rate limiting with automatic cleanup
- Trusted proxy IP validation
- HTTPS enforcement when API keys are configured
- CORS production validation

### Documentation
- README with quick start guide
- Example configuration file
- Systemd service files
- AUR package build files
- GitHub release workflow

## Installation

### Cargo
```bash
cargo install aperture-router
```

### AUR (Arch Linux)
```bash
yay -S aperture-router
```

### Binary
Download from [Releases](https://github.com/Wayazi/aperture-router/releases)

## Configuration

Create a `config.toml`:

```toml
[server]
host = "127.0.0.1"
port = 8080

[aperture]
base_url = "http://100.100.100.100"  # Your Aperture gateway

[logging]
level = "info"

[security]
api_keys = ["your-api-key-here"]  # Optional: Enable authentication
```

Or use environment variables:

```bash
export APERTURE_BASE_URL=http://100.100.100.100
export APERTURE_ROUTER_PORT=8080
export APERTURE_API_KEY=your-api-key-here
```

## Usage

```bash
aperture-router
```

With custom config:

```bash
aperture-router --config /path/to/config.toml
```

With debug logging:

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

## License

MIT License - see [LICENSE](LICENSE) file for details.

[Unreleased]: https://github.com/Wayazi/aperture-router/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Wayazi/aperture-router/releases/tag/v0.1.0
