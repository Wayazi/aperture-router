# CLI commands reference

> All `aperture-router` CLI commands and flags.

Source: `src/main.rs`, `src/cli/`.

## Synopsis

```
aperture-router [OPTIONS] [COMMAND]
```

If no `COMMAND` is given, `run` is assumed.

## Global options

| Flag | Description |
|------|-------------|
| `-c, --config <PATH>` | Path to config file |
| `--system` | Use `/etc/aperture-router/config.toml` |
| `-d, --debug` | Enable debug logging (`aperture_router=debug,tower_http=debug,axum=debug`) |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

### Config path resolution

1. `--config PATH` (explicit)
2. `--system` flag **or** running as root/sudo → `/etc/aperture-router/config.toml`
3. Otherwise → `./config.toml`

Running elevated is detected via `SUDO_USER` env var or `USER=root`.

## Commands

### `run` (default)

Starts the HTTP server.

```bash
aperture-router
aperture-router --debug
aperture-router --config /path/to/config.toml
```

Loads `.env` if present, initializes tracing, loads config (or builds from env), discovers models, starts background tasks, and serves on `host:port`.

Exit codes:

- `0` — graceful shutdown (SIGINT/SIGTERM)
- non-zero — config error, bind failure, or fatal runtime error

### `config wizard`

Interactive configuration. Requires build with `--features wizard`.

```bash
aperture-router config wizard
aperture-router config wizard --url http://100.100.100.100
aperture-router config wizard --output /etc/aperture-router/config.toml
```

| Flag | Description |
|------|-------------|
| `-u, --url <URL>` | Aperture gateway URL (skip prompt) |
| `-o, --output <PATH>` | Output config path |

Steps: connect to Aperture → discover models/providers → select → optionally generate OpenCode config → save `config.toml`.

### `config generate`

Non-interactive config creation from args/env.

```bash
aperture-router config generate --url http://100.100.100.100 --generate-key
aperture-router config generate --url http://100.100.100.100 -o /etc/aperture-router/config.toml
```

| Flag | Description |
|------|-------------|
| `-u, --url <URL>` | Aperture gateway URL (required if `APERTURE_BASE_URL` unset) |
| `-o, --output <PATH>` | Output path (default: config path) |
| `--generate-key` | Generate a random API key and print it |

Reads `APERTURE_BASE_URL`, `APERTURE_API_KEY` (gateway key), `APERTURE_CLIENT_API_KEYS`, and `APERTURE_ALLOW_NO_AUTH` from the environment. Generated keys are `apr_`-prefixed base62 of two UUIDv4 values.

> `--generate-key` takes precedence: if it produces a key, `APERTURE_CLIENT_API_KEYS` is ignored (and still scrubbed from the environment).

### `config fetch`

Fetch and print models from an Aperture gateway.

```bash
aperture-router config fetch --url http://100.100.100.100
```

| Flag | Description |
|------|-------------|
| `-u, --url <URL>` | Aperture gateway URL (required) |

Prints a table of providers and their models (`id → canonical_id`). Does not modify any config.

### `config list`

Print the current configuration.

```bash
aperture-router config list
aperture-router --config /etc/aperture-router/config.toml config list
```

Shows server address, Aperture URL, multi-provider status, and each provider's URL, style, models, and key status. Secrets are not printed.

### `config enable <NAME>`

Enable a provider in the config file.

```bash
aperture-router config enable zai-coding
```

Sets `enabled = true` for the named provider and saves. Errors if the provider does not exist.

### `config disable <NAME>`

Disable a provider in the config file.

```bash
aperture-router config disable zai-coding
```

Sets `enabled = false`. Restart the service to apply.

### `config export`

Export the config in various formats.

```bash
aperture-router config export --toml
aperture-router config export --opencode
aperture-router config export --openclaw
aperture-router config export --opencode -o ~/.config/opencode/opencode.json
```

| Flag | Description |
|------|-------------|
| `--toml` | Export as TOML (default if no format flag) |
| `--opencode` | Export as `opencode.json` |
| `--openclaw` | Export as `openclaw.json` |
| `-o, --output <PATH>` | Output path |
| `--router-url <URL>` | Router URL for export formats (default `http://127.0.0.1:8765`) |

`--opencode` and `--openclaw` fetch live models from Aperture and merge with an existing file at the output path (if present). Symlinks are refused. Output files are set to mode `0600`.

### `config validate`

Validate the config file.

```bash
aperture-router config validate
```

Runs `Config::validate()` and prints a safe summary (no secrets). Exits non-zero on failure.

## Environment variables

| Variable | Effect |
|----------|--------|
| `APERTURE_BASE_URL` | Aperture gateway URL |
| `APERTURE_API_KEY` | Aperture gateway API key (removed from env after load) |
| `APERTURE_CLIENT_API_KEYS` | Comma-separated client auth keys (removed from env after load) |
| `APERTURE_HOST` | Override `host` |
| `APERTURE_PORT` | Override `port` |
| `APERTURE_ALLOW_NO_AUTH` | Disable auth requirement |
| `RUST_LOG` | Log filter (default `aperture_router=info`) |

`APERTURE_API_KEY` sets the gateway key used when proxying to Aperture — it is **not** a client auth key. Client auth keys go in `security.api_keys` in the config file, or in `APERTURE_CLIENT_API_KEYS` (comma-separated) for env-only setups.

## Logging

Without `--debug`, the filter is `RUST_LOG` or `aperture_router=info`. With `--debug`, it is `aperture_router=debug,tower_http=debug,axum=debug`.

Every request is logged with `request_id` and `session_id` in a tracing span:

```
request_id=abc-123 session_id=550e8400... method=POST path=/v1/messages "Request started"
```

## Signals

| Signal | Behavior |
|--------|----------|
| `SIGINT` (Ctrl-C) | Graceful shutdown: cancel background tasks, wait for completion |
| `SIGTERM` | Same as SIGINT |

Background tasks joined on shutdown: cleanup task (auth attempts), model refresh task, rate-limiter cleanup task.

## Examples

```bash
# Quick start, env-only
export APERTURE_BASE_URL=http://100.100.100.100
aperture-router

# System service config with generated key
sudo aperture-router config generate --url http://100.100.100.100 --generate-key

# Validate and inspect
aperture-router config validate
aperture-router config list

# Export for OpenCode
aperture-router config export --opencode -o ~/.config/opencode/opencode.json

# Debug run
aperture-router --debug --config ./dev-config.toml
```
