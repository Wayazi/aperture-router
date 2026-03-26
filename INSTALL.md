# Installation Guide for aperture-router

This guide covers installation and setup methods for aperture-router.

## Table of Contents

- [Requirements](#requirements)
- [Quick Start](#quick-start)
- [Installation Methods](#installation-methods)
- [Configuration](#configuration)
- [Systemd Service](#systemd-service)
- [Verification](#verification)
- [Troubleshooting](#troubleshooting)

## Requirements

- Linux (x86_64 or aarch64)
- Network access to your Tailscale Aperture gateway

## Quick Start

The fastest way to get running:

```bash
# Install
yay -S aperture-router  # Arch Linux
# or
cargo install aperture-router  # Any Linux

# Set your Aperture URL and run
export APERTURE_BASE_URL=http://your-aperture-gateway:8080
aperture-router
```

That's it! No config file needed.

## Installation Methods

### AUR (Arch Linux) - Recommended

```bash
yay -S aperture-router
# or
paru -S aperture-router
```

This includes:
- Binary at `/usr/bin/aperture-router`
- Systemd service files
- Sysusers.d for automatic user creation
- Sysconfig template

### Cargo

```bash
cargo install aperture-router
```

Binary installed to `~/.cargo/bin/aperture-router`. Make sure `~/.cargo/bin` is in your PATH.

### From Source

```bash
git clone https://github.com/Wayazi/aperture-router
cd aperture-router
cargo build --release
sudo cp target/release/aperture-router /usr/local/bin/
```

### Pre-built Binary

Download from [Releases](https://github.com/Wayazi/aperture-router/releases):

```bash
wget https://github.com/Wayazi/aperture-router/releases/download/v0.2.0/aperture-router-x86_64-linux.tar.gz
tar xzf aperture-router-x86_64-linux.tar.gz
sudo cp aperture-router /usr/local/bin/
```

## Configuration

### Method 1: Environment Variables (No Config File)

```bash
# Required
export APERTURE_BASE_URL=http://your-aperture-gateway:8080

# Optional - API key for authentication
export APERTURE_API_KEY=your-api-key-at-least-32-characters

# Optional - Allow running without auth (development only)
export APERTURE_ALLOW_NO_AUTH=1

# Start
aperture-router
```

### Method 2: Generate Config

```bash
# Auto-generate config with API key
aperture-router config generate \
  --url http://your-aperture-gateway:8080 \
  --generate-key

# Or specify output path
aperture-router config generate \
  --url http://your-aperture-gateway:8080 \
  --output /etc/aperture-router/config.toml
```

### Method 3: Interactive Wizard

```bash
aperture-router config wizard
```

The wizard will:
1. Connect to your Aperture gateway
2. Discover available models
3. Let you select providers
4. Generate config file
5. Optionally create OpenCode config

### Method 4: Manual Config File

Create `config.toml`:

```toml
host = "127.0.0.1"
port = 8765

[aperture]
base_url = "http://your-aperture-gateway:8080"

[security]
api_keys = ["your-api-key-at-least-32-characters"]
```

## Systemd Service (Arch Linux)

The AUR package includes everything for systemd:

```bash
# Set your Aperture URL
echo "APERTURE_BASE_URL=http://your-aperture-gateway:8080" | sudo tee /etc/sysconfig/aperture-router

# Optional: Add API key
echo "APERTURE_API_KEY=your-api-key-here" | sudo tee -a /etc/sysconfig/aperture-router

# Enable and start
sudo systemctl enable --now aperture-router

# Check status
sudo systemctl status aperture-router

# View logs
journalctl -u aperture-router -f
```

### Manual Systemd Setup

If not using AUR:

```bash
# Create user
sudo useradd -r -s /bin/false -d /var/lib/aperture-router aperture-router

# Install service files
sudo cp contrib/systemd/aperture-router.service /etc/systemd/system/
sudo cp contrib/systemd/aperture-router.sysusers /usr/lib/sysusers.d/aperture-router.conf
sudo cp contrib/systemd/aperture-router.tmpfiles /usr/lib/tmpfiles.d/aperture-router.conf

# Create directories
sudo mkdir -p /etc/aperture-router /var/lib/aperture-router
sudo chown aperture-router:aperture-router /var/lib/aperture-router

# Reload and start
sudo systemctl daemon-reload
sudo systemctl enable --now aperture-router
```

## Verification

### Check Version

```bash
aperture-router --version
# aperture-router 0.2.0
```

### Test Health Endpoint

```bash
curl http://127.0.0.1:8765/health
# {"status":"ok"}
```

### List Available Models

```bash
curl http://127.0.0.1:8765/v1/models
```

### Test with API Key

```bash
curl -H "Authorization: Bearer your-api-key" \
  http://127.0.0.1:8765/v1/models
```

## Troubleshooting

### "No config file found and APERTURE_BASE_URL not set"

Set the environment variable:
```bash
export APERTURE_BASE_URL=http://your-aperture-gateway:8080
```

Or create a config file:
```bash
aperture-router config generate --url http://your-aperture-gateway:8080
```

### "Permission denied" reading config

For systemd service, check file permissions:
```bash
ls -la /etc/aperture-router/config.toml
# Should be: -rw-r----- (640) or -rw------- (600)

# Fix permissions
sudo chmod 640 /etc/aperture-router/config.toml
sudo chown root:aperture-router /etc/aperture-router/config.toml
```

Add your user to the group:
```bash
sudo usermod -aG aperture-router $USER
# Log out and back in for changes to take effect
```

### "Production mode requires authentication but no API keys configured"

Add an API key:

**Option 1:** Environment variable
```bash
export APERTURE_API_KEY=$(aperture-router config generate --url http://gateway --generate-key 2>&1 | grep "Generated" | awk '{print $4}')
```

**Option 2:** Config file
```bash
aperture-router config generate --url http://gateway --generate-key
```

**Option 3:** Disable auth (development only)
```bash
export APERTURE_ALLOW_NO_AUTH=1
```

### "Port already in use"

Change port in config or environment:
```bash
export APERTURE_PORT=8766
# or in config.toml: port = 8766
```

### Connection to Aperture fails

1. Verify Aperture is accessible:
   ```bash
   curl http://your-aperture-gateway:8080/v1/models
   ```

2. Check firewall allows outbound connections

3. Verify Tailscale is running:
   ```bash
   tailscale status
   ```

## Uninstallation

### AUR

```bash
yay -R aperture-router
# Config preserved in /etc/sysconfig/aperture-router
```

### Cargo

```bash
cargo uninstall aperture-router
```

### Manual

```bash
sudo systemctl stop aperture-router
sudo systemctl disable aperture-router
sudo rm /usr/local/bin/aperture-router
sudo rm -rf /etc/aperture-router
sudo userdel aperture-router
```

## Next Steps

- [README.md](README.md) - Usage examples and CLI commands
- [DEPLOYMENT.md](DEPLOYMENT.md) - Production deployment guide
- [CHANGELOG.md](CHANGELOG.md) - Version history
