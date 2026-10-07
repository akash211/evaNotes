use crate::auth::AuthUser;
use crate::db::reminders;
use crate::error::{AppError, Result};
use crate::models::reminder::{
    AcknowledgeReminderRequest, CreateReminderRequest, RecurType, ReminderFilter, ReminderState,
    SnoozeReminderRequest, UpdateReminderRequest,
};
use crate::services::reminder_engine::compute_next_cron_trigger;
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
pub struct ListRemindersQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub state: Option<String>,
    pub note_id: Option<String>,
    pub upcoming: Option<bool>,
}

pub async fn list_reminders(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Query(query): Query<ListRemindersQuery>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;

    let filter = ReminderFilter {
        state: query.state.as_deref().map(ReminderState::from_str_opt),
        note_id: query.note_id.filter(|s| !s.trim().is_empty()),
        upcoming_only: query.upcoming,
        page: query.page,
        limit: query.limit,
    };

    let (list, total) = reminders::list_reminders(&conn, &user.id, &filter)?;
    let page = filter.page.unwrap_or(1).max(1);
    let limit = filter.limit.unwrap_or(20).clamp(1, 100);
    let total_pages = if total == 0 {
        1
    } else {
        (total as f64 / limit as f64).ceil() as u64
    };

    Ok(Json(json!({
        "reminders": list,
        "pagination": {
            "page": page,
            "limit": limit,
            "total": total,
            "total_pages": total_pages,
        }
    })))
}

pub async fn create_reminder(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(mut req): Json<CreateReminderRequest>,
) -> Result<Response> {
    if req.title.trim().is_empty() {
        return Err(AppError::BadRequest("Reminder title is required".into()));
    }
    if req.remind_at.trim().is_empty() {
        return Err(AppError::BadRequest(
            "remind_at timestamp is required".into(),
        ));
    }

    // Convert recurring presets to cron expression if needed
    if req.is_recurring && req.cron_expr.is_none() {
        if let Some(recur) = req.recur_type {
            req.cron_expr = match recur {
                RecurType::Daily => Some("0 9 * * *".to_string()),
                RecurType::Weekly => Some("0 9 * * 1".to_string()),
                RecurType::Monthly => Some("0 9 1 * *".to_string()),
                RecurType::Custom => None,
            };
        }
    }

    // Validate cron expression
    if req.is_recurring {
        if let Some(ref expr) = req.cron_expr {
            if compute_next_cron_trigger(expr).is_none() {
                return Err(AppError::BadRequest(format!(
                    "Invalid cron expression: {expr}"
                )));
            }
        }
    }

    let conn = state.pool.get()?;
    let reminder = reminders::create_reminder(&conn, &user.id, req)?;

    Ok((StatusCode::CREATED, Json(reminder)).into_response())
}

pub async fn get_reminder(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let reminder = reminders::get_reminder_by_id(&conn, &user.id, &id)?
        .ok_or_else(|| AppError::NotFound(format!("Reminder {id} not found")))?;

    Ok(Json(reminder))
}

pub async fn update_reminder(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateReminderRequest>,
) -> Result<impl IntoResponse> {
    if let Some(true) = req.is_recurring {
        if let Some(ref expr) = req.cron_expr {
            if compute_next_cron_trigger(expr).is_none() {
                return Err(AppError::BadRequest(format!(
                    "Invalid cron expression: {expr}"
                )));
            }
        }
    }

    let conn = state.pool.get()?;
    let updated = reminders::update_reminder(&conn, &user.id, &id, req)?;

    Ok(Json(updated))
}

pub async fn delete_reminder(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    reminders::delete_reminder(&conn, &user.id, &id)?;

    Ok(Json(json!({
        "status": "deleted",
        "id": id,
    })))
}

pub async fn snooze_reminder(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    Json(req): Json<SnoozeReminderRequest>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let snoozed = reminders::snooze_reminder(&conn, &user.id, &id, req.duration_minutes)?;

    Ok(Json(snoozed))
}

pub async fn acknowledge_reminder(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    Json(req): Json<AcknowledgeReminderRequest>,
) -> Result<impl IntoResponse> {
    let conn = state.pool.get()?;
    let acknowledged = reminders::acknowledge_reminder(&conn, &user.id, &id, &req.action)?;

    Ok(Json(acknowledged))
}
