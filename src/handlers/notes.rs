use crate::auth::AuthUser;
use crate::db::notes;
use crate::db::shares;
use crate::db::tags;
use crate::db::versions;
use crate::error::{AppError, Result};
use crate::models::note::{
    CreateNoteRequest, NoteFilter, NoteType, SearchScope, UpdateNoteRequest,
};
use crate::models::share::CreateShareRequest;
use crate::models::tag::SetNoteTagsRequest;
use crate::models::version::RestoreVersionRequest;
use crate::services::markdown::{render_code_to_html, render_markdown_to_html, sanitize_filename};
use crate::services::pdf::note_to_pdf_bytes;
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
    /// Comma-separated scopes: title,content,tags,folders (default: all)
    pub search_in: Option<String>,
    #[serde(rename = "type")]
    pub note_type: Option<String>,
    pub archived: Option<bool>,
    pub pinned: Option<bool>,
    /// Comma-separated tag names = AND filter
    pub tag: Option<String>,
    /// Folder id, or "root"
    pub folder: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetNoteQuery {
    /// This client's device id — other open editors are reported back.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    pub format: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PresencePayload {
    #[serde(default)]
    pub device: String,
}

#[derive(Debug, Deserialize)]
pub struct HighlightPayload {
    pub language: String,
    pub code: String,
}

