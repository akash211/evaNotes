use crate::error::{AppError, Result};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

/// Canonical reminder timestamp used everywhere: "YYYY-MM-DDTHH:MM:SSZ" (UTC).
/// This format compares correctly with SQLite's strftime('%Y-%m-%dT%H:%M:%SZ','now')
/// and parses correctly in JavaScript via `new Date(...)`.
pub const CANONICAL_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

pub fn now_canonical() -> String {
    Utc::now().format(CANONICAL_FORMAT).to_string()
}

/// Normalize any accepted input (RFC 3339 with offset, ISO with milliseconds,
/// or `datetime-local` "YYYY-MM-DDTHH:MM" which is interpreted as the server's
/// local time) into the canonical UTC format.
pub fn normalize_remind_at(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("remind_at is required".into()));
    }

    // 1. RFC 3339 (e.g. 2026-10-24T19:00:00Z, +05:30, or with milliseconds)
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(dt.with_timezone(&Utc).format(CANONICAL_FORMAT).to_string());
    }

    // 2. datetime-local input without timezone ("YYYY-MM-DDTHH:MM" or with seconds)
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            if let Some(local) = chrono::Local.from_local_datetime(&naive).earliest() {
                return Ok(local
                    .with_timezone(&Utc)
                    .format(CANONICAL_FORMAT)
                    .to_string());
            }
        }
    }

    // 3. Already canonical (accept as-is so idempotent updates work)
    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, CANONICAL_FORMAT) {
        return Ok(naive.and_utc().format(CANONICAL_FORMAT).to_string());
    }

    Err(AppError::BadRequest(format!(
        "Invalid remind_at timestamp: {trimmed}. Use RFC 3339 or datetime-local format."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_rfc3339_with_offset() {
        let out = normalize_remind_at("2026-10-24T19:00:00+05:30").unwrap();
        assert_eq!(out, "2026-10-24T13:30:00Z");
    }

    #[test]
    fn normalizes_rfc3339_with_millis() {
        let out = normalize_remind_at("2026-10-24T19:00:00.000Z").unwrap();
        assert_eq!(out, "2026-10-24T19:00:00Z");
    }

    #[test]
    fn keeps_canonical() {
        let out = normalize_remind_at("2026-10-24T19:00:00Z").unwrap();
        assert_eq!(out, "2026-10-24T19:00:00Z");
    }

    #[test]
    fn rejects_garbage() {
        assert!(normalize_remind_at("not-a-time").is_err());
        assert!(normalize_remind_at("").is_err());
    }
}
