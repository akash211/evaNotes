pub mod auth;

use crate::auth::AuthUser;
use axum::{response::IntoResponse, Json};
use serde_json::json;

pub async fn protected_me(AuthUser(user): AuthUser) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "user_id": user.id,
        "email": user.email,
        "is_admin": user.is_admin,
    }))
}
