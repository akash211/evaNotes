use serde::{Deserialize, Serialize};

/// A user-owned tag that can be attached to any number of notes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub user_id: String,
    pub name: String,
    pub color: Option<String>,
    /// Number of notes carrying this tag (populated for list endpoints).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTagRequest {
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateTagRequest {
    pub name: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetNoteTagsRequest {
    /// Full, ordered list of tag names this note should carry.
    /// Unknown names are created automatically.
    #[serde(default)]
    pub tags: Vec<String>,
}
