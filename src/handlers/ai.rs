use crate::auth::AuthUser;
use crate::error::{AppError, Result};
use crate::models::ai::AiRewriteRequest;
use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use serde_json::json;

pub async fn rewrite_handler(
    State(state): State<AppState>,
    AuthUser(_user): AuthUser,
    Json(req): Json<AiRewriteRequest>,
) -> Result<impl IntoResponse> {
    if state.config.ai_api_key.is_none() {
        return Err(AppError::BadRequest(
            "AI is not enabled on this server (set AI_API_KEY + AI_MODELS)".into(),
        ));
    }

    if !crate::models::ai::AI_STYLES
        .iter()
        .any(|(id, _)| *id == req.style)
    {
        return Err(AppError::BadRequest(format!(
            "Unknown style '{}'",
            req.style
        )));
    }

    let rewritten =
        crate::services::ai::rewrite_text(&state.config, &req.text, &req.model, &req.style).await?;

    Ok(Json(json!({
        "rewritten": rewritten,
        "model": req.model,
        "style": req.style,
    })))
}
