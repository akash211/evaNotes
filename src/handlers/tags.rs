use crate::auth::AuthUser;
use crate::db::tags;
use crate::error::{AppError, Result};
use crate::models::tag::{CreateTagRequest, UpdateTagRequest};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct SuggestTagsQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

/// List all tags with note counts.
pub async fn list_tags(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let tags = tags::list_tags_with_counts(&conn, &user.id)?;
    Ok(Json(json!({ "tags": tags })))
}

/// Fast prefix autocomplete for the tag input.
pub async fn suggest_tags(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Query(query): Query<SuggestTagsQuery>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let prefix = query.q.unwrap_or_default();
    let limit = query.limit.unwrap_or(12);
    let tags = tags::suggest_tags(&conn, &user.id, &prefix, limit)?;
    Ok(Json(json!({ "tags": tags })))
}

pub async fn create_tag(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(req): Json<CreateTagRequest>,
) -> Result<Response> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("Tag name is required".into()));
    }
    let conn = state.pool.get()?;
    let tag = tags::create_tag(&conn, &user.id, &req.name, req.color)?;
    Ok((StatusCode::CREATED, Json(tag)).into_response())
}

pub async fn update_tag(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(tag_id): Path<i64>,
    Json(req): Json<UpdateTagRequest>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let tag = tags::rename_tag(&conn, &user.id, tag_id, req.name, req.color)?;
    Ok(Json(tag))
}

pub async fn delete_tag(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(tag_id): Path<i64>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    tags::delete_tag(&conn, &user.id, tag_id)?;
    Ok(Json(json!({
        "status": "deleted",
        "id": tag_id,
    })))
}
