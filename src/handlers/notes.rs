use crate::auth::AuthUser;
use crate::db::notes;
use crate::error::{AppError, Result};
use crate::models::note::{CreateNoteRequest, NoteFilter, NoteType, UpdateNoteRequest};
use crate::services::markdown::{render_markdown_to_html, sanitize_filename};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct ListNotesQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub search: Option<String>,
    #[serde(rename = "type")]
    pub note_type: Option<String>,
    pub archived: Option<bool>,
    pub pinned: Option<bool>,
}

pub async fn list_notes(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Query(query): Query<ListNotesQuery>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;

    let filter = NoteFilter {
        search: query.search.filter(|s| !s.trim().is_empty()),
        note_type: query.note_type.as_deref().map(NoteType::from_str_opt),
        is_archived: query.archived,
        is_pinned: query.pinned,
        page: query.page,
        limit: query.limit,
    };

    let (notes_list, total) = notes::list_notes(&conn, &user.id, &filter)?;
    let page = filter.page.unwrap_or(1).max(1);
    let limit = filter.limit.unwrap_or(20).clamp(1, 100);
    let total_pages = if total == 0 {
        1
    } else {
        (total as f64 / limit as f64).ceil() as u64
    };

    Ok(Json(json!({
        "notes": notes_list,
        "pagination": {
            "page": page,
            "limit": limit,
            "total": total,
            "total_pages": total_pages,
        }
    })))
}

pub async fn create_note(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(req): Json<CreateNoteRequest>,
) -> Result<Response> {
    if req.title.trim().is_empty() && req.content.trim().is_empty() {
        return Err(AppError::BadRequest(
            "Note must have a title or content".into(),
        ));
    }

    if req.content.len() > 1_048_576 {
        return Err(AppError::BadRequest(
            "Note content exceeds 1MB limit".into(),
        ));
    }

    let conn = state.pool.get()?;
    let note = notes::create_note(&conn, &user.id, req)?;

    Ok((StatusCode::CREATED, Json(note)).into_response())
}

pub async fn get_note(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let note = notes::get_note_by_id(&conn, &user.id, &note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;

    let content_html = if note.note_type == NoteType::Detailed {
        Some(render_markdown_to_html(&note.content))
    } else {
        None
    };

    Ok(Json(json!({
        "note": note,
        "content_html": content_html,
    })))
}

pub async fn update_note(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
    Json(req): Json<UpdateNoteRequest>,
) -> Result<impl IntoResponse> {
    if let Some(ref content) = req.content {
        if content.len() > 1_048_576 {
            return Err(AppError::BadRequest(
                "Note content exceeds 1MB limit".into(),
            ));
        }
    }

    let conn = state.pool.get()?;
    let updated = notes::update_note(&conn, &user.id, &note_id, req)?;

    Ok(Json(updated))
}

pub async fn delete_note(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    notes::soft_delete_note(&conn, &user.id, &note_id)?;

    Ok(Json(json!({
        "status": "deleted",
        "id": note_id,
    })))
}

pub async fn toggle_pin(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let is_pinned = notes::toggle_pin_note(&conn, &user.id, &note_id)?;

    Ok(Json(json!({
        "id": note_id,
        "is_pinned": is_pinned,
    })))
}

pub async fn toggle_archive(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let is_archived = notes::toggle_archive_note(&conn, &user.id, &note_id)?;

    Ok(Json(json!({
        "id": note_id,
        "is_archived": is_archived,
    })))
}

pub async fn export_note(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
) -> Result<Response> {
    let conn = state.pool.get()?;
    let note = notes::get_note_by_id(&conn, &user.id, &note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;

    let filename = format!("{}.md", sanitize_filename(&note.title));
    let content_disposition = format!("attachment; filename=\"{filename}\"");

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "text/markdown; charset=utf-8".parse().unwrap(),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        content_disposition.parse().unwrap(),
    );

    Ok((StatusCode::OK, headers, note.content).into_response())
}
