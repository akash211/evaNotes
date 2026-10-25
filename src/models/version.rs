use serde::{Deserialize, Serialize};

/// A stored previous state of a note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteVersion {
    pub id: i64,
    pub note_id: String,
    pub title: String,
    pub content: String,
    pub note_type: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreVersionRequest {
    pub version_id: i64,
}
