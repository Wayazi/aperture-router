# Security Audit Report: aperture-router

**Audit Date:** 2026-04-27
**Auditor:** Security Review via Claude Code
**Project:** aperture-router v0.2.2
**Scope:** Full codebase line-by-line review

---

## Executive Summary

**Overall Security Score: B+**

The aperture-router project demonstrates strong security awareness with comprehensive SSRF protection, timing-attack-resistant authentication, and proper input validation. However, several issues require attention.

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 1 | Requires immediate fix |
| MEDIUM | 1 | Should be fixed soon |
| LOW | 3 | Should be addressed |

---

## Detailed Findings

### Issue 1: CRITICAL - ConnectInfo Not Configured

**Severity:** CRITICAL
**CWE:** CWE-287 (Improper Authentication)
**Files:** `src/main.rs:246-250`, `src/middleware/auth.rs:200-237`

#### Description

The Axum server is configured without `into_make_service_with_connect_info::<SocketAddr>()`, meaning `ConnectInfo` extension is never populated. This causes the `extract_client_ip()` function to fall back to `0.0.0.0` for ALL requests.

#### Affected Code

```rust
// src/main.rs:246-250
axum::serve(listener, app)  // Missing ConnectInfo configuration
    .with_graceful_shutdown(shutdown_signal(shutdown_token.clone()))
    .await?;

// src/middleware/auth.rs:207-218
let peer_ip = request
    .extensions()
    .get::<ConnectInfo<std::net::SocketAddr>>()
    .map(|info| info.ip())
    .unwrap_or_else(|| {
        // This fallback triggers for ALL requests
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0))
    });
```

#### Impact

1. **Rate Limiting Completely Broken** - All users share `0.0.0.0` IP bucket
2. **Security Log Corruption** - All logs show `0.0.0.0` instead of real IPs
3. **Trusted Proxy Check Bypassed** - X-Forwarded-For header processing skipped

#### Attack Scenario

1. Attacker makes 5 failed auth attempts
2. **Every legitimate user worldwide** is now rate-limited
3. Complete denial of service without any real attack volume

#### Recommended Fix

```rust
// src/main.rs
axum::serve(
    listener,
    app.into_make_service_with_connect_info::<std::net::SocketAddr>()
)
.with_graceful_shutdown(shutdown_signal(shutdown_token.clone()))
.await?;

// src/middleware/auth.rs
let peer_ip = request
    .extensions()
    .get::<ConnectInfo<std::net::SocketAddr>>()
    .map(|info| info.ip())
    .ok_or_else(|| {
        tracing::error!("ConnectInfo not available - server misconfiguration");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
```

---

### Issue 2: MEDIUM - Shell Command PATH Vulnerability

**Severity:** MEDIUM
**CWE:** CWE-426 (Untrusted Search Path)
**File:** `src/cli/commands.rs:42-47`

#### Description

The `fix_system_config_ownership()` function executes `chown` via `std::process::Command::new("chown")` without an absolute path, allowing PATH manipulation attacks.

#### Affected Code

```rust
let output = std::process::Command::new("chown")  // Relative path!
    .arg("aperture-router:aperture-router")
    .arg(path)
    .output();
```

#### Impact

An attacker who can manipulate the PATH environment variable (e.g., via compromised AUR helper, sudo configuration, or malicious environment) could place a malicious `chown` binary that executes with root privileges.

#### Attack Scenario

1. Attacker creates `/tmp/malicious/chown` (malicious script)
2. Attacker sets `PATH=/tmp/malicious:$PATH`
3. Code runs as root via sudo
4. Malicious binary executes with root privileges

#### Recommended Fix

Use native Rust syscalls via `nix` crate:

```rust
use nix::unistd::{chown, User, Group};

fn fix_system_config_ownership(path: &str) -> anyhow::Result<()> {
    let user = User::from_name("aperture-router")?.unwrap();
    let group = Group::from_name("aperture-router")?.unwrap();
    chown(std::path::Path::new(path), Some(user.uid), Some(group.gid))?;
    Ok(())
}
```

---

### Issue 3: LOW - API Keys Not Zeroized in Config

**Severity:** LOW
**CWE:** CWE-316 (Cleartext Storage of Sensitive Information in Memory)
**Files:** `src/config.rs:111`, `src/main.rs:183-186`

#### Description

The `SecurityConfig` struct stores API keys as plain `Vec<String>` instead of `Zeroizing<String>`. While `AuthState` correctly converts these to zeroized strings, the original plain strings remain in memory in the `Config` struct.

#### Affected Code

```rust
// src/config.rs
pub struct SecurityConfig {
    pub api_keys: Vec<String>,        // Should be Vec<Zeroizing<String>>
    pub admin_api_keys: Vec<String>,  // Should be Vec<Zeroizing<String>>
}

// src/main.rs:183-186
if let Ok(key) = std::env::var("APERTURE_API_KEY") {
    if !key.is_empty() {
        config.security.api_keys = vec![key];  // Stored as plain string
    }
}
```

#### Impact

- API keys persist in memory until garbage collection
- Memory dumps or heap inspection could extract keys
- Environment variable remains visible in `/proc/[pid]/environ`

