-- V005: Folders (with nesting), note versions, public shares, note icons

CREATE TABLE IF NOT EXISTS folders (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    parent_id  TEXT REFERENCES folders(id) ON DELETE SET NULL,
    icon       TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_folders_user_parent ON folders(user_id, parent_id);

-- Notes can live in a folder (NULL = top level, no folder)
ALTER TABLE notes ADD COLUMN folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL;
ALTER TABLE notes ADD COLUMN icon TEXT;
CREATE INDEX IF NOT EXISTS idx_notes_folder ON notes(folder_id);

-- Automatic version history: previous note states, snapshotted at least
-- VERSION_MIN_INTERVAL_SECS (default 900) apart, pruned to the newest
-- VERSION_MAX_KEEP (default 10).
CREATE TABLE IF NOT EXISTS note_versions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    note_id    TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    note_type  TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_versions_note ON note_versions(note_id, created_at DESC);

-- Public share links. mode: 'snapshot' (frozen copy) or 'live' (always current).
CREATE TABLE IF NOT EXISTS shares (
    id               TEXT PRIMARY KEY,
    user_id          TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    note_id          TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    mode             TEXT NOT NULL DEFAULT 'live',
    snapshot_title   TEXT,
    snapshot_content TEXT,
    snapshot_type    TEXT,
    created_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_shares_note ON shares(note_id);
