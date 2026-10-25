# evaNotes 📝

> A secure, self-hosted Notes & Reminders application built with **Rust**, featuring Google OAuth, SQLite persistence, Markdown editing, **full-text search**, **tags**, **folders**, **public sharing**, **automatic version history**, an **AI rewrite assistant**, recurring reminders with **email & Telegram delivery**, a **calendar view**, and a modern responsive dark-mode Web UI.

---

## Features

### Notes
- ✏️ **Quick Capture**: type a title and hit Enter — a note is saved instantly (Ctrl+Enter opens the Markdown editor)
- 📄 **Four note types**: Simple text, Markdown (with live split-pane preview), **Code** (35+ languages with syntax highlighting), and **Sheet** (paste a table from Excel/Google Sheets and edit it as a grid)
- 📁 **Folders**: unlimited nesting (subfolders inside folders), move notes between folders, notes without a folder stay at the top level
- 🏷 **Symbols**: assign any of 150+ icons/emoji to notes and folders
- 🕸 **Diagrams**: add flowchart / decision / arrow diagrams inside Markdown (mermaid)
- ⬇ **Export**: Markdown **and PDF** downloads for every note

### Search & Organisation
- 🔍 **Full-Text Search** (SQLite FTS5): indexes everything you write — title, content, tags, and folder names (your whole library, effectively no size limit)
- 🎛 **Search scopes**: one search box; choose what to include (Title / Content / Tags / Folders) — all selected by default
- 🏷️ **Tags**: multiple tags per note, autocomplete suggestions, auto-suggested tags from `#hashtags` in your content, rename/delete, multi-tag AND filtering

### Collaboration & Safety
- 🔗 **Public sharing**: every note can have a public link — either **"always updated"** (live) or **"current version"** (frozen snapshot). Visitors get a clean page with PDF/Markdown download buttons; revoke anytime
- 🕘 **Automatic versions**: notes auto-save, and previous states are snapshotted (at least 15 min apart, last 10 kept per note) with one-click restore and a "clear all versions" button
- 📱 **Multi-device awareness**: opening a note that's already open elsewhere shows a warning banner (via heartbeat presence tracking)

### Reminders & Calendar
- ⏰ One-time and recurring reminders (daily / weekly / monthly / custom cron), snooze, acknowledge/dismiss, attach to notes
- 📡 **Delivery channels**: real-time WebSocket + browser notifications, **email** (SMTP), **Telegram** (bot API)
- 📅 **Calendar view**: month / week / day, with reminders pinned at their actual times

### AI Assistant (optional)
- ✨ Rewrite notes with AI: minimal changes, formal, casual, concise, professional, friendly styles
- 🔌 Uses any OpenAI-compatible API (OpenRouter by default) configured via `.env` — disabled in the UI until configured
- 🎛 Model dropdown in the top-right (default `nvidia/nemotron-3.5-super-vl:free`, resets each session)

### Security & Platform
- 🔐 Google OAuth 2.0 + passwordless email OTP, HMAC-SHA256 session cookies, rate limiting, XSS-sanitized Markdown
- 📦 Single binary + SQLite, Docker-ready

---

## Quickstart with Docker 🐳

```yaml
services:
  evanotes:
    image: akashkece/evanotes:0.1.1
    container_name: evanotes
    restart: unless-stopped
    ports:
      - "8080:8080"
    volumes:
      - ./data:/data
    environment:
      - ADMIN_EMAIL=
      - GOOGLE_CLIENT_ID=
      - SECURE_COOKIES=false
      - ENFORCE_HTTPS=false
      # Email reminders (optional)
      - SMTP_HOST=
      - SMTP_PORT=587
      - SMTP_USER=
      - SMTP_PASSWORD=
      - SMTP_FROM=
      # Telegram reminders (optional)
      - TELEGRAM_BOT_TOKEN=
      - TELEGRAM_CHAT_ID=
      # AI rewrite (optional — any OpenAI-compatible API)
      - AI_API_BASE=https://openrouter.ai/api/v1
      - AI_API_KEY=
      - AI_MODELS=openai/gpt-6-luna,z-ai/glm-5.3-flash,nvidia/nemotron-3.5-super-vl:free,openai/gpt-5.6-luna
      - AI_DEFAULT_MODEL=nvidia/nemotron-3.5-super-vl:free
```