#### Recommended Fix

```rust
// Remove env var after loading
if let Ok(key) = std::env::var("APERTURE_API_KEY") {
    if !key.is_empty() {
        config.security.api_keys = vec![key];
    }
    std::env::remove_var("APERTURE_API_KEY");
}
```

---

### Issue 4: LOW - .internal Domain Blocking Incomplete

**Severity:** LOW
**CWE:** CWE-918 (Server-Side Request Forgery)
**File:** `src/cli/security.rs:74-89`

#### Description

The `is_blocked_host()` function has a logic flaw in the `.internal` domain blocking due to operator precedence.

#### Affected Code

```rust
|| host.ends_with(".internal") && host.starts_with("metadata")
```

Due to `&&` binding tighter than `||`, this only blocks hosts that BOTH end with `.internal` AND start with `metadata`.

#### Bypass Examples

| Host | Blocked? | Reason |
|------|----------|--------|
| `metadata.internal` | YES | Starts with "metadata" |
| `kubernetes-metadata.internal` | **NO** | Does NOT start with "metadata" |
| `k8s-metadata.internal` | **NO** | Does NOT start with "metadata" |
| `my-metadata.internal` | **NO** | Does NOT start with "metadata" |

#### Recommended Fix

```rust
|| host.ends_with(".internal") && host.contains("metadata")
```

---

### Issue 5: LOW - API Key Printed to Stderr

**Severity:** LOW
**CWE:** CWE-532 (Insertion of Sensitive Information into Log File)
**File:** `src/cli/commands.rs:401-406`

#### Description

When generating an API key via `--generate-key`, the key is printed to stderr which may be captured by logging systems (systemd journal, centralized logging, etc.).

#### Affected Code

```rust
eprintln!("🔑 Generated API key: {}", api_key);
eprintln!("   Save this key securely - it won't be shown again!");
```

#### Impact

- API keys may persist in log files
- Multi-user systems may expose keys via process listing
- Log aggregation systems may store keys indefinitely

#### Recommended Fix

```rust
println!("🔑 Generated API key: {}", api_key);  // stdout instead of stderr
println!("   Save this key securely - it won't be shown again!");
```

---

## Security Strengths (Positive Findings)

The audit also identified many strong security practices:

### 1. SSRF Protection (Excellent)

- **DNS Rebinding Prevention** - Resolves hostnames and validates IPs
- **Metadata Endpoint Blocking** - AWS/GCP/Azure/Alibaba blocked
- **IPv4-mapped IPv6 Detection** - Prevents bypass via IPv6 notation
- **Tailscale CGN Allowed** - Correctly allows `100.64.0.0/10` for Tailscale

### 2. Authentication (Strong)

- **Timing-Safe Comparison** - Uses `subtle::ConstantTimeEq`
- **Zeroizing Keys** - `Zeroizing<String>` in `AuthState`
- **Rate Limiting** - IP-based with atomic operations (TOCTOU-safe)
- **Separate Admin Keys** - Admin endpoints require explicit keys

### 3. Input Validation (Comprehensive)

- **Model Names** - Length limit, path traversal prevention, ASCII-only
- **Request Bodies** - Size limits, JSON depth limits
- **URLs** - Scheme validation, SSRF protection

### 4. Security Headers (Complete)

```rust
Content-Security-Policy: default-src 'self'
X-Frame-Options: DENY
X-Content-Type-Options: nosniff
X-XSS-Protection: 1; mode=block
Strict-Transport-Security: max-age=31536000; includeSubDomains
```

### 5. File Operations (Secure)

- **Atomic Writes** - Uses temp file + rename
- **Symlink Protection** - Refuses to follow symlinks
- **Proper Permissions** - `0o600` for config files

---

## File-by-File Security Scores

| File | Score | Notes |
|------|-------|-------|
| `src/http_client.rs` | A+ | Redirect policy disabled, endpoint whitelist |
| `src/proxy/client.rs` | A+ | Comprehensive SSRF protection |
| `src/config.rs` | A | Strong validation, needs zeroization |
| `src/server.rs` | A | Security headers, body limits |
| `src/middleware/auth.rs` | A | Timing-safe, rate limiting (after fix) |
| `src/cli/commands.rs` | A- | Shell command issue |
| `src/cli/security.rs` | A- | Domain blocking incomplete |
| `src/routes/streaming.rs` | A | Comprehensive limits |
| `src/routes/chat.rs` | A | Input validation |
| `src/routes/messages.rs` | A | Input validation |
| `src/routes/proxy.rs` | A | Response size limits |
| `src/types/validation.rs` | A | Comprehensive validation |
| `src/main.rs` | B+ | ConnectInfo issue |

---

## Remediation Priority

1. **Immediate (Phase 1):** Fix ConnectInfo configuration
2. **Soon (Phase 2):** Replace shell command with native syscall
3. **When Possible (Phases 3-5):** Address remaining LOW issues

---

## Conclusion

The aperture-router codebase demonstrates security-conscious development. The most critical finding (ConnectInfo) has significant impact but a straightforward fix. After remediation, the security score should improve to A-.

**Recommendation:** Implement fixes in order of severity, with full test coverage for each change.
