use serde::{Deserialize, Serialize};

/// A public share link. `mode` is either "snapshot" (frozen copy made when the
/// link was created) or "live" (always shows the note's current state).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Share {
    pub id: String,
    pub user_id: String,
    pub note_id: String,
    pub mode: String,
    pub created_at: String,
    /// Absolute-ish path of the public page; UI composes the full URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShareRequest {
    #[serde(default = "default_share_mode")]
    pub mode: String, // "snapshot" | "live"
}

fn default_share_mode() -> String {
    "live".to_string()
}
