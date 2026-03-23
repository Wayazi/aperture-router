# Deployment Guide for aperture-router

This guide covers production deployment of aperture-router.

## Table of Contents

- [Production Configuration](#production-configuration)
- [Systemd Service](#systemd-service)
- [Security Hardening](#security-hardening)
- [Monitoring](#monitoring)
- [Performance Tuning](#performance-tuning)
- [High Availability](#high-availability)
- [Troubleshooting](#troubleshooting)

## Production Configuration

### Minimal Production Config

```toml
# /etc/aperture-router/config.toml

[server]
host = "0.0.0.0"  # Listen on all interfaces
port = 8080

[aperture]
base_url = "http://100.100.100.100"  # Your Aperture gateway
model_refresh_interval_secs = 300

[logging]
level = "info"  # Use "warn" in production for less verbosity

[security]
# Enable authentication in production
api_keys = ["your-strong-api-key-here"]

# Rate limiting
max_auth_attempts = 5
auth_window_secs = 60
ban_duration_secs = 300

# Request limits
max_body_size_bytes = 10485760  # 10MB
max_json_depth = 256
max_streaming_size_bytes = 104857600  # 100MB

# Require auth in production
require_auth_in_prod = true
```

### Environment Variables

For sensitive configuration, use environment variables:

```bash
# /etc/sysconfig/aperture-router
APERTURE_BASE_URL=http://100.100.100.100
APERTURE_ROUTER_HOST=0.0.0.0
APERTURE_ROUTER_PORT=8080
RUST_LOG=info
APERTURE_API_KEY=your-strong-api-key-here
```

## Systemd Service

### Installation

```bash
# Install service files
sudo cp contrib/systemd/aperture-router.service /etc/systemd/system/
sudo cp contrib/systemd/aperture-router.sysconfig /etc/sysconfig/aperture-router
sudo cp contrib/systemd/aperture-router.tmpfiles /etc/tmpfiles.d/aperture-router.conf

# Create user
sudo useradd -r -s /bin/false -d /var/lib/aperture-router aperture-router

# Create directories
sudo mkdir -p /etc/aperture-router /var/lib/aperture-router
sudo chown -R aperture-router:aperture-router /var/lib/aperture-router

# Set permissions
sudo chmod 750 /var/lib/aperture-router
sudo chmod 640 /etc/aperture-router/config.toml
sudo chown root:aperture-router /etc/aperture-router/config.toml

# Reload and start
sudo systemctl daemon-reload
sudo systemctl enable --now aperture-router
```

### Service Management

```bash
# Check status
sudo systemctl status aperture-router

# View logs
sudo journalctl -u aperture-router -f

# Restart
sudo systemctl restart aperture-router

# Stop
sudo systemctl stop aperture-router
```

## Security Hardening

### 1. Use Strong API Keys

Generate a strong API key:

```bash
# Generate 32-character random key
openssl rand -base64 24
```

### 2. Enable Authentication

Always enable authentication in production:

```toml
[security]
api_keys = ["your-strong-api-key-here"]
require_auth_in_prod = true
```

### 3. Use HTTPS/TLS

For production, put aperture-router behind a reverse proxy with TLS:

```nginx
# /etc/nginx/conf.d/aperture-router.conf
server {
    listen 443 ssl http2;
    server_name aperture.example.com;

    ssl_certificate /etc/letsencrypt/live/aperture.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/aperture.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

### 4. Firewall Configuration

Allow only necessary ports:

```bash
# UFW
sudo ufw allow 443/tcp
sudo ufw enable

# firewalld
sudo firewall-cmd --permanent --add-service=https
sudo firewall-cmd --reload
```

### 5. Network Isolation

- Run on internal network only
- Use Tailscale for remote access
- Restrict access to trusted clients only

## Monitoring

### Health Checks

```bash
# Simple health check
curl http://localhost:8080/health

# With timeout
timeout 5 curl -f http://localhost:8080/health || echo "Health check failed"
```

### Log Monitoring

View real-time logs:

```bash
sudo journalctl -u aperture-router -f
```

Filter by severity:

```bash
# Errors only
sudo journalctl -u aperture-router -p err

# Warnings and errors
sudo journalctl -u aperture-router -p warn..err
```

### Metrics

The service logs important events:
- Authentication failures
- Rate limit violations
- SSRF attempts
- Model validation failures
- Upstream errors

Set up alerts for:
- `Health check failed`
- `Rate limit exceeded`
- `SSRF blocked`
- `Upstream request failed`

## Performance Tuning

### Connection Pooling

The default HTTP client settings are optimized for most workloads. To adjust:

```toml
[http]
# Default timeouts work well for most cases
connect_timeout_secs = 10
request_timeout_secs = 300
```

### Rate Limiting

Adjust rate limits based on your needs:

```toml
[security]
# More lenient for trusted clients
max_auth_attempts = 10
auth_window_secs = 60

# More strict for public-facing deployments
max_auth_attempts = 3
auth_window_secs = 60
```

### Resource Limits

Systemd service already includes resource limits:

```ini
[Service]
LimitNOFILE=65536
LimitNPROC=4096
```

Adjust if needed:

```bash
# Edit service override
sudo systemctl edit aperture-router

[Service]
LimitNOFILE=131072
```

## High Availability

### Multiple Instances

Run multiple instances behind a load balancer:

```bash
# Instance 1
APERTURE_ROUTER_PORT=8080 aperture-router &

# Instance 2
APERTURE_ROUTER_PORT=8081 aperture-router &
```

Configure HAProxy or nginx:

```nginx
upstream aperture {
    least_conn;
    server 127.0.0.1:8080;
    server 127.0.0.1:8081;
}

server {
    listen 443 ssl;
    location / {
        proxy_pass http://aperture;
    }
}
```

### Health Checks with Load Balancer

Configure health checks:

```nginx
upstream aperture {
    server 127.0.0.1:8080 max_fails=3 fail_timeout=30s;
    server 127.0.0.1:8081 max_fails=3 fail_timeout=30s;
}
```

## Troubleshooting

### Service Won't Start

1. Check logs:
   ```bash
   sudo journalctl -u aperture-router -n 50
   ```

2. Verify configuration:
   ```bash
   sudo -u aperture-router aperture-router --config /etc/aperture-router/config.toml
   ```

3. Check file permissions:
   ```bash
   namei -l /etc/aperture-router/config.toml
   ```

### High Memory Usage

1. Check current usage:
   ```bash
   systemctl status aperture-router
   ```

2. Reduce limits:
   ```toml
   [security]
   max_streaming_size_bytes = 52428800  # 50MB
   ```

### Slow Responses

1. Check upstream latency:
   ```bash
   time curl http://100.100.100.100/health
   ```

2. Adjust timeouts:
   ```toml
   [http]
   request_timeout_secs = 600
   ```

### Database Lock Errors

If using model caching, ensure proper file permissions:

```bash
sudo chown -R aperture-router:aperture-router /var/lib/aperture-router
```

## Backup and Recovery

### Backup Configuration

```bash
sudo tar czf aperture-router-backup-$(date +%Y%m%d).tar.gz \
  /etc/aperture-router \
  /etc/systemd/system/aperture-router.service \
  /etc/sysconfig/aperture-router
```

### Restore Configuration

```bash
sudo tar xzf aperture-router-backup-YYYYMMDD.tar.gz -C /
sudo systemctl daemon-reload
sudo systemctl restart aperture-router
```

## Updates

### Update from AUR

```bash
yay -Syu aperture-router
sudo systemctl restart aperture-router
```

### Update from Source

```bash
git pull
cargo build --release
sudo systemctl stop aperture-router
sudo cp target/release/aperture-router /usr/local/bin/
sudo systemctl start aperture-router
```

### Update Configuration

After updates, review configuration changes:

```bash
diff config.example.toml /etc/aperture-router/config.toml
```

## Support

For issues and questions:
- GitHub Issues: https://github.com/Wayazi/aperture-router/issues
- Documentation: https://github.com/Wayazi/aperture-router
