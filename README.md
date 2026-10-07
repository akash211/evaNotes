# evaNotes 📝

> A secure, self-hosted Notes & Reminder application built with **Rust**, featuring Google OAuth, SQLite persistence, Markdown editing, recurring reminders, and a modern dark-mode Web UI.

---

## Features

- ✏️ **Simple Notes**: Quick plain-text notes for fast capture — title + body, nothing more
- 📄 **Detailed Notes (Markdown)**: Full Markdown editor with live preview — headings, lists, code blocks, links, and more
- ⏰ **Reminders**: One-time and recurring reminders with real-time WebSocket notifications
  - **Recurring**: Daily, weekly, monthly, or custom cron schedules
  - **Snooze**: 5m / 15m / 1h / custom snooze on triggered reminders
  - **Attach to Notes**: Link reminders to any note for context
- 🔐 **Authentication & Security**:
  - **Google OAuth 2.0**: One-click Google Sign-In for authorized administrators
  - **Email OTP**: Passwordless 6-digit code login via SMTP (or Docker logs)
  - **HMAC-SHA256 Sessions**: Secure, HTTP-only, SameSite cookies
  - **Rate Limiting**: Brute-force protection on all auth endpoints
- 🌐 **Modern Web UI**:
  - Dark mode glassmorphism design with smooth micro-animations
  - Split-pane Markdown editor with live preview
  - Reminder dashboard with status badges and timeline view
  - Real-time WebSocket notifications for triggered reminders
  - Mobile-responsive layout
- 📦 **Self-Hosted & Docker Ready**: Single binary, SQLite database, official Docker Hub image
- 🔍 **Search & Organize**: Full-text search, pinning, archiving, color tags, and Markdown file export

---

## Quickstart with Docker 🐳

### Using Docker Compose (Recommended)

```yaml
services:
  evanotes:
    image: akashkece/evanotes:0.1.0
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
      - SMTP_HOST=
      - SMTP_PORT=587
      - SMTP_USER=
      - SMTP_PASSWORD=
      - SMTP_FROM=
```

```bash
docker compose up -d
```

Open **[http://localhost:8080](http://localhost:8080)**.

If SMTP is not configured, view your login OTP via:
```bash
docker compose logs -f evanotes
```

---

## Local Development

### Requirements
- Rust (stable toolchain, 1.82+)
- SQLite 3.x (usually pre-installed on macOS/Linux)

### Build & Run

```bash
git clone https://github.com/akash211/evaNotes.git
cd evaNotes

# Build
cargo build --release

# Run
./target/release/evanotes
```

Open **[http://127.0.0.1:8080](http://127.0.0.1:8080)**.

### Running Tests

```bash
cargo test
```

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
| `SMTP_HOST` | — | SMTP server hostname |
| `SMTP_PORT` | `587` | SMTP server port |
| `SMTP_USER` | — | SMTP username |
| `SMTP_PASSWORD` | — | SMTP password |
| `SMTP_FROM` | — | SMTP sender address |

---

## License

MIT License.
