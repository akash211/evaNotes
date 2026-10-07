use crate::models::reminder::Reminder;
use serde_json::json;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct NotificationHub {
    tx: broadcast::Sender<String>,
}

impl NotificationHub {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(128);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    pub fn broadcast_reminder(&self, reminder: &Reminder) {
        let msg = json!({
            "type": "reminder_triggered",
            "data": reminder
        })
        .to_string();

        let _ = self.tx.send(msg);
    }

    pub fn broadcast_event(&self, event_type: &str, data: serde_json::Value) {
        let msg = json!({
            "type": event_type,
            "data": data
        })
        .to_string();

        let _ = self.tx.send(msg);
    }
}

impl Default for NotificationHub {
    fn default() -> Self {
        Self::new()
    }
}
