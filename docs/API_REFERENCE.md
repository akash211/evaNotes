# evaNotes — API Reference

> REST API + WebSocket endpoint for notes, reminders, and authentication.

**Base URL**: `http://localhost:8080`

---

## 1. Authentication

All endpoints under `/api/*` (except auth endpoints) require a valid session. Sessions are managed via HTTP-only cookies set on successful login.

### Error Responses (Auth)

| Status | Body | Meaning |
|:---|:---|:---|
| `401` | `{"error": "unauthorized"}` | Missing or invalid session token |
| `429` | `{"error": "rate_limited", "retry_after": 60}` | Too many requests (OTP endpoints: 2/min/IP) |

---

### `POST /api/auth/google`

Verify a Google OAuth ID token and create a session.

**Request:**
```json
{
  "id_token": "eyJhbGciOiJSUzI1NiIs..."
}
```

**Response (200):**
```json
{
  "status": "authenticated",
  "email": "user@gmail.com",
  "auth_method": "google"
}
```
*Also sets `Set-Cookie: session=<token>; HttpOnly; SameSite=Strict; Path=/`*

**Errors:** `401` (token invalid or email not authorized), `400` (missing id_token)

---

### `POST /api/auth/otp/request`

Request a one-time password sent to the configured admin email.

**Request:**
```json
{
  "email": "admin@example.com"
}
```

**Response (200):**
```json
{
  "status": "otp_sent",
  "message": "A 6-digit code has been sent to your email"
}
```

**Errors:** `401` (email not authorized), `429` (rate limited)

> **Note:** If SMTP is not configured, the OTP is logged to stdout/Docker logs: `[evaNotes AUTH] Login OTP for admin@example.com: 123456`

---

### `POST /api/auth/otp/verify`

Verify an OTP code and create a session.

**Request:**
```json
{
  "email": "admin@example.com",
  "code": "123456"
}
```

**Response (200):**
```json
{
  "status": "authenticated",
  "email": "admin@example.com",
  "auth_method": "otp"
}
```

**Errors:** `401` (invalid/expired code or max attempts exceeded), `429` (rate limited)

---

### `POST /api/auth/logout`

Destroy the current session.

**Response (200):**
```json
{
  "status": "logged_out"
}
```

---

### `GET /api/auth/status`

Check if the current session is valid. Also serves as the health check endpoint.

**Response (200):**
```json
{
  "authenticated": true,
  "email": "admin@example.com"
}
```

**Response (200, unauthenticated):**
```json
{
  "authenticated": false
}
```

---

## 2. Notes

All notes endpoints require authentication.

### `GET /api/notes`

List notes with pagination, search, and filters.

**Query Parameters:**

| Param | Type | Default | Description |
|:---|:---|:---|:---|
| `page` | int | `1` | Page number (1-indexed) |
| `limit` | int | `20` | Notes per page (max 100) |
| `search` | string | — | Full-text search in title and content |
| `type` | string | — | Filter by `simple` or `detailed` |
| `archived` | bool | `false` | Include archived notes |
| `pinned` | bool | — | Filter pinned notes only |
| `sort` | string | `updated_at` | Sort field: `updated_at`, `created_at`, `title` |
| `order` | string | `desc` | Sort order: `asc` or `desc` |

**Response (200):**
```json
{
  "notes": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "title": "Meeting Notes",
      "content": "# Agenda\n- Item 1\n- Item 2",
      "note_type": "detailed",
      "is_pinned": true,
      "is_archived": false,
      "color": "#6366f1",
      "created_at": "2026-10-07T18:00:00Z",
      "updated_at": "2026-10-07T19:30:00Z",
      "reminder_count": 1
    }
  ],
  "pagination": {
    "page": 1,
    "limit": 20,
    "total": 42,
    "total_pages": 3
  }
}
```

---

### `POST /api/notes`

Create a new note.

**Request:**
```json
{
  "title": "Quick Todo",
  "content": "Buy groceries",
  "note_type": "simple",
  "color": "#10b981"
}
```

**Response (201):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440001",
  "title": "Quick Todo",
  "content": "Buy groceries",
  "note_type": "simple",
  "is_pinned": false,
  "is_archived": false,
  "color": "#10b981",
  "created_at": "2026-10-07T20:00:00Z",
  "updated_at": "2026-10-07T20:00:00Z"
}
```

**Errors:** `400` (validation error), `413` (content too large, max 1MB)

---

### `GET /api/notes/:id`

Get a single note by ID. For `detailed` notes, includes rendered HTML.

**Response (200):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "title": "Meeting Notes",
  "content": "# Agenda\n- Item 1\n- Item 2",
  "content_html": "<h1>Agenda</h1>\n<ul><li>Item 1</li><li>Item 2</li></ul>",
  "note_type": "detailed",
  "is_pinned": true,
  "is_archived": false,
  "color": "#6366f1",
  "created_at": "2026-10-07T18:00:00Z",
  "updated_at": "2026-10-07T19:30:00Z",
  "reminders": [
    {
      "id": "660e8400-e29b-41d4-a716-446655440000",
      "title": "Review meeting notes",
      "remind_at": "2026-10-08T09:00:00Z",
      "state": "pending"
    }
  ]
}
```

**Errors:** `404` (note not found or belongs to another user)

---

### `PUT /api/notes/:id`

Update a note. Partial updates supported (only include fields to change).

**Request:**
```json
{
  "title": "Updated Title",
  "content": "Updated content"
}
```

**Response (200):** Updated note object (same shape as GET response, without `content_html`)

**Errors:** `404`, `400`, `413`

---

### `DELETE /api/notes/:id`

