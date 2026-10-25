-- V004: Reminder delivery channels (web, email, telegram) stored as a JSON array.
-- e.g. '["web"]', '["web","email"]', '["web","telegram"]'

ALTER TABLE reminders ADD COLUMN channels TEXT NOT NULL DEFAULT '["web"]';
