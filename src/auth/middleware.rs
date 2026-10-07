use crate::auth::session::{extract_session_token, verify_signed_token};
use crate::db::users::get_session_with_user;
use crate::error::AppError;
use crate::models::user::User;
use crate::AppState;
use axum::{extract::FromRequestParts, http::request::Parts};

#[derive(Debug, Clone)]
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_session_token(&parts.headers).ok_or_else(|| {
            AppError::Unauthorized("Authentication required. Please sign in.".into())
        })?;

        // 1. Verify HMAC signature
        if verify_signed_token(&token, &state.config.session_secret).is_none() {
            return Err(AppError::Unauthorized(
                "Invalid or tampered session token.".into(),
            ));
        }

        // 2. Verify database session validity & expiry
        let conn = state.pool.get()?;
        let session_data = get_session_with_user(&conn, &token)?;

        match session_data {
            Some((_session, user)) => Ok(AuthUser(user)),
            None => Err(AppError::Unauthorized("Session expired or revoked.".into())),
        }
    }
}
