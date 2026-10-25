use crate::auth::AuthUser;
use crate::db::folders;
use crate::error::{AppError, Result};
use crate::models::folder::{CreateFolderRequest, UpdateFolderRequest};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub async fn list_folders(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let tree = folders::list_folder_tree(&conn, &user.id)?;
    Ok(Json(json!({ "folders": tree })))
}

pub async fn create_folder(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(req): Json<CreateFolderRequest>,
) -> Result<Response> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("Folder name is required".into()));
    }
    let conn = state.pool.get()?;
    let folder = folders::create_folder(&conn, &user.id, &req.name, req.parent_id, req.icon)?;
    Ok((StatusCode::CREATED, Json(folder)).into_response())
}

pub async fn update_folder(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(folder_id): Path<String>,
    Json(req): Json<UpdateFolderRequest>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let folder = folders::update_folder(
        &conn,
        &user.id,
        &folder_id,
        req.name,
        req.parent_id,
        req.icon,
    )?;
    Ok(Json(folder))
}

pub async fn delete_folder(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(folder_id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    folders::delete_folder(&conn, &user.id, &folder_id)?;
    Ok(Json(json!({ "status": "deleted", "id": folder_id })))
}