Soft-delete a note. Sets `is_deleted = 1` and `deleted_at`.

**Response (200):**
```json
{
  "status": "deleted",
  "id": "550e8400-e29b-41d4-a716-446655440000"
}
```

---

### `PUT /api/notes/:id/pin`

Toggle pin status.

**Response (200):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "is_pinned": true
}
```

---

### `PUT /api/notes/:id/archive`

Toggle archive status.

**Response (200):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "is_archived": true
}
```

---

### `GET /api/notes/:id/export`

Export a detailed note as a Markdown `.md` file download.

**Response:** `200` with `Content-Type: text/markdown` and `Content-Disposition: attachment; filename="meeting-notes.md"`

---

## 3. Reminders

All reminder endpoints require authentication.

### `GET /api/reminders`

List reminders with filters.

**Query Parameters:**

| Param | Type | Default | Description |
|:---|:---|:---|:---|
| `state` | string | — | Filter by state: `pending`, `triggered`, `snoozed`, `acknowledged`, `dismissed` |
| `upcoming` | bool | `false` | Only show reminders with `remind_at` in the future |
| `note_id` | string | — | Filter reminders linked to a specific note |
| `page` | int | `1` | Page number |
| `limit` | int | `20` | Results per page (max 100) |

**Response (200):**
```json
{
  "reminders": [
    {
      "id": "660e8400-e29b-41d4-a716-446655440000",
      "note_id": "550e8400-e29b-41d4-a716-446655440000",
      "title": "Review meeting notes",
      "description": "Go through action items",
      "remind_at": "2026-10-08T09:00:00Z",
      "is_recurring": false,
      "cron_expr": null,
      "recur_type": null,
      "state": "pending",
      "snooze_until": null,
      "created_at": "2026-10-07T20:00:00Z",
      "updated_at": "2026-10-07T20:00:00Z",
      "last_triggered": null
    }
  ],
  "pagination": {
    "page": 1,
    "limit": 20,
    "total": 5,
    "total_pages": 1
  }
}
```

---

### `POST /api/reminders`

Create a reminder. Can be standalone or linked to a note.

**Request (one-time):**
```json
{
  "title": "Doctor appointment",
  "description": "Annual checkup at 3pm",
  "remind_at": "2026-10-15T14:30:00Z",
  "note_id": null
}
```

**Request (recurring):**
```json
{
  "title": "Weekly standup",
  "description": "Team sync every Monday at 9am",
  "remind_at": "2026-10-13T09:00:00Z",
  "is_recurring": true,
  "recur_type": "weekly",
  "cron_expr": "0 9 * * 1",
  "note_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

**Recurring Presets:**

| `recur_type` | `cron_expr` Example | Description |
|:---|:---|:---|
| `daily` | `0 9 * * *` | Every day at 9:00 AM |
| `weekly` | `0 9 * * 1` | Every Monday at 9:00 AM |
| `monthly` | `0 9 1 * *` | 1st of every month at 9:00 AM |
| `custom` | `*/30 * * * *` | User-defined cron expression |

**Response (201):** Created reminder object

**Errors:** `400` (invalid cron expression or remind_at in the past)

---

### `PUT /api/reminders/:id`

Update a reminder. Partial updates supported.

**Response (200):** Updated reminder object

---

### `DELETE /api/reminders/:id`

Permanently delete a reminder.

**Response (200):**
```json
{
  "status": "deleted",
  "id": "660e8400-e29b-41d4-a716-446655440000"
}
```

---

### `POST /api/reminders/:id/snooze`

Snooze a triggered reminder.

**Request:**
```json
{
  "duration_minutes": 15
}
```

Preset options: `5`, `15`, `30`, `60`, or custom value.

**Response (200):**
```json
{
  "id": "660e8400-e29b-41d4-a716-446655440000",
  "state": "snoozed",
  "snooze_until": "2026-10-08T09:15:00Z"
}
```

---

### `POST /api/reminders/:id/acknowledge`

Acknowledge or dismiss a triggered reminder.

**Request:**
```json
{
  "action": "acknowledge"
}
```

Valid actions: `acknowledge`, `dismiss`

**Response (200):**
```json
{
  "id": "660e8400-e29b-41d4-a716-446655440000",
  "state": "acknowledged"
}
```

---

## 4. WebSocket

### `GET /ws` → Upgrade to WebSocket

Establishes a persistent WebSocket connection for real-time reminder notifications and note sync events.

**Connection:** Requires valid session cookie.

**Server → Client Messages:**

```json
{
  "type": "reminder_triggered",
  "data": {
    "id": "660e8400-e29b-41d4-a716-446655440000",
    "title": "Weekly standup",
    "description": "Team sync every Monday at 9am",
    "triggered_at": "2026-10-13T09:00:00Z",
    "note_id": "550e8400-e29b-41d4-a716-446655440000"
  }
}
```

```json
{
  "type": "note_updated",
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "action": "updated"
  }
}
```

**Client → Server Messages:**

```json
{
  "type": "ping"
}
```

---

## 5. Common Error Responses

All error responses follow a consistent structure:

```json
{
  "error": "error_code",
  "message": "Human-readable description",
  "details": {}
}
```

| Status | Error Code | Description |
|:---|:---|:---|
| `400` | `bad_request` | Invalid request body or parameters |
| `401` | `unauthorized` | Missing or invalid authentication |
| `404` | `not_found` | Resource not found or not owned by user |
| `409` | `conflict` | Resource state conflict (e.g., snoozing a non-triggered reminder) |
| `413` | `payload_too_large` | Request body exceeds size limit (1MB) |
| `429` | `rate_limited` | Too many requests |
| `500` | `internal_error` | Unexpected server error |
