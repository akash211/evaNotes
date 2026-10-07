# evaNotes — Security Checklist

> Every development phase must pass these security gates before merging to `main`.

---

## 1. Dependency Security

- [ ] `cargo audit` — zero known vulnerabilities in dependency tree
- [ ] `cargo deny check advisories` — no security advisories on any crate
- [ ] `cargo deny check licenses` — all dependencies use permissive licenses (MIT, Apache-2.0, BSD)
- [ ] `cargo deny check bans` — no banned/blacklisted crates
- [ ] Minimal dependency surface — no unnecessary crates; audit each new `Cargo.toml` addition
- [ ] `Cargo.lock` committed — deterministic, reproducible builds
- [ ] Dependabot or Renovate configured for automated dependency update PRs

---

## 2. Authentication & Session Security

### 2.1 Google OAuth
- [ ] ID tokens verified **server-side** via Google's `https://oauth2.googleapis.com/tokeninfo` endpoint
- [ ] Token `aud` (audience) field validated against configured `GOOGLE_CLIENT_ID`
- [ ] Token `email_verified` field must be `true`
- [ ] Token `exp` (expiry) checked — reject expired tokens
- [ ] Only `ADMIN_EMAIL` is authorized — reject all other emails with `401`
- [ ] Never trust client-side token validation alone

### 2.2 Email OTP
- [ ] OTP codes: exactly 6 digits, generated via `rand::rngs::OsRng` (cryptographic PRNG)
- [ ] OTP expiry: 5-minute window from generation
- [ ] Single-use: OTP marked `used = 1` immediately upon successful verification
- [ ] Max attempts: 5 failed attempts per OTP → lockout (require new OTP request)
- [ ] Rate limiting: ≤2 OTP request per minute per IP address (sliding window)
- [ ] OTP codes **never** logged when SMTP is configured (only logged to stdout when SMTP is unconfigured for dev convenience)
- [ ] Timing-safe comparison: use `constant_time_eq` or `hmac::verify` (prevent timing attacks)

### 2.3 Sessions
- [ ] Session tokens: HMAC-SHA256 signed using `SESSION_SECRET` environment variable
- [ ] `SESSION_SECRET` auto-generated (32 bytes, `OsRng`) if not provided — logged once on startup
- [ ] Cookie attributes: `HttpOnly`, `SameSite=Strict`, `Path=/`
- [ ] `Secure` flag set when `SECURE_COOKIES=true` (mandatory behind HTTPS proxy)
- [ ] Session expiry: 24-hour sliding window (renewed on each authenticated request)
- [ ] Session invalidation: explicit logout deletes session from DB
- [ ] Expired sessions cleaned up periodically (background task or on-access)
- [ ] No session data in URL parameters (cookies and `Authorization` header only)

---

## 3. Input Validation & Injection Prevention

### 3.1 SQL Injection
- [ ] **All** SQL queries use `rusqlite` parameterized statements (`?1`, `?2`, `:named`)
- [ ] **Zero** string concatenation or interpolation in SQL queries — verified by code review and `test_security.rs`
- [ ] `LIKE` queries use parameterized values with escaped wildcards
- [ ] No dynamic table or column names from user input

### 3.2 Cross-Site Scripting (XSS)
- [ ] Note content HTML-entity-escaped before embedding in HTML responses
- [ ] Markdown → HTML rendering via `pulldown-cmark` with output sanitization:
  - Strip `<script>`, `<iframe>`, `<object>`, `<embed>`, `<form>` tags
  - Strip `javascript:` URI schemes
  - Strip `on*` event handler attributes
- [ ] Frontend: use `textContent` (not `innerHTML`) for user-generated content unless rendering sanitized Markdown
- [ ] Content-Security-Policy header blocks inline scripts

### 3.3 Request Validation
- [ ] Request body size limit: 1MB max (axum `DefaultBodyLimit`)
- [ ] JSON deserialization with strict serde types (no `serde_json::Value` for user input)
- [ ] UUID parameters validated (reject non-UUID path params)
- [ ] Pagination parameters bounded (`limit` max 100, `page` min 1)
- [ ] Cron expressions validated against safe subset (reject expressions that would fire more than once per minute)
- [ ] `remind_at` must be a valid future UTC datetime
- [ ] `color` field validated as hex color (`#[0-9a-fA-F]{6}`)

### 3.4 Path Traversal
- [ ] Markdown export filenames sanitized (alphanumeric + hyphens only, no `..` or `/`)
- [ ] No user-controlled file paths reach the filesystem

---

## 4. API Security

- [ ] All `/api/*` endpoints (except `/api/auth/*`) require valid session (enforced by middleware)
- [ ] Auth middleware extracts session from cookie or `Authorization: Bearer <token>` header
- [ ] CORS configuration:
  - Default: same-origin only
  - Configurable via `CORS_ORIGINS` env var (comma-separated list)
  - Never `*` in production
