# evaNotes — API Reference

> REST + WebSocket API for notes, tags, reminders, and authentication.

**Base URL**: `http://localhost:8080`

All `/api/*` endpoints except the auth endpoints require a valid session.
Sessions are delivered as `Set-Cookie: session=…` (HttpOnly, SameSite=Strict) and may
alternatively be sent as `Authorization: Bearer <token>`.

**Error body shape** (non-2xx):

```json
{ "error": "unauthorized|bad_request|rate_limited|not_found|forbidden|conflict|internal_error", "message": "…" }
```

`429` responses include a `Retry-After` header.

---

## 1. Authentication

### `GET /api/auth/status`

Public. Reports auth + feature availability.

```json
{
  "status": "ok",
  "app": "evaNotes",
  "version": "0.1.0",
  "authenticated": true,
  "email": "you@example.com",
  "is_admin": true,
  "user_id": "uuid",
  "google_login_enabled": false,
  "google_client_id": null,
  "telegram_enabled": false,
  "email_enabled": false
}
```

### `GET /api/auth/me` — 🔒

```json
{ "status": "ok", "user_id": "uuid", "email": "you@example.com", "is_admin": true }
```

### `POST /api/auth/otp/request`

```json
{ "email": "you@example.com" }
```
→ `200 {"status":"otp_sent","message":"A 6-digit verification code has been dispatched."}`

- `401` if `ADMIN_EMAIL` is set and the email does not match.
- `429` after `OTP_RATE_LIMIT_MAX` (default 5) requests per IP per 60 s, with a `Retry-After` header.
- If SMTP is unconfigured the code is printed to the server log:
  `[evaNotes AUTH] Login OTP for you@example.com: 123456`

### `POST /api/auth/otp/verify`

```json
{ "email": "you@example.com", "code": "123456" }
```
→ `200` with session cookie set:

```json
{
  "status": "authenticated",
  "email": "you@example.com",
  "auth_method": "otp",
  "is_admin": true,
  "user_id": "uuid",
  "token": "…"
}
```

Codes expire after 5 minutes, are single-use, and lock after 5 wrong attempts.

### `POST /api/auth/google`

```json
{ "id_token": "…" }
```
→ `200`, same response shape as OTP verify (flat object + `Set-Cookie`).

### `POST /api/auth/logout`

→ `200 {"status":"logged_out"}` + cookie invalidation.

---

## 2. Notes

### Note object

```json
{
  "id": "uuid",
  "user_id": "uuid",
  "title": "K8s cheatsheet",
  "content": "# kubectl\nkubectl get pods",
  "note_type": "simple | detailed | code | sheet",
  "is_pinned": false,
  "is_archived": false,
  "is_deleted": false,
  "color": "#6366f1",
  "icon": "🚀",
  "folder_id": null,
  "folder_name": "Work",
  "tags": ["kubernetes", "devops"],
  "created_at": "2026-10-23 19:37:06",
  "updated_at": "2026-10-23 19:37:06",
  "deleted_at": null,
  "reminder_count": 1
}
```

### `GET /api/notes` — 🔒

Query params:

| Param | Type | Meaning |
|:--|:--|:--|
| `page`, `limit` | int | pagination (default 1 / 20, max 100) |
| `search` | string | search query (all scopes by default) |
| `search_in` | `title,content,tags,folders` (comma list) | limit which parts the search matches |
| `type` | `simple\|detailed\|code\|sheet` | note type filter |
| `archived` | bool | show archived only |
| `pinned` | bool | pinned only |
| `tag` | `a,b` (comma list) | notes carrying ALL these tags |
| `folder` | folder id \| `root` | notes in this folder **and its subfolders** (`root` = no folder) |

```json
{ "notes": [ …Note… ], "pagination": { "page": 1, "limit": 20, "total": 3, "total_pages": 1 } }
```

### `POST /api/notes` — 🔒

```json
{ "title": "…", "content": "…", "note_type": "detailed", "color": "#6366f1",
  "icon": "🚀", "folder_id": "…", "tags": ["kubernetes"] }
```
→ `201` with the created Note (tags auto-created when unknown). Content limit: 1 MB.

