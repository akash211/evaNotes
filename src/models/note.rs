use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NoteType {
    Simple,
    Detailed,
    Code,
    Sheet,
}

impl NoteType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Detailed => "detailed",
            Self::Code => "code",
            Self::Sheet => "sheet",
        }
    }

    pub fn from_str_opt(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "detailed" => Self::Detailed,
            "code" => Self::Code,
            "sheet" => Self::Sheet,
            _ => Self::Simple,
        }
    }

    pub fn all() -> [Self; 4] {
        [Self::Simple, Self::Detailed, Self::Code, Self::Sheet]
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
    /// Assigned symbol/emoji shown next to the title.
    pub icon: Option<String>,
    /// Parent folder id (None = top level).
    pub folder_id: Option<String>,
    /// Folder name, populated for list endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_name: Option<String>,
    /// Tag names attached to this note.
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNoteRequest {
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default = "default_note_type")]
    pub note_type: NoteType,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub folder_id: Option<String>,
    /// Tag names to attach on creation (unknown names are auto-created).
    #[serde(default)]
    pub tags: Vec<String>,
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
    pub icon: Option<String>,
    pub is_pinned: Option<bool>,
    pub is_archived: Option<bool>,
    /// None = unchanged; Some(None) = move to top level; Some(Some(id)) = move into folder.
    #[serde(default, deserialize_with = "deserialize_tristate")]
    pub folder_id: Option<Option<String>>,
    /// When provided, replaces the note's full tag set.
    pub tags: Option<Vec<String>>,
}

/// Deserializer for tri-state fields: absent -> None (unchanged),
/// null -> Some(None), value -> Some(Some(value)).
pub fn deserialize_tristate<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

/// Which parts of a note the search query should match. `search_in` maps to
/// title / content / tags / folders. Defaults to all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchScope {
    Title,
    Content,
    Tags,
    Folders,
}

impl SearchScope {
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "title" => Some(Self::Title),
            "content" => Some(Self::Content),
            "tags" | "tag" => Some(Self::Tags),
            "folders" | "folder" => Some(Self::Folders),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NoteFilter {
    pub search: Option<String>,
    pub search_in: Option<Vec<SearchScope>>,
    pub note_type: Option<NoteType>,
    pub is_archived: Option<bool>,
    pub is_pinned: Option<bool>,
    /// Filter notes carrying this exact tag name. Repeated params = AND.
    pub tags: Vec<String>,
    /// Folder id; notes in this folder and its subfolders. "root" = no folder.
    pub folder_id: Option<String>,
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

/// Language metadata for code notes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeLanguage {
    pub id: &'static str,
    pub label: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_id_tristate_serde() {
        // null -> Some(None) (move to root)
        let v: UpdateNoteRequest = serde_json::from_str(r#"{"folder_id": null}"#).unwrap();
        assert!(matches!(v.folder_id, Some(None)));

        // value -> Some(Some(id))
        let v2: UpdateNoteRequest = serde_json::from_str(r#"{"folder_id": "abc"}"#).unwrap();
        assert!(matches!(v2.folder_id, Some(Some(ref s)) if s == "abc"));

        // absent -> None (unchanged)
        let v3: UpdateNoteRequest = serde_json::from_str(r#"{}"#).unwrap();
        assert!(v3.folder_id.is_none());
    }
}
