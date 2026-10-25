use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long an editor is considered "open" without a heartbeat.
const STALE_AFTER: Duration = Duration::from_secs(75);

#[derive(Debug, Clone, Serialize)]
pub struct EditorInfo {
    pub device: String,
    pub since: String,
}

struct Entry {
    device: String,
    last_seen: Instant,
}

/// Tracks which devices currently have a note open (for the multi-device
/// "note is open elsewhere" warning).
#[derive(Clone, Default)]
pub struct PresenceHub {
    entries: Arc<Mutex<HashMap<String, Vec<Entry>>>>,
}

impl PresenceHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register / refresh presence for (note_id, device).
    pub fn heartbeat(&self, note_id: &str, device: &str) {
        let mut map = self.entries.lock().unwrap();
        let list = map.entry(note_id.to_string()).or_default();
        let now = Instant::now();
        if let Some(entry) = list.iter_mut().find(|e| e.device == device) {
            entry.last_seen = now;
        } else {
            list.push(Entry {
                device: device.to_string(),
                last_seen: now,
            });
        }
    }

    pub fn leave(&self, note_id: &str, device: &str) {
        let mut map = self.entries.lock().unwrap();
        if let Some(list) = map.get_mut(note_id) {
            list.retain(|e| e.device != device);
        }
    }

    /// Devices OTHER than `except_device` that currently have the note open.
    pub fn other_editors(&self, note_id: &str, except_device: &str) -> Vec<EditorInfo> {
        let mut map = self.entries.lock().unwrap();
        let now = Instant::now();
        let list = map.entry(note_id.to_string()).or_default();

        // Prune stale entries
        list.retain(|e| now.duration_since(e.last_seen) < STALE_AFTER);

        list.iter()
            .filter(|e| e.device != except_device)
            .map(|e| EditorInfo {
                device: e.device.clone(),
                since: format!("{}s ago", now.duration_since(e.last_seen).as_secs()),
            })
            .collect()
    }
}
