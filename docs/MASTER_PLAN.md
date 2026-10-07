# evaNotes 📝 — Master Development Plan

> A secure, high-performance Notes & Reminder application built with **Rust**, featuring Google OAuth, SQLite persistence, Markdown support, recurring reminders, and a modern Web UI — containerized and published to Docker Hub.

---

## 1. Project Vision & Goals

| Goal | Description |
|:---|:---|
| **Core Product** | A self-hosted notes & reminder app with a modern glassmorphism Web UI |
| **Language** | Rust (stable toolchain, 2024 edition) |
| **Database** | SQLite via `rusqlite` (with WAL mode for concurrent reads) |
| **Auth** | Google OAuth 2.0 (GIS) + Email OTP fallback |
| **Deployment** | Multi-stage Docker image → Docker Hub (`akashkece/evanotes`) |
| **CI/CD** | GitHub Actions: Test → Lint → Security Audit → Build → Docker Push |
| **Conventions** | Mirror evaTorrent's patterns: `.env` config, `AGENTS.md`, branch workflow, semver tags |

---

## 2. Feature Breakdown

### 2.1 Note Types

| Type | Storage | Description |
|:---|:---|:---|
| **Simple Note** | SQLite `notes` table (`content TEXT`) | Plain text note — quick capture, title + body |
| **Detailed Note** | SQLite `notes` table (`content TEXT` as Markdown) + optional `.md` file export | Full Markdown with headings, lists, code blocks, links. Rendered in the Web UI via a Markdown → HTML renderer |

### 2.2 Reminders

| Feature | Description |
|:---|:---|
| **One-Time Reminder** | Date + time, triggers a notification (WebSocket push + optional email) |
| **Recurring Reminder** | Cron-like schedule: daily, weekly, monthly, custom interval |
| **Snooze** | Snooze a triggered reminder for 5m / 15m / 1h / custom |
| **Reminder States** | `pending` → `triggered` → `acknowledged` / `snoozed` / `dismissed` |
| **Attach to Note** | Any note (simple or detailed) can have 0..N reminders attached |

### 2.3 Authentication & Authorization

| Method | Flow |
|:---|:---|
| **Google OAuth 2.0** | Google Identity Services (GIS) one-click login → verify ID token via Google's tokeninfo endpoint → create HMAC-SHA256 session |
| **Email OTP** | 6-digit cryptographic code sent via SMTP (or logged to stdout/Docker logs if SMTP unconfigured) |
| **Session Management** | HMAC-SHA256 signed tokens in HTTP-only, Secure, SameSite cookies |
| **Admin Model** | Single-admin (configured via `ADMIN_EMAIL` env var or first-launch setup wizard) |

### 2.4 Web UI

| Element | Description |
|:---|:---|
| **Design Language** | Dark mode, glassmorphism, smooth micro-animations (consistent with evaTorrent aesthetic) |
| **Tabs/Views** | Notes List · Note Editor · Reminders · Settings |
| **Note Editor** | Split-pane Markdown editor with live preview for detailed notes; simple textarea for quick notes |
| **Reminder Dashboard** | Calendar/timeline view of upcoming reminders with status badges |
| **Real-time Updates** | WebSocket-driven live reminder notifications |
| **Responsive** | Mobile-first responsive layout |

---

## 3. Technology Stack

| Layer | Technology | Rationale |
|:---|:---|:---|
| **Language** | Rust (stable, 2024 edition) | Memory safety, performance, security |
| **Web Framework** | `axum` (v0.8+) | Tokio-based, modular, excellent ecosystem |
| **Async Runtime** | `tokio` (multi-threaded) | Industry standard for async Rust |
| **Database** | SQLite via `rusqlite` + `r2d2` pool | Zero-config, file-based, perfect for self-hosted |
| **Migrations** | `refinery` | SQL migration files, versioned schema |
| **Auth - Google** | `reqwest` + Google tokeninfo endpoint | Lightweight, no heavy SDK dependency |
| **Auth - OTP** | `rand` (cryptographic) + `lettre` (SMTP) | Secure random OTP + async email delivery |
| **Auth - Sessions** | `hmac` + `sha2` crates | HMAC-SHA256 signed session tokens |
| **Markdown** | `pulldown-cmark` | CommonMark-compliant, fast, pure Rust |
| **Scheduling** | `tokio-cron-scheduler` | Cron-based recurring reminder engine |
| **Serialization** | `serde` + `serde_json` | Standard Rust serialization |
| **Config** | `dotenvy` | `.env` file loading (mirrors evaTorrent pattern) |
| **Logging** | `tracing` + `tracing-subscriber` | Structured, async-aware logging |
| **Security Audit** | `cargo-audit` | CVE database checks on all dependencies |
| **Linting** | `clippy` (pedantic) | Catch common mistakes and enforce idioms |
| **Formatting** | `rustfmt` | Consistent code style |
| **Testing** | Built-in `#[cfg(test)]` + `tokio::test` | Unit + integration tests |
| **Frontend** | Vanilla HTML/CSS/JS | Consistent with evaTorrent (no framework bloat) |
| **Container** | Multi-stage Dockerfile (Rust builder + Debian slim runtime) | Small image, non-root user |