### `GET /api/notes/{id}?device=dev-id` — 🔒

```json
{
  "note": { …Note… },
  "content_html": "…",
  "active_editors": [ { "device": "phone-a", "since": "12s ago" } ]
}
```
`content_html` is server-rendered (Markdown / highlighted code), XSS-sanitized.
`active_editors` lists other devices that currently have the note open.

### `PUT /api/notes/{id}` — 🔒

Partial update: `title, content, note_type, color, icon, is_pinned, is_archived, tags`.
`folder_id` is tri-state: absent = unchanged, `null` = move to top level, id = move into folder.
Every update snapshots the previous state into the version history (throttled).

### `DELETE /api/notes/{id}` — 🔒

Soft delete → `200 {"status":"deleted","id":"…"}`.

### `PUT /api/notes/{id}/pin` / `PUT /api/notes/{id}/archive` — 🔒

Toggle → `{"id":"…","is_pinned":true}` / `{"id":"…","is_archived":true}`.

### `PUT /api/notes/{id}/tags` — 🔒

```json
{ "tags": ["kubernetes", "infra"] }
```
→ `200 {"id":"…","tags":["kubernetes","infra"]}`. Replaces the tag set; unknown tags are auto-created.

### `GET /api/notes/{id}/export?format=md|pdf` — 🔒

- `md` (default): `text/markdown` attachment
- `pdf`: `application/pdf` attachment

---

## 2b. Versions, presence, sharing

### `GET /api/notes/{id}/versions` — 🔒

`{"versions":[{"id":1,"note_id":"…","title":"…","content":"…","note_type":"…","created_at":"…"}]}`
Newest first. Automatic snapshots, at least `VERSION_MIN_INTERVAL_SECS` apart, max `VERSION_MAX_KEEP`.

### `POST /api/notes/{id}/versions/clear` — 🔒

→ `{"status":"cleared","deleted":N}`.

### `POST /api/notes/{id}/restore` — 🔒

```json
{ "version_id": 3 }
```
→ `{"status":"restored","note":{…}}` (the pre-restore state is itself snapshotted first).

### `POST /api/notes/{id}/presence` — 🔒 / `DELETE` — 🔒

```json
{ "device": "phone-a" }
```
POST = heartbeat (keep-alive every ~30s), returns `{"status":"ok","active_editors":[…]}`
for *other* devices. DELETE = device closed the note. Stale entries (75s) are pruned.

### `GET/POST/DELETE /api/notes/{id}/share` — 🔒

- GET → `{"share": {"id":"slug","mode":"live|snapshot","path":"/share/slug", …} | null}`
- POST `{ "mode": "snapshot" | "live" }` → creates/updates the note's share link
- DELETE → revokes the link

### Public share pages (no auth)

| Route | Returns |
|:--|:--|
| `GET /share/{slug}` | self-contained HTML page (rendered note + download buttons) |
| `GET /share/{slug}/md` | Markdown download |
| `GET /share/{slug}/pdf` | PDF download |

Snapshot shares serve a frozen copy; live shares always render the note's current state.

### `POST /api/highlight` — 🔒

```json
{ "language": "rust", "code": "fn main() {}" }
```
→ `{"html":"<pre …>…"}` (syntax-highlighted HTML for live code preview).

---

## 3. Folders

### Folder object

```json
{ "id": "uuid", "user_id": "uuid", "name": "Work", "parent_id": null,
  "icon": "💼", "created_at": "…", "updated_at": "…",
  "note_count": 3, "children": [ …Folder… ] }
```

### `GET /api/folders` — 🔒

→ `{"folders":[ …tree… ]}` (nested via `children`).

### `POST /api/folders` — 🔒

```json
{ "name": "Work", "parent_id": null, "icon": "💼" }
```
→ `201` Folder.

### `PUT /api/folders/{id}` — 🔒

```json
{ "name": "…", "parent_id": null, "icon": "…" }
```
`parent_id` tri-state like notes (absent / null / id). Cycle moves are rejected with `400`.
`409`? No — `400` for invalid parent, `404` unknown.

### `DELETE /api/folders/{id}` — 🔒

→ `{"status":"deleted","id":"…"}`. Subfolders move up one level; the folder's notes move to the top level.