- [ ] CSRF protection via `SameSite=Strict` cookie (form submissions from other origins blocked)
- [ ] Rate limiting on mutation endpoints (create/update/delete): 30 requests/min/IP
- [ ] Rate limiting on auth endpoints: 2 requests/min/IP
- [ ] No sensitive data in URL query parameters (tokens, passwords, OTPs)
- [ ] Response headers:
  - `X-Content-Type-Options: nosniff`
  - `X-Frame-Options: DENY`
  - `X-XSS-Protection: 0` (rely on CSP instead)
  - `Referrer-Policy: strict-origin-when-cross-origin`
  - `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' ws: wss:; frame-ancestors 'none'`

---

## 5. Container Security

- [ ] Multi-stage Docker build:
  - Stage 1 (`builder`): Rust compiler, builds release binary
  - Stage 2 (`runtime`): Debian slim, copies only the binary + static assets
  - No compiler, build tools, or source code in runtime image
- [ ] Non-root user:
  - `RUN useradd -m -s /bin/bash evanotes`
  - `USER evanotes`
  - Data directories owned by `evanotes` user
- [ ] No secrets baked into image:
  - All sensitive config via environment variables at runtime
  - No `.env` file in image
- [ ] Health check:
  - `HEALTHCHECK CMD curl -f http://localhost:8080/api/auth/status || exit 1`
- [ ] `.dockerignore` excludes: `target/`, `.git/`, `tests/`, `.env`, `AGENTS.md`
- [ ] Pin base image versions (e.g., `rust:1.82-slim-bookworm`, not `rust:latest`)
- [ ] Run `docker scan` or Trivy for image vulnerability scanning

---

## 6. Data Security

- [ ] SQLite WAL mode enabled for crash safety (`PRAGMA journal_mode=WAL`)
- [ ] Foreign keys enforced (`PRAGMA foreign_keys=ON`)
- [ ] Soft-delete for notes (data recovery possible; `is_deleted` flag)
- [ ] Session tokens **never** logged (even at debug/trace level)
- [ ] OTP codes logged **only** when SMTP is unconfigured (development convenience)
- [ ] Database file permissions: readable/writable only by the `evanotes` user (in Docker)
- [ ] `.env` file in `.gitignore` — never committed to version control
- [ ] `SESSION_SECRET` never committed — generated at runtime or provided via env
- [ ] Backup strategy documented (SQLite file copy while WAL checkpoint is clean)

---

## 7. Automated Security Testing (`test_security.rs`)

### 7.1 SQL Injection Tests
- [ ] Inject `'; DROP TABLE notes; --` in note title → verify no SQL execution
- [ ] Inject `" OR 1=1 --` in search query → verify no data leak
- [ ] Inject SQL in all string input fields (title, content, description, email)

### 7.2 XSS Tests
- [ ] Store `<script>alert('xss')</script>` in note content → verify escaped in response
- [ ] Store `<img onerror="alert('xss')" src=x>` → verify `onerror` stripped
- [ ] Store `[Click](javascript:alert('xss'))` in Markdown → verify `javascript:` blocked
- [ ] Verify CSP header present on all HTML responses

### 7.3 Authentication Bypass Tests
- [ ] Request `/api/notes` without session cookie → `401`
- [ ] Request with expired session token → `401`
- [ ] Request with forged/modified HMAC token → `401`
- [ ] Request with valid token for non-existent user → `401`
- [ ] Verify Google OAuth rejects tokens with wrong `aud` claim

### 7.4 Authorization (IDOR) Tests
- [ ] User A creates note → User B requests `/api/notes/{A's note ID}` → `404`
- [ ] User A creates reminder → User B requests `/api/reminders/{A's reminder ID}` → `404`
- [ ] Verify all CRUD operations scoped to authenticated user's data only

### 7.5 Rate Limiting Tests
- [ ] Send 3 OTP requests in 1 minute → verify 3rd returns `429`
- [ ] Send 6 OTP verification attempts → verify lockout after 5th
- [ ] Verify rate limit headers in response (`Retry-After`)

### 7.6 Input Validation Tests
- [ ] Note content > 1MB → `413`
- [ ] Invalid UUID in path → `400`
- [ ] Invalid cron expression → `400`
- [ ] `remind_at` in the past → `400`
- [ ] Negative pagination values → `400`

---

## 8. Pre-Merge Checklist (Every Phase)

Run these checks before every merge to `main`:

```bash
# 1. Format check
cargo fmt --check

# 2. Lint (pedantic)
cargo clippy -- -D warnings

# 3. All tests pass
cargo test

# 4. Security audit
cargo audit

# 5. License check (if cargo-deny installed)
cargo deny check

# 6. Review git diff for sensitive data
git diff --cached | grep -i "secret\|password\|token\|api_key"
```