```bash
docker compose up -d
```

Open **[http://localhost:8080](http://localhost:8080)**.

> Published images are **multi-architecture** (`linux/amd64` + `linux/arm64`), so the same tag runs on x86 servers and ARM hosts (e.g. Oracle Cloud Ampere).

If SMTP is not configured, view your login OTP via:
```bash
docker compose logs -f evanotes
```

---

## Local Development

### Requirements
- Rust (stable toolchain, 1.82+)

### Build & Run

```bash
git clone https://github.com/akash211/evaNotes.git
cd evaNotes

cargo build --release
./target/release/evanotes
```

Open **[http://127.0.0.1:8080](http://127.0.0.1:8080)**.

### Running Tests

```bash
cargo test
```

---

## 💾 Backups — yes, one file is everything

All data (users, notes, tags, folders, reminders, versions, shares) lives in a single SQLite database:

- **Docker**: `./data/evanotes.db` (the mounted volume)
- **Local**: `~/.evanotes/evanotes.db` (or `$EVA_DATA_DIR/evanotes.db`)

**Copying that one file is a complete backup** — restore it into the same location and everything comes back exactly as it was.

Two practical notes:
1. evaNotes runs SQLite in **WAL mode**, so while the server is running there may also be `evanotes.db-wal` and `evanotes.db-shm` files next to the database. For a guaranteed-consistent backup, either stop the server first, or copy all three files together. (After a clean shutdown the `-wal`/`-shm` files merge back into the main file.)
2. The database uses **append-only migrations**, so backups made with any version of evaNotes open fine on newer versions (never on older ones).

---

## Documentation

| Document | Description |
|:---|:---|
| [Master Plan](docs/MASTER_PLAN.md) | Development phases & roadmap |
| [Architecture](docs/ARCHITECTURE.md) | Component structure, data flow diagrams |
| [Database Schema](docs/DATABASE_SCHEMA.md) | Full SQLite schema documentation |
| [API Reference](docs/API_REFERENCE.md) | REST + WebSocket API docs |
| [Security Checklist](docs/SECURITY_CHECKLIST.md) | 7-section security audit checklist |
| [CI/CD Pipeline](docs/CI_CD_PIPELINE.md) | GitHub Actions workflow & release process |

---

## Environment Variables

| Variable | Default | Description |
|:---|:---|:---|
| `HOST` | `0.0.0.0` | Server bind address |
| `PORT` | `8080` | Server port |
| `RUST_LOG` | `info` | Log level |
| `EVA_DATA_DIR` | `/data` (Docker) / `~/.evanotes` (local) | Database directory |
| `ADMIN_EMAIL` | — | Authorized admin email |
| `SESSION_SECRET` | (auto-generated) | HMAC session signing key |
| `GOOGLE_CLIENT_ID` | — | Google OAuth 2.0 Client ID |
| `SECURE_COOKIES` | `false` | Enable `Secure` cookie flag |
| `ENFORCE_HTTPS` | `false` | Redirect HTTP → HTTPS |
| `CORS_ORIGINS` | — | Allowed CORS origins (comma-separated, default: same-origin) |
| `OTP_RATE_LIMIT_MAX` | `5` | Max OTP requests per IP per minute |
| `SMTP_HOST` | — | SMTP server hostname (enables email reminders) |
| `SMTP_PORT` | `587` | SMTP server port |
| `SMTP_USER` | — | SMTP username |
| `SMTP_PASSWORD` | — | SMTP password |
| `SMTP_FROM` | — | SMTP sender address |
| `TELEGRAM_BOT_TOKEN` | — | Telegram bot token (enables Telegram reminders) |
| `TELEGRAM_CHAT_ID` | — | Telegram chat ID to deliver reminders to |
| `AI_API_BASE` | `https://openrouter.ai/api/v1` | OpenAI-compatible API endpoint |
| `AI_API_KEY` | — | API key — empty = AI feature hidden in the UI |
| `AI_MODELS` | (see example) | Comma-separated model allowlist for the dropdown |
| `AI_DEFAULT_MODEL` | `nvidia/nemotron-3.5-super-vl:free` | Default model at the start of each session |
| `VERSION_MIN_INTERVAL_SECS` | `900` | Min seconds between automatic version snapshots |
| `VERSION_MAX_KEEP` | `10` | Max versions kept per note |

---

## License

MIT License.
