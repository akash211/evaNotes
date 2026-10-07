use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NoteType {
    Simple,
    Detailed,
}

impl NoteType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Detailed => "detailed",
        }
    }

    pub fn from_str_opt(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "detailed" => Self::Detailed,
            _ => Self::Simple,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub content: String,
    pub note_type: NoteType,
    pub is_pinned: bool,
    pub is_archived: bool,
    pub is_deleted: bool,
    pub color: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNoteRequest {
    pub title: String,
    pub content: String,
    #[serde(default = "default_note_type")]
    pub note_type: NoteType,
    pub color: Option<String>,
}

fn default_note_type() -> NoteType {
    NoteType::Simple
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateNoteRequest {
    pub title: Option<String>,
    pub content: Option<String>,
    pub note_type: Option<NoteType>,
    pub color: Option<String>,
    pub is_pinned: Option<bool>,
    pub is_archived: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NoteFilter {
    pub search: Option<String>,
    pub note_type: Option<NoteType>,
    pub is_archived: Option<bool>,
    pub is_pinned: Option<bool>,
    pub page: Option<u32>,
    pub limit: Option<u32>,
}