---

## 4. Development Phases & Git Workflow

### Git Branch Strategy (mirrors evaTorrent)

```
main ← (protected, merge only after tests pass)
  └── feat/phase-N-description ← (feature branches, one per phase)
```

- **Every phase** gets its own branch: `feat/phase-1-project-skeleton`, `feat/phase-2-database`, etc.
- Commits are atomic and well-described.
- Merge to `main` only after all tests for that phase pass.
- After final merge, create a semver tag (`v0.1.0`) and push to Docker Hub.

---

### Phase 1: Project Skeleton & Tooling
**Branch**: `feat/phase-1-project-skeleton`

| Task | Details |
|:---|:---|
| Initialize Cargo project | `cargo init --name evanotes` |
| Set up `Cargo.toml` | Add all dependencies (axum, tokio, rusqlite, serde, etc.) |
| Create directory structure | `src/`, `migrations/`, `tests/`, `src/web/static/` |
| Create `.gitignore` | Rust targets, IDE, OS, `.env`, `AGENTS.md` |
| Create `.env.example` | Template with all config vars |
| Create `config.rs` | Env var loading with defaults |
| Create `main.rs` | Basic axum server with health check endpoint |
| Create `error.rs` | Unified error handling |
| Set up `clippy.toml` | Pedantic lint configuration |
| Set up `rustfmt.toml` | Formatting rules |
| **Tests** | Server boots, health check returns 200 |
| **Security** | `cargo audit`, `cargo clippy -- -D warnings` |
| **Merge** | PR → `main` after tests pass |

---

### Phase 2: Database Layer
**Branch**: `feat/phase-2-database`

| Task | Details |
|:---|:---|
| Add SQLite setup | Connection pool (`r2d2`), WAL mode, foreign keys enabled |
| Create migration files | `V001__initial_schema.sql`, `V002__add_indexes.sql` |
| Implement `db/migrations.rs` | Auto-run migrations on startup |
| Implement `db/notes.rs` | CRUD operations with parameterized queries |
| Implement `db/reminders.rs` | CRUD + state transitions, recurring logic |
| Implement `db/users.rs` | User + session management |
| Create model structs | `Note`, `Reminder`, `User`, `Session` with serde derives |
| **Tests** | Full CRUD for notes, reminders, users; migration idempotency |
| **Security** | SQL injection test suite; parameterized query verification |
| **Merge** | PR → `main` |

---

### Phase 3: Authentication System
**Branch**: `feat/phase-3-authentication`

| Task | Details |
|:---|:---|
| Implement `auth/google.rs` | Google ID token verification via tokeninfo endpoint |
| Implement `auth/otp.rs` | OTP generation, SMTP delivery (lettre), rate limiting |
| Implement `auth/session.rs` | HMAC-SHA256 token creation, verification, expiry |
| Implement `auth/middleware.rs` | Axum extractor for authenticated requests |
| Implement auth handlers | `/api/auth/*` endpoints |
| Add rate limiting | In-memory sliding window per IP |
| **Tests** | Full auth flow; rate limit enforcement; token expiry; forged token rejection |
| **Security** | OTP brute-force lockout; session fixation prevention; timing-safe comparison |
| **Merge** | PR → `main` |

---

### Phase 4: Notes API & Service Layer
**Branch**: `feat/phase-4-notes-api`

| Task | Details |
|:---|:---|
| Implement notes handlers | CRUD endpoints with auth middleware |
| Implement `services/markdown.rs` | pulldown-cmark rendering with HTML sanitization |
| Add pagination & search | Query params: `page`, `limit`, `search`, `type`, `archived` |
| Add note pinning & archiving | Toggle endpoints |
| Add Markdown export | `/api/notes/:id/export` → `.md` file download |
| **Tests** | Full CRUD; pagination; search; Markdown rendering; auth enforcement |
| **Security** | XSS in Markdown content; IDOR checks; body size limits |
| **Merge** | PR → `main` |

