# Systemd Service Files for aperture-router

This directory contains systemd service configuration files for running aperture-router as a system service.

## Files

- `aperture-router.service` - Systemd service unit file
- `aperture-router.sysconfig` - Environment variables configuration
- `aperture-router.tmpfiles` - tmpfiles.d configuration for state directories

## Installation

### 1. Install the Binary

First, install aperture-router:

```bash
# From AUR
yay -S aperture-router

# Or from source
cargo install --path .
sudo cp ~/.cargo/bin/aperture-router /usr/local/bin/
```

### 2. Create User and Directories

```bash
sudo useradd -r -s /bin/false -d /var/lib/aperture-router aperture-router
sudo mkdir -p /etc/aperture-router
sudo tmpfiles --create --prefix /var/lib/aperture-router
```

### 3. Install Service Files

```bash
sudo cp contrib/systemd/aperture-router.service /etc/systemd/system/
sudo cp contrib/systemd/aperture-router.sysconfig /etc/sysconfig/aperture-router
sudo cp contrib/systemd/aperture-router.tmpfiles /etc/tmpfiles.d/aperture-router.conf
```

### 4. Create Configuration

```bash
sudo cp config.example.toml /etc/aperture-router/config.toml
sudo editor /etc/aperture-router/config.toml
```

### 5. Enable and Start Service

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now aperture-router
sudo systemctl status aperture-router
```

## Management

### Check Status

```bash
sudo systemctl status aperture-router
```

### View Logs

```bash
sudo journalctl -u aperture-router -f
```

### Restart Service

```bash
sudo systemctl restart aperture-router
```

### Stop Service

```bash
sudo systemctl stop aperture-router
```

### Disable Service

```bash
sudo systemctl disable aperture-router
```

## Environment Variables

You can set environment variables in `/etc/sysconfig/aperture-router`:

```bash
# Override Aperture gateway URL
APERTURE_BASE_URL=http://100.100.100.100

# Set server host/port
APERTURE_ROUTER_HOST=0.0.0.0
APERTURE_ROUTER_PORT=8080

# Enable debug logging
RUST_LOG=debug

# Set API key
APERTURE_API_KEY=your-api-key-here
```

## Security Features

The service file includes several security hardening features:

- **NoNewPrivileges**: Prevents process from gaining new privileges
- **PrivateTmp**: Provides private /tmp directory
- **ProtectSystem**: Mounts /usr, /boot, /etc read-only
- **ProtectHome**: Makes home directories inaccessible
- **CapabilityBoundingSet**: Only allows CAP_NET_BIND_SERVICE
- **AmbientCapabilities**: Grants permission to bind to privileged ports

## Resource Limits

- **LimitNOFILE**: 65536 (max open files)
- **LimitNPROC**: 4096 (max processes)

## Troubleshooting

### Service Won't Start

1. Check the logs:
   ```bash
   sudo journalctl -u aperture-router -n 50
   ```

2. Verify configuration:
   ```bash
   sudo -u aperture-router aperture-router --config /etc/aperture-router/config.toml --check
   ```

3. Check file permissions:
   ```bash
   ls -la /etc/aperture-router/
   ls -la /var/lib/aperture-router/
   ```

### Permission Issues

Ensure proper ownership:
```bash
sudo chown -R aperture-router:aperture-router /var/lib/aperture-router
sudo chown root:root /etc/aperture-router/config.toml
sudo chmod 640 /etc/aperture-router/config.toml
```

### Port Already in Use

Check what's using the port:
```bash
sudo ss -tlnp | grep :8080
```

Change the port in `/etc/aperture-router/config.toml` or `/etc/sysconfig/aperture-router`.