---

## 4. Tags

### Tag object

```json
{ "id": 1, "user_id": "uuid", "name": "kubernetes", "color": null, "note_count": 4 }
```

### `GET /api/tags` — 🔒

`{"tags":[ …Tag… ]}` — all tags with note counts, sorted by name.

### `GET /api/tags/suggest?q=ku&limit=12` — 🔒

Fast indexed prefix autocomplete. Empty `q` returns the most-used tags.

### `POST /api/tags` — 🔒

```json
{ "name": "infra", "color": "#10b981" }
```
→ `201` Tag. Creating an existing name (case-insensitive) returns the existing tag.

### `PUT /api/tags/{id}` — 🔒

```json
{ "name": "k8s", "color": "#6366f1" }
```
→ updated Tag. `409` on name collision.

### `DELETE /api/tags/{id}` — 🔒

→ `200 {"status":"deleted","id":1}`. Detaches the tag from all notes (cascade).

---

## 4. Reminders

### Reminder object

```json
{
  "id": "uuid",
  "user_id": "uuid",
  "note_id": null,
  "title": "Standup",
  "description": null,
  "remind_at": "2026-10-24T09:00:00Z",
  "is_recurring": true,
  "cron_expr": "0 9 * * 1-5",
  "recur_type": "daily",
  "state": "pending | triggered | snoozed | acknowledged | dismissed",
  "snooze_until": null,
  "channels": ["web", "telegram"],
  "created_at": "…",
  "updated_at": "…",
  "last_triggered": null
}
```

`remind_at` is always canonical UTC (`YYYY-MM-DDTHH:MM:SSZ`). Accepted inputs on
create/update: RFC 3339 (any offset/millis) or `datetime-local` (server-local time).

### `GET /api/reminders?state=&note_id=&upcoming=&page=&limit=` — 🔒

→ `{"reminders":[ … ], "pagination": {…}}` ordered by `remind_at` ascending.

### `POST /api/reminders` — 🔒

```json
{
  "title": "Standup",
  "description": null,
  "remind_at": "2026-10-24T09:00:00Z",
  "is_recurring": true,
  "cron_expr": "0 9 * * 1-5",
  "recur_type": "daily",
  "note_id": null,
  "channels": ["web", "email", "telegram"]
}
```

- `recur_type`: `daily | weekly | monthly | custom` (presets inject a default cron).
- `channels` default `["web"]`. `email` needs SMTP env vars; `telegram` needs
  `TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID`. Unconfigured channels are skipped with a
  server-side warning at trigger time (the reminder still triggers).

### `GET /api/reminders/{id}` — 🔒 · `PUT /api/reminders/{id}` — 🔒 (partial)

### `DELETE /api/reminders/{id}` — 🔒

### `POST /api/reminders/{id}/snooze` — 🔒

```json
{ "duration_minutes": 15 }
```
→ reminder with `state:"snoozed"` and `remind_at` shifted forward.

### `POST /api/reminders/{id}/acknowledge` — 🔒

```json
{ "action": "acknowledge" }
```
`action` = `acknowledge` (done) or `dismiss`.

---

## 6. AI Assistant

### `POST /api/ai/rewrite` — 🔒

```json
{ "text": "…", "model": "nvidia/nemotron-3.5-super-vl:free", "style": "formal" }
```
→ `200 {"rewritten":"…","model":"…","style":"…"}`.

- Requires `AI_API_KEY` + `AI_MODELS` (otherwise `400`).
- `model` must be in the configured allowlist.
- `style`: `minimal | formal | casual | concise | professional | friendly`.
- Uses an OpenAI-compatible `/chat/completions` endpoint (`AI_API_BASE`).
- The frontend also learns availability, models and styles from `GET /api/auth/status`
  (`ai_enabled`, `ai_models`, `ai_default_model`, `ai_styles`).

---

## 7. WebSocket — `GET /ws`

Requires a session (cookie or `?token=`). Server pushes:

```json
{
  "type": "reminder_triggered",
  "data": { …Reminder… }
}
```

The client shows an in-app toast + desktop notification (and the reminder engine
simultaneously delivers the configured email/Telegram channels).
