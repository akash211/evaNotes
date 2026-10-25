use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Direct note count (children of this folder only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_count: Option<i64>,
    /// Subfolders (populated by the tree endpoint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Folder>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFolderRequest {
    pub name: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateFolderRequest {
    pub name: Option<String>,
    /// Some(None) = move to top level.
    #[serde(
        default,
        deserialize_with = "crate::models::note::deserialize_tristate"
    )]
    pub parent_id: Option<Option<String>>,
    pub icon: Option<String>,
}
