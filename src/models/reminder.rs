use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReminderState {
    Pending,
    Triggered,
    Snoozed,
    Acknowledged,
    Dismissed,
}

impl ReminderState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Triggered => "triggered",
            Self::Snoozed => "snoozed",
            Self::Acknowledged => "acknowledged",
            Self::Dismissed => "dismissed",
        }
    }

    pub fn from_str_opt(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "triggered" => Self::Triggered,
            "snoozed" => Self::Snoozed,
            "acknowledged" => Self::Acknowledged,
            "dismissed" => Self::Dismissed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RecurType {
    Daily,
    Weekly,
    Monthly,
    Custom,
}

impl RecurType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Custom => "custom",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            "monthly" => Some(Self::Monthly),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Known reminder delivery channels.
pub const CHANNEL_WEB: &str = "web";
pub const CHANNEL_EMAIL: &str = "email";
pub const CHANNEL_TELEGRAM: &str = "telegram";

pub fn default_channels() -> Vec<String> {
    vec![CHANNEL_WEB.to_string()]
}

pub fn parse_channels(raw: Option<String>) -> Vec<String> {
    let raw = match raw {
        Some(s) if !s.trim().is_empty() => s,
        _ => return default_channels(),
    };
    match serde_json::from_str::<Vec<String>>(&raw) {
        Ok(channels) => {
            let mut channels: Vec<String> = channels
                .into_iter()
                .map(|c| c.trim().to_lowercase())
                .filter(|c| !c.is_empty())
                .collect();
            channels.dedup();
            if channels.is_empty() {
                default_channels()
            } else {
                channels
            }
        }
        Err(_) => default_channels(),
    }
}

pub fn serialize_channels(channels: &[String]) -> String {
    serde_json::to_string(channels).unwrap_or_else(|_| "[\"web\"]".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub id: String,
    pub user_id: String,
    pub note_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    /// Canonical UTC timestamp: "YYYY-MM-DDTHH:MM:SSZ".
    pub remind_at: String,
    pub is_recurring: bool,
    pub cron_expr: Option<String>,
    pub recur_type: Option<RecurType>,
    pub state: ReminderState,
    pub snooze_until: Option<String>,
    /// Delivery channels, e.g. ["web", "telegram"].
    #[serde(default = "default_channels")]
    pub channels: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_triggered: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReminderRequest {
    pub title: String,
    pub description: Option<String>,
    pub remind_at: String,
    #[serde(default)]
    pub is_recurring: bool,
    pub cron_expr: Option<String>,
    pub recur_type: Option<RecurType>,
    pub note_id: Option<String>,
    #[serde(default = "default_channels")]
    pub channels: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateReminderRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub remind_at: Option<String>,
    pub is_recurring: Option<bool>,
    pub cron_expr: Option<String>,
    pub recur_type: Option<RecurType>,
    pub note_id: Option<String>,
    pub channels: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnoozeReminderRequest {
    #[serde(default = "default_snooze_duration")]
    pub duration_minutes: u32,
}

fn default_snooze_duration() -> u32 {
    15
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeReminderRequest {
    #[serde(default = "default_action")]
    pub action: String, // "acknowledge" or "dismiss"
}

fn default_action() -> String {
    "acknowledge".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReminderFilter {
    pub state: Option<ReminderState>,
    pub note_id: Option<String>,
    pub upcoming_only: Option<bool>,
    pub page: Option<u32>,
    pub limit: Option<u32>,
}
