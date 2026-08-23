# Deploy with systemd

> Production deployment of aperture-router as a systemd service on Arch Linux.

## Prerequisites

- Arch Linux (or an AUR-capable distro)
- An Aperture gateway reachable from the host
- Root/sudo access

## 1. Install the package

```bash
yay -S aperture-router
# or
paru -S aperture-router
```

The AUR package installs:

| Path | Purpose |
|------|---------|
| `/usr/bin/aperture-router` | Binary |
| `/etc/systemd/system/aperture-router.service` | Service unit |
| `/etc/sysconfig/aperture-router` | Environment file |
| `/etc/tmpfiles.d/aperture-router.conf` | State directories |
| `/usr/lib/sysusers.d/aperture-router.conf` | System user |

The `aperture-router` system user and the `/var/lib/aperture-router` state directory are created automatically by `systemd-sysusers` and `systemd-tmpfiles` on boot (or on demand — see step 3).

## 2. Configure the service

Edit `/etc/sysconfig/aperture-router`:

```bash
APERTURE_BASE_URL=http://100.100.100.100
APERTURE_HOST=0.0.0.0
APERTURE_PORT=8080
RUST_LOG=info
APERTURE_CLIENT_API_KEYS=your-strong-api-key-here
```

| Variable | Default | Description |
|----------|---------|-------------|
| `APERTURE_BASE_URL` | — | Aperture gateway URL (required) |
| `APERTURE_HOST` | `127.0.0.1` | Listen address |
| `APERTURE_PORT` | `8765` | Listen port |
| `APERTURE_CLIENT_API_KEYS` | — | Client auth keys for inbound requests (comma-separated; min 32 chars each) |
| `APERTURE_API_KEY` | — | Aperture *gateway* key (upstream) — set only if your Aperture gateway itself requires auth |
| `RUST_LOG` | `aperture_router=info` | Log filter |
| `APERTURE_ALLOW_NO_AUTH` | — | Set `1` to disable auth (dev only) |

Environment variables override values in `config.toml`.

Alternatively, use a config file at `/etc/aperture-router/config.toml`:

```bash
sudo cp config.example.toml /etc/aperture-router/config.toml
sudo chmod 640 /etc/aperture-router/config.toml
sudo chown root:aperture-router /etc/aperture-router/config.toml
```

Generate a config non-interactively:

```bash
sudo aperture-router config generate --url http://100.100.100.100 --generate-key
```

## 3. Create runtime directories (if not auto-created)

```bash
sudo systemd-tmpfiles --create /etc/tmpfiles.d/aperture-router.conf
```

This creates `/var/lib/aperture-router` and `/var/lib/aperture-router/cache` owned by the `aperture-router` user (mode `0750`).

## 4. Enable and start

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now aperture-router
```

## 5. Verify

```bash
sudo systemctl status aperture-router
curl http://127.0.0.1:8080/health
```

Expected health response:

```json
{"status":"ok","service":"aperture-router","version":"0.3.1"}
```

## Logs

```bash
# Follow logs
sudo journalctl -u aperture-router -f

# Last 100 lines
sudo journalctl -u aperture-router -n 100

# Errors only
sudo journalctl -u aperture-router -p err

# Since boot
sudo journalctl -u aperture-router -b
```

Log lines include `request_id` and `session_id` fields for correlation.

## Service management

```bash
sudo systemctl restart aperture-router   # Restart
sudo systemctl stop aperture-router      # Stop
sudo systemctl disable aperture-router   # Disable autostart
```

## Resource limits

The unit file sets `LimitNOFILE=65536` and `LimitNPROC=4096`. Override:

```bash
sudo systemctl edit aperture-router
```

```ini
[Service]
LimitNOFILE=131072
```

## Security hardening (built-in)

The service unit applies: `NoNewPrivileges`, `PrivateTmp`, `ProtectSystem=strict`, `ProtectHome`, `CapabilityBoundingSet=CAP_NET_BIND_SERVICE`, `ProtectKernelModules`, `ProtectKernelTunels`, `MemoryDenyWriteExecute`, `LockPersonality`, `PrivateDevices`, `ProtectHostname`, `RestrictNamespaces`, `SystemCallArchitectures=native`, `SystemCallFilter=@system-service`. Only `/var/lib/aperture-router` is writable (`ReadWritePaths`).

## Updates

```bash
yay -Syu aperture-router
sudo systemctl restart aperture-router
```

After updates, diff the example config to catch new options:

```bash
diff config.example.toml /etc/aperture-router/config.toml
```

## Troubleshooting

| Symptom | Check |
|---------|-------|
| Service won't start | `journalctl -u aperture-router -n 50` |
| Permission denied reading config | `namei -l /etc/aperture-router/config.toml` (should be `640 root:aperture-router`) |
| Port in use | `ss -tlnp \| grep :8080` |
| Config validation error | `sudo -u aperture-router aperture-router --config /etc/aperture-router/config.toml config validate` |
| No models discovered | Verify `APERTURE_BASE_URL` is reachable: `curl http://100.100.100.100/v1/models` |

## Backup

```bash
sudo tar czf aperture-router-backup-$(date +%Y%m%d).tar.gz \
  /etc/aperture-router /etc/sysconfig/aperture-router
```
