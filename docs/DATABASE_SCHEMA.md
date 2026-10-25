# evaNotes — Database Schema

SQLite database (WAL mode), migrated by [refinery](https://github.com/rust-db/refinery)
at startup from `migrations/`. Migrations are append-only.

## Tables

### users

| Column | Type | Notes |
|:--|:--|:--|
| id | TEXT PK | UUIDv4 |
| email | TEXT UNIQUE NOT NULL | lowercase |
| auth_method | TEXT | `otp` or `google` |
| is_admin | INTEGER | 0/1 |
| created_at | TEXT | UTC `YYYY-MM-DD HH:MM:SS` |
| last_login | TEXT | UTC |

### sessions

| Column | Type | Notes |
|:--|:--|:--|
| token | TEXT PK | HMAC-signed, `userId:nonce.signature` |
| user_id | TEXT FK → users.id | ON DELETE CASCADE |
| created_at | TEXT | |
| expires_at | TEXT | UTC `YYYY-MM-DD HH:MM:SS`, compared with `datetime('now')` |

### otps

| Column | Type | Notes |
|:--|:--|:--|
| id | INTEGER PK AUTOINCREMENT | |
| email | TEXT | login email |
| code | TEXT | 6 digits |
| created_at | TEXT | |
| expires_at | TEXT | UTC `YYYY-MM-DD HH:MM:SS` (5 min) |
| used | INTEGER | single-use flag |
| attempts | INTEGER | max 5 |

### notes

| Column | Type | Notes |
|:--|:--|:--|
| id | TEXT PK | UUIDv4 |
| user_id | TEXT FK → users.id | CASCADE |
| title | TEXT | |
| content | TEXT | up to 1 MB |
| note_type | TEXT | `simple` \| `detailed` \| `code` \| `sheet` |
| is_pinned / is_archived / is_deleted | INTEGER | 0/1 (soft delete) |
| color | TEXT NULL | hex color |
| icon | TEXT NULL (V005) | assigned symbol/emoji |
| folder_id | TEXT NULL FK → folders.id (V005) | ON DELETE SET NULL (top level) |
| created_at / updated_at / deleted_at | TEXT | |

### folders (V005)

| Column | Type | Notes |
|:--|:--|:--|
| id | TEXT PK | UUIDv4 |
| user_id | TEXT FK → users.id | CASCADE |
| name | TEXT | |
| parent_id | TEXT NULL FK → folders.id | ON DELETE SET NULL; nesting depth unlimited |
| icon | TEXT NULL | |
| created_at / updated_at | TEXT | |

### note_versions (V005)

| Column | Type | Notes |
|:--|:--|:--|
| id | INTEGER PK AUTOINCREMENT | |
| note_id | TEXT FK → notes.id | CASCADE |
| title / content / note_type | TEXT | frozen previous state |
| created_at | TEXT | UTC; snapshots throttled to `VERSION_MIN_INTERVAL_SECS` and pruned to `VERSION_MAX_KEEP` |

### shares (V005)

| Column | Type | Notes |
|:--|:--|:--|
| id | TEXT PK | URL slug |
| user_id | TEXT FK → users.id | CASCADE |
| note_id | TEXT FK → notes.id | CASCADE |
| mode | TEXT | `live` \| `snapshot` |
| snapshot_title / snapshot_content / snapshot_type | TEXT NULL | frozen copy for snapshot mode |
| created_at | TEXT | |

### tags & note_tags (V003)

| tags | Type | Notes |
|:--|:--|:--|
| id | INTEGER PK AUTOINCREMENT | |
| user_id | TEXT FK → users.id | CASCADE |
| name | TEXT | `UNIQUE(user_id, name COLLATE NOCASE)` |
| color | TEXT NULL | |
| created_at | TEXT | |

| note_tags | Type | Notes |
|:--|:--|:--|
| note_id | TEXT FK → notes.id | CASCADE, part of PK |
| tag_id | INTEGER FK → tags.id | CASCADE, part of PK |

### reminders

| Column | Type | Notes |
|:--|:--|:--|
| id | TEXT PK | UUIDv4 |
| user_id | TEXT FK → users.id | CASCADE |
| note_id | TEXT NULL FK → notes.id | ON DELETE SET NULL |
| title / description | TEXT | |
| remind_at | TEXT | canonical UTC `YYYY-MM-DDTHH:MM:SSZ` |
| is_recurring | INTEGER | |
| cron_expr | TEXT NULL | 5/6/7-field cron |
| recur_type | TEXT NULL | `daily` \| `weekly` \| `monthly` \| `custom` |
| state | TEXT | `pending` \| `triggered` \| `snoozed` \| `acknowledged` \| `dismissed` |
| snooze_until | TEXT NULL | |
| channels | TEXT (V004) | JSON array, e.g. `["web","email","telegram"]`, default `["web"]` |
| created_at / updated_at / last_triggered | TEXT | |

### notes_fts (V003, virtual)

`CREATE VIRTUAL TABLE notes_fts USING fts5(title, content, content='notes', content_rowid='rowid')`
— external-content FTS5 index over note **title + content**, kept in sync by triggers
(`notes_fts_ai`, `notes_fts_ad`, `notes_fts_au`) and backfilled with a `rebuild` on migration.

Search queries are sanitized into quoted prefix tokens (`"term"*`), e.g. `"organic"* "coffee"*`.

## Indexes

- `idx_notes_user_id`, `idx_notes_deleted`, `idx_notes_type`, `idx_notes_updated`, `idx_notes_pinned`
- `idx_reminders_user_id`, `idx_reminders_state`, `idx_reminders_remind_at`, `idx_reminders_note_id`
- `idx_sessions_user_id`, `idx_sessions_expires`
- `idx_otps_email`, `idx_otps_expires`
- `idx_tags_user_name` — powers fast tag autocomplete/filtering
- `idx_note_tags_tag`

## Timestamp conventions

- `remind_at` / `snooze_until`: canonical UTC `YYYY-MM-DDTHH:MM:SSZ`. Engine comparisons use
  `strftime('%Y-%m-%dT%H:%M:%SZ','now')`. Never compare against `datetime('now')`.
- `created_at` / `updated_at` / `expires_at` everywhere else: UTC `YYYY-MM-DD HH:MM:SS`,
  comparable with `datetime('now')`.

## Migrations

| File | Purpose |
|:--|:--|
| `V001__initial_schema.sql` | users, sessions, otps, notes, reminders |
| `V002__add_indexes.sql` | performance indexes |
| `V003__tags_and_search.sql` | tags, note_tags, notes_fts + triggers + backfill |
| `V004__reminder_channels.sql` | reminders.channels JSON column |
| `V005__folders_versions_shares.sql` | folders (nested), notes.folder_id + icon, note_versions, shares |