pub async fn list_notes(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Query(query): Query<ListNotesQuery>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;

    let search_in: Option<Vec<SearchScope>> = query.search_in.as_deref().map(|raw| {
        raw.split(',')
            .filter_map(SearchScope::from_str_opt)
            .collect::<Vec<_>>()
    });

    let filter = NoteFilter {
        search: query.search.filter(|s| !s.trim().is_empty()),
        search_in,
        note_type: query.note_type.as_deref().map(NoteType::from_str_opt),
        is_archived: query.archived,
        is_pinned: query.pinned,
        tags: query
            .tag
            .map(|t| {
                t.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        folder_id: query.folder.filter(|s| !s.trim().is_empty()),
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

    let mut conn = state.pool.get()?;
    let note = notes::create_note(&mut conn, &user.id, req)?;

    Ok((StatusCode::CREATED, Json(note)).into_response())
}

pub async fn get_note(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
    Query(query): Query<GetNoteQuery>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let note = notes::get_note_by_id(&conn, &user.id, &note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;

    let content_html = match note.note_type {
        NoteType::Detailed => Some(render_markdown_to_html(&note.content)),
        NoteType::Code => Some(render_code_to_html(&note.content, "text")),
        _ => None,
    };

    // Who else has this note open right now?
    let active_editors = query
        .device
        .map(|d| state.presence.other_editors(&note_id, &d))
        .unwrap_or_default();

    Ok(Json(json!({
        "note": note,
        "content_html": content_html,
        "active_editors": active_editors,
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

    let mut conn = state.pool.get()?;
    let updated = notes::update_note(
        &mut conn,
        &user.id,
        &note_id,
        req,
        state.config.version_min_interval_secs,
        state.config.version_max_keep,
    )?;

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

/// Replace the full tag set of a note (unknown tag names are auto-created).
pub async fn set_note_tags(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
    Json(req): Json<SetNoteTagsRequest>,
) -> Result<impl IntoResponse> {
    let mut conn = state.pool.get()?;
    let tx = conn.transaction()?;
    let applied = tags::set_note_tags(&tx, &user.id, &note_id, &req.tags)?;
    tx.commit()?;

    Ok(Json(json!({
        "id": note_id,
        "tags": applied,
    })))
}

// ---------------------------------------------------------------- versions

pub async fn list_versions(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let _ = notes::get_note_by_id(&conn, &user.id, &note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;
    let versions = versions::list_versions(&conn, &note_id)?;
    Ok(Json(json!({ "versions": versions })))
}

pub async fn clear_versions(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let _ = notes::get_note_by_id(&conn, &user.id, &note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;
    let count = versions::clear_versions(&conn, &note_id)?;
    Ok(Json(json!({ "status": "cleared", "deleted": count })))
}

pub async fn restore_version(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
    Json(req): Json<RestoreVersionRequest>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let _ = notes::get_note_by_id(&conn, &user.id, &note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;
    let version = versions::get_version(&conn, &note_id, req.version_id)?
        .ok_or_else(|| AppError::NotFound(format!("Version {} not found", req.version_id)))?;

    let mut conn2 = state.pool.get()?;
    let updated = notes::update_note(
        &mut conn2,
        &user.id,
        &note_id,
        UpdateNoteRequest {
            title: Some(version.title),
            content: Some(version.content),
            note_type: Some(NoteType::from_str_opt(&version.note_type)),
            ..Default::default()
        },
        state.config.version_min_interval_secs,
        state.config.version_max_keep,
    )?;

    Ok(Json(json!({
        "status": "restored",
        "note": updated,
    })))
}

// ---------------------------------------------------------------- sharing

pub async fn get_share(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let _ = notes::get_note_by_id(&conn, &user.id, &note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;
    let mut share = shares::get_share_for_note(&conn, &user.id, &note_id)?;
    if let Some(ref mut s) = share {
        s.path = Some(format!("/share/{}", s.id));
    }
    Ok(Json(json!({ "share": share })))
}

pub async fn create_share(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
    Json(req): Json<CreateShareRequest>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let _ = notes::get_note_by_id(&conn, &user.id, &note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;
    let mut share = shares::create_share(&conn, &user.id, &note_id, &req.mode)?;
    share.path = Some(format!("/share/{}", share.id));
    Ok(Json(json!({ "share": share })))
}

pub async fn delete_share(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    shares::delete_share_for_note(&conn, &user.id, &note_id)?;
    Ok(Json(json!({ "status": "revoked", "note_id": note_id })))
}

// ---------------------------------------------------------------- presence

pub async fn note_presence_heartbeat(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
    Json(req): Json<PresencePayload>,
) -> Result<impl IntoResponse> {
    let device = req.device.trim().to_string();
    if device.is_empty() {
        return Err(AppError::BadRequest("device is required".into()));
    }
    // Only the note's owner may register presence for it
    {
        let conn = state.pool.get()?;
        let _ = notes::get_note_by_id(&conn, &user.id, &note_id)?
            .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;
    }
    state.presence.heartbeat(&note_id, &device);
    let others = state.presence.other_editors(&note_id, &device);
    Ok(Json(json!({
        "status": "ok",
        "active_editors": others,
    })))
}

pub async fn note_presence_leave(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
    Json(req): Json<PresencePayload>,
) -> Result<impl IntoResponse> {
    // Only the note's owner may clear presence for it
    {
        let conn = state.pool.get()?;
        let _ = notes::get_note_by_id(&conn, &user.id, &note_id)?
            .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;
    }
    state.presence.leave(&note_id, req.device.trim());
    Ok(Json(json!({ "status": "left" })))
}

// ---------------------------------------------------------------- highlight

pub async fn highlight_code(
    State(_state): State<AppState>,
    AuthUser(_user): AuthUser,
    Json(req): Json<HighlightPayload>,
) -> Result<impl IntoResponse> {
    if req.code.len() > 500_000 {
        return Err(AppError::BadRequest("Code too large to highlight".into()));
    }
    let html = render_code_to_html(&req.code, &req.language);
    Ok(Json(json!({ "html": html })))
}

// ---------------------------------------------------------------- export

pub async fn export_note(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(note_id): Path<String>,
    Query(query): Query<ExportQuery>,
) -> Result<Response> {
    let conn = state.pool.get()?;
    let note = notes::get_note_by_id(&conn, &user.id, &note_id)?
        .ok_or_else(|| AppError::NotFound(format!("Note {note_id} not found")))?;

    if query.format.as_deref() == Some("pdf") {
        let filename = format!("{}.pdf", sanitize_filename(&note.title));
        let content_disposition = format!("attachment; filename=\"{filename}\"");
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "application/pdf".parse().unwrap());
        headers.insert(
            header::CONTENT_DISPOSITION,
            content_disposition.parse().unwrap(),
        );
        let pdf = note_to_pdf_bytes(&note);
        return Ok((StatusCode::OK, headers, pdf).into_response());
    }

    // Default: Markdown (code notes too — wrapped in a fenced block)
    let content = match note.note_type {
        NoteType::Code => format!("```{}\n{}\n```", "text", note.content),
        _ => note.content,
    };
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

    Ok((StatusCode::OK, headers, content).into_response())
}
