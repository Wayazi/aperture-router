# Setup a reverse proxy with TLS

> Put nginx or Caddy in front of aperture-router to terminate TLS and forward to the local service.

## Why

aperture-router listens on plain HTTP. For browser-facing or remote deployments, terminate TLS with a reverse proxy and expose only 443. The proxy also lets you set `X-Forwarded-For` so the router sees the real client IP for rate limiting.

## Prerequisites

- aperture-router running on `127.0.0.1:8765` (or any local port)
- A domain name pointing at the host
- Root/sudo access

## Configure trusted proxies

So the router honors `X-Forwarded-For`, add the proxy IP to `trusted_proxies`:

```toml
[cors]
trusted_proxies = ["127.0.0.1", "::1"]
```

Only connections from a trusted-proxy IP will read `X-Forwarded-For`; all others use the TCP peer IP. This prevents spoofing.

## nginx

### Install

```bash
sudo apt install nginx          # Debian/Ubuntu
sudo dnf install nginx          # Fedora
sudo pacman -S nginx            # Arch
```

### TLS with certbot

```bash
sudo certbot certonly --nginx -d aperture.example.com
```

### Site config

`/etc/nginx/conf.d/aperture-router.conf`:

```nginx
server {
    listen 443 ssl http2;
    server_name aperture.example.com;

    ssl_certificate     /etc/letsencrypt/live/aperture.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/aperture.example.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;

    # Streaming: disable buffering
    proxy_buffering off;
    proxy_cache off;
    proxy_read_timeout 600s;
    proxy_send_timeout 600s;

    location / {
        proxy_pass http://127.0.0.1:8765;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # SSE: HTTP/1.1, no chunked buffering
        proxy_http_version 1.1;
        proxy_set_header Connection "";
    }
}

# Redirect HTTP -> HTTPS
server {
    listen 80;
    server_name aperture.example.com;
    return 301 https://$host$request_uri;
}
```

Apply:

```bash
sudo nginx -t && sudo systemctl reload nginx
```

### Notes for streaming

- `proxy_buffering off` is **required** for SSE streaming (`/v1/messages`, `/v1/chat/completions` with `stream: true`).
- `proxy_read_timeout 600s` matches the router's default `request_timeout_secs = 300` with headroom.
- HTTP/2 to the client is fine; HTTP/1.1 to the upstream (`proxy_http_version 1.1`) avoids SSE issues.

## Caddy

Caddy obtains and renews certificates automatically.

`/etc/caddy/Caddyfile`:

```caddyfile
aperture.example.com {
    reverse_proxy 127.0.0.1:8765 {
        header_up X-Real-IP {remote_host}
        header_up X-Forwarded-For {remote_host}
        header_up X-Forwarded-Proto {scheme}

        # Flush for SSE streaming
        flush_interval -1
    }
}
```

Apply:

```bash
sudo systemctl reload caddy
```

`flush_interval -1` disables buffering so SSE chunks flush immediately.

## Firewall

Allow only 80/443:

```bash
# UFW
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw enable

# firewalld
sudo firewall-cmd --permanent --add-service=http
sudo firewall-cmd --permanent --add-service=https
sudo firewall-cmd --reload
```

Keep the router port (`8765`) blocked from external access; it should only be reachable on `127.0.0.1`.

## CORS

If browser clients hit the router directly through the proxy, configure allowed origins:

```toml
[cors]
allowed_origins = ["https://app.example.com"]
trusted_proxies = ["127.0.0.1", "::1"]
```

Do **not** use `"*"` in production — credentials mode breaks and it allows any origin.

## Verifying

```bash
curl -v https://aperture.example.com/health
```

Check the `X-Session-ID` response header is present, and that logs show the real client IP (not `127.0.0.1`):

```bash
sudo journalctl -u aperture-router -f | grep "Request started"
```

## Load balancing multiple instances

```nginx
upstream aperture {
    least_conn;
    server 127.0.0.1:8765 max_fails=3 fail_timeout=30s;
    server 127.0.0.1:8766 max_fails=3 fail_timeout=30s;
}

server {
    listen 443 ssl;
    # ...
    location / {
        proxy_pass http://aperture;
        # ... same headers as above
    }
}
```

Run instances with different ports:

```bash
APERTURE_PORT=8765 aperture-router &
APERTURE_PORT=8766 aperture-router &
```

Note: rate-limit state is per-process, not shared across instances.

## Security headers

The router already sets `Content-Security-Policy`, `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`, `Strict-Transport-Security`, `Referrer-Policy`, and `Permissions-Policy`. You do not need to duplicate them in the proxy, but adding HSTS at the proxy edge is harmless.
