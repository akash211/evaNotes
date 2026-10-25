use crate::models::reminder::Reminder;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Per-user notification fan-out. Events are only ever delivered to WebSocket
/// clients belonging to the same user, so one account can never observe
/// another account's reminder content.
#[derive(Clone, Default)]
pub struct NotificationHub {
    channels: Arc<Mutex<HashMap<String, broadcast::Sender<String>>>>,
}

impl NotificationHub {
    pub fn new() -> Self {
        Self::default()
    }

    fn sender_for(&self, user_id: &str) -> broadcast::Sender<String> {
        let mut map = self.channels.lock().unwrap();
        map.entry(user_id.to_string())
            .or_insert_with(|| {
                let (tx, _rx) = broadcast::channel(64);
                tx
            })
            .clone()
    }

    pub fn subscribe(&self, user_id: &str) -> broadcast::Receiver<String> {
        self.sender_for(user_id).subscribe()
    }

    pub fn broadcast_reminder(&self, reminder: &Reminder) {
        let msg = json!({
            "type": "reminder_triggered",
            "data": reminder
        })
        .to_string();

        let map = self.channels.lock().unwrap();
        if let Some(tx) = map.get(&reminder.user_id) {
            let _ = tx.send(msg);
        }
    }

    pub fn broadcast_event(&self, user_id: &str, event_type: &str, data: serde_json::Value) {
        let msg = json!({
            "type": event_type,
            "data": data
        })
        .to_string();

        let map = self.channels.lock().unwrap();
        if let Some(tx) = map.get(user_id) {
            let _ = tx.send(msg);
        }
    }
}
