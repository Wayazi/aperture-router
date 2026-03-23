# Installation Guide for aperture-router

This guide covers various installation methods for aperture-router.

## Table of Contents

- [Requirements](#requirements)
- [Installation Methods](#installation-methods)
  - [Cargo Install](#cargo-install)
  - [AUR (Arch Linux)]#aur-arch-linux)
  - [Build from Source](#build-from-source)
  - [Pre-built Binary](#pre-built-binary)
- [Post-Installation](#post-installation)
- [Verification](#verification)

## Requirements

- Linux (x86_64 or aarch64)
- Rust 1.70+ (for building from source)
- Network access to your Tailscale Aperture gateway

## Installation Methods

### Cargo Install

The easiest way to install aperture-router is using Cargo:

```bash
cargo install aperture-router
```

This will:
1. Download and compile aperture-router
2. Install the binary to `~/.cargo/bin/aperture-router`

Make sure `~/.cargo/bin` is in your PATH:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### AUR (Arch Linux)

If you're on Arch Linux, install from the AUR:

```bash
# Using yay
yay -S aperture-router

# Using paru
paru -S aperture-router

# Manual installation
git clone https://aur.archlinux.org/aperture-router.git
cd aperture-router
makepkg -si
```

### Build from Source

To build from source:

```bash
# Clone the repository
git clone https://github.com/Wayazi/aperture-router
cd aperture-router

# Build in release mode
cargo build --release

# Install to system directory
sudo cp target/release/aperture-router /usr/local/bin/
```

### Pre-built Binary

Download a pre-built binary from the [Releases](https://github.com/Wayazi/aperture-router/releases) page:

```bash
# Download for x86_64
wget https://github.com/Wayazi/aperture-router/releases/download/v0.1.0/aperture-router-x86_64-linux.tar.gz

# Extract
tar xzf aperture-router-x86_64-linux.tar.gz

# Install
sudo cp aperture-router /usr/local/bin/

# Verify checksum
sha256sum -c aperture-router-x86_64-linux.sha256
```

## Post-Installation

### 1. Create Configuration Directory

```bash
sudo mkdir -p /etc/aperture-router
```

### 2. Copy Example Configuration

```bash
sudo cp config.example.toml /etc/aperture-router/config.toml
```

Or download from GitHub:

```bash
sudo wget -O /etc/aperture-router/config.toml \
  https://raw.githubusercontent.com/Wayazi/aperture-router/main/config.example.toml
```

### 3. Edit Configuration

```bash
sudo editor /etc/aperture-router/config.toml
```

Minimum required configuration:

```toml
[aperture]
base_url = "http://100.100.100.100"  # Your Aperture gateway IP
```

### 4. Create User (for systemd service)

```bash
sudo useradd -r -s /bin/false -d /var/lib/aperture-router aperture-router
```

### 5. Create State Directory

```bash
sudo mkdir -p /var/lib/aperture-router
sudo chown -R aperture-router:aperture-router /var/lib/aperture-router
```

## Systemd Service (Optional)

For production use, install the systemd service:

```bash
# Install service files
sudo cp contrib/systemd/aperture-router.service /etc/systemd/system/
sudo cp contrib/systemd/aperture-router.sysconfig /etc/sysconfig/aperture-router
sudo cp contrib/systemd/aperture-router.tmpfiles /etc/tmpfiles.d/aperture-router.conf

# Reload systemd
sudo systemctl daemon-reload

# Enable and start
sudo systemctl enable --now aperture-router

# Check status
sudo systemctl status aperture-router
```

See [DEPLOYMENT.md](DEPLOYMENT.md) for more deployment details.

## Verification

### Check Version

```bash
aperture-router --version
```

Expected output:
```
aperture-router 0.1.0
```

### Test Health Endpoint

```bash
curl http://localhost:8080/health
```

Expected output:
```json
{"status":"healthy"}
```

### List Models

```bash
curl http://localhost:8080/v1/models
```

## Troubleshooting

### Binary Not Found

If `aperture-router` is not found, check your PATH:

```bash
echo $PATH
which aperture-router
```

If using Cargo install, add to your `~/.bashrc` or `~/.zshrc`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### Permission Denied

Make the binary executable:

```bash
chmod +x aperture-router
```

### Port Already in Use

Check what's using port 8080:

```bash
sudo ss -tlnp | grep :8080
```

Change the port in `/etc/aperture-router/config.toml`:

```toml
[server]
port = 8081
```

### Connection Refused

1. Verify Aperture gateway is accessible:
   ```bash
   curl http://100.100.100.100/health
   ```

2. Check aperture-router logs:
   ```bash
   journalctl -u aperture-router -f
   ```

3. Verify configuration:
   ```bash
   aperture-router --config /etc/aperture-router/config.toml --check
   ```

## Uninstallation

### Cargo

```bash
cargo uninstall aperture-router
```

### AUR

```bash
yay -R aperture-router
```

### Manual Installation

```bash
sudo rm /usr/local/bin/aperture-router
sudo rm -rf /etc/aperture-router
sudo userdel aperture-router
```

### Systemd Service

```bash
sudo systemctl stop aperture-router
sudo systemctl disable aperture-router
sudo rm /etc/systemd/system/aperture-router.service
sudo rm /etc/sysconfig/aperture-router
sudo rm /etc/tmpfiles.d/aperture-router.conf
sudo systemctl daemon-reload
```

## Next Steps

- See [DEPLOYMENT.md](DEPLOYMENT.md) for production deployment
- See [README.md](README.md) for usage examples
- See [CHANGELOG.md](CHANGELOG.md) for version history