---

### Phase 5: Reminders Engine
**Branch**: `feat/phase-5-reminders`

| Task | Details |
|:---|:---|
| Implement reminders handlers | CRUD + snooze/acknowledge endpoints |
| Implement `services/reminder_engine.rs` | Tokio background task polling `reminders` table every 30s |
| Add recurring reminder logic | Parse cron expressions, compute next trigger time |
| Implement `services/notification.rs` | WebSocket broadcast on trigger + optional email |
| Add note ↔ reminder linking | Associate reminders with notes |
| **Tests** | One-time trigger; recurring trigger; snooze; acknowledge; state machine |
| **Security** | Validate cron expressions (reject malicious patterns); IDOR on reminders |
| **Merge** | PR → `main` |

---

### Phase 6: Web UI
**Branch**: `feat/phase-6-web-ui`

| Task | Details |
|:---|:---|
| Create `index.html` | SPA shell with login screen + main app tabs |
| Create `style.css` | Dark mode design system, glassmorphism panels, animations |
| Create `app.js` | Frontend controller: auth flow, API client, note editor, reminder UI |
| Login screen | Google Sign-In button + OTP email form |
| Notes list view | Card grid with type badges, pin/archive actions, search bar |
| Note editor | Simple textarea (simple) / split-pane Markdown editor (detailed) |
| Reminder dashboard | Timeline/list with status badges, create/edit modals |
| WebSocket integration | Live reminder toast notifications |
| **Tests** | Manual UI testing checklist + API-level integration tests |
| **Security** | Content Security Policy headers; XSS prevention in DOM manipulation |
| **Merge** | PR → `main` |

---

### Phase 7: Security Audit & Hardening
**Branch**: `feat/phase-7-security-audit`

| Task | Details |
|:---|:---|
| Run full security checklist | Complete `SECURITY_CHECKLIST.md` |
| Implement `test_security.rs` | Comprehensive security test suite |
| Add CSP headers | `Content-Security-Policy` in all HTML responses |
| Add CORS configuration | Configurable allowed origins |
| Add request body limits | axum `DefaultBodyLimit` |
| Run `cargo audit` | Zero vulnerabilities |
| Run `cargo deny` | License and advisory checks |
| Review all SQL queries | Verify zero string interpolation |
| Review all HTML output | Verify proper escaping |
| **Merge** | PR → `main` |

---

### Phase 8: Docker & CI/CD
**Branch**: `feat/phase-8-docker-cicd`

| Task | Details |
|:---|:---|
| Create `Dockerfile` | Multi-stage: Rust builder → Debian slim runtime, non-root user |
| Create `docker-compose.yml` | Volume mappings, env vars, health check |
| Create `.dockerignore` | Exclude `target/`, `.git/`, `tests/` |
| Create `.github/workflows/ci.yml` | Test → Clippy → Audit → Build → Docker Push |
| Local Docker testing | Build & run, verify all features work in container |
| **Merge** | PR → `main` |

---

### Phase 9: Release
**Branch**: (direct on `main` after all merges)

| Task | Details |
|:---|:---|
| Final integration testing | Full feature test in Docker container |
| Create `AGENTS.md` | Complete architectural guide (gitignored) |
| Update `README.md` | Final documentation with quickstart, features, screenshots |
| Tag release | `git tag v0.1.0` |
| Push to Docker Hub | CI auto-pushes on tag: `akashkece/evanotes:0.1.0` + `latest` |
| Verify Docker Hub | Pull and run from Docker Hub image |

---

## 5. Success Criteria

- [ ] All 9 phases completed and merged to `main`
- [ ] Zero `cargo audit` vulnerabilities
- [ ] Zero `cargo clippy` warnings (pedantic)
- [ ] All unit + integration tests passing
- [ ] Security test suite covering OWASP Top 10 relevant items
- [ ] Docker image running successfully with all features
- [ ] Tag `v0.1.0` created and pushed
- [ ] Docker Hub image `akashkece/evanotes:0.1.0` published and verified
- [ ] `README.md` complete with quickstart documentation
- [ ] `AGENTS.md` complete for future AI/contributor guidance
