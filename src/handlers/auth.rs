use crate::auth::otp::{generate_otp_code, send_otp_email, verify_otp_code};
use crate::auth::session::{
    build_logout_cookie, build_session_cookie, extract_session_token, generate_signed_token,
    verify_signed_token,
};
use crate::db::users::{
    create_or_get_user, create_otp, create_session, delete_session, get_latest_active_otp,
    get_session_with_user, increment_otp_attempts, mark_otp_used, update_last_login,
};
use crate::error::{AppError, Result};
use crate::AppState;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct OtpRequestPayload {
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct OtpVerifyPayload {
    pub email: String,
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct GoogleLoginPayload {
    pub id_token: String,
}

/// Resolve the real client IP. Behind Cloudflare Tunnel the trustworthy value is
/// `CF-Connecting-IP`; `X-Forwarded-For` is only a fallback (its *last* entry is
/// the one appended by the nearest proxy — earlier entries are client-controlled).
fn client_ip(headers: &HeaderMap) -> String {
    for name in ["cf-connecting-ip", "x-real-ip"] {
        if let Some(ip) = headers
            .get(name)
            .and_then(|h| h.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return ip.to_string();
        }
    }
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|h| h.to_str().ok()) {
        if let Some(last) = xff.split(',').map(str::trim).rfind(|s| !s.is_empty()) {
            return last.to_string();
        }
    }
    "127.0.0.1".to_string()
}

pub async fn request_otp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<OtpRequestPayload>,
) -> Result<Response> {
    let email = payload.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::BadRequest(
            "Valid email address is required".into(),
        ));
    }

    // Determine client IP for rate limiting
    let client_ip = client_ip(&headers);

    // Check rate limiter: max N requests per 60 seconds (configurable)
    state.otp_limiter.check_and_record(
        &client_ip,
        state.config.otp_rate_limit_max as usize,
        Duration::from_secs(60),
    )?;

    // Check admin authorization if ADMIN_EMAIL is set
    if let Some(admin) = &state.config.admin_email {
        if !email.eq_ignore_ascii_case(admin) {
            tracing::warn!("Unauthorized login attempt for email: {}", email);
            return Err(AppError::Unauthorized(
                "This email is not authorized to access this instance.".into(),
            ));
        }
    }

    let code = generate_otp_code();
    // Stored in UTC "YYYY-MM-DD HH:MM:SS" so it compares exactly with SQLite datetime('now')
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(5))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    {
        let conn = state.pool.get()?;
        create_otp(&conn, &email, &code, &expires_at)?;
    }

    send_otp_email(&state.config, &email, &code).await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "otp_sent",
            "message": "A 6-digit verification code has been dispatched."
        })),
    )
        .into_response())
}

pub async fn verify_otp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<OtpVerifyPayload>,
) -> Result<Response> {
    let email = payload.email.trim().to_lowercase();
    let input_code = payload.code.trim();

    if email.is_empty() || input_code.is_empty() {
        return Err(AppError::BadRequest(
            "Email and verification code are required".into(),
        ));
    }

    // Throttle guessing attempts per client IP (separate namespace from OTP sends)
    state.otp_limiter.check_and_record(
        &format!("verify:{}", client_ip(&headers)),
        10,
        Duration::from_secs(60),
    )?;

    if let Some(admin) = &state.config.admin_email {
        if !email.eq_ignore_ascii_case(admin) {
            return Err(AppError::Unauthorized(
                "This email is not authorized to access this instance.".into(),
            ));
        }
    }

    let conn = state.pool.get()?;
    let otp_record = get_latest_active_otp(&conn, &email)?
        .ok_or_else(|| AppError::Unauthorized("Invalid or expired verification code.".into()))?;

    if otp_record.attempts >= 5 {
        return Err(AppError::Forbidden(
            "Maximum failed attempts reached. Please request a new code.".into(),
        ));
    }

    if !verify_otp_code(input_code, &otp_record.code) {
        let attempts = increment_otp_attempts(&conn, otp_record.id)?;
        let remaining = 5 - attempts;
        return Err(AppError::Unauthorized(format!(
            "Incorrect verification code. {} attempts remaining.",
            remaining.max(0)
        )));
    }

    mark_otp_used(&conn, otp_record.id)?;

    let is_admin = state
        .config
        .admin_email
        .as_ref()
        .map(|a| a.eq_ignore_ascii_case(&email))
        .unwrap_or(true);

    let user = create_or_get_user(&conn, &email, "otp", is_admin)?;
    update_last_login(&conn, &user.id)?;

    let token = generate_signed_token(&user.id, &state.config.session_secret);
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(30))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    create_session(&conn, &token, &user.id, &expires_at)?;

    let cookie = build_session_cookie(&token, state.config.secure_cookies, 30);

    let mut response = (
        StatusCode::OK,
        Json(json!({
            "status": "authenticated",
            "email": user.email,
            "auth_method": "otp",
            "is_admin": user.is_admin,
            "user_id": user.id,
            "token": token
        })),
    )
        .into_response();

    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());

    Ok(response)
}

pub async fn google_login(
    State(state): State<AppState>,
    Json(payload): Json<GoogleLoginPayload>,
) -> Result<Response> {
    let email =
        crate::auth::google::verify_google_id_token(&payload.id_token, &state.config).await?;

    if let Some(admin) = &state.config.admin_email {
        if !email.eq_ignore_ascii_case(admin) {
            return Err(AppError::Unauthorized(
                "This Google account is not authorized to access this instance.".into(),
            ));
        }
    }

    let conn = state.pool.get()?;
    let is_admin = state
        .config
        .admin_email
        .as_ref()
        .map(|a| a.eq_ignore_ascii_case(&email))
        .unwrap_or(true);

    let user = create_or_get_user(&conn, &email, "google", is_admin)?;
    update_last_login(&conn, &user.id)?;

    let token = generate_signed_token(&user.id, &state.config.session_secret);
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(30))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    create_session(&conn, &token, &user.id, &expires_at)?;

    let cookie = build_session_cookie(&token, state.config.secure_cookies, 30);

    let mut response = (
        StatusCode::OK,
        Json(json!({
            "status": "authenticated",
            "email": user.email,
            "auth_method": "google",
            "is_admin": user.is_admin,
            "user_id": user.id,
            "token": token
        })),
    )
        .into_response();

    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());

    Ok(response)
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    if let Some(token) = extract_session_token(&headers) {
        let conn = state.pool.get()?;
        let _ = delete_session(&conn, &token);
    }

    let cookie = build_logout_cookie(state.config.secure_cookies);

    let mut response = (
        StatusCode::OK,
        Json(json!({
            "status": "logged_out",
            "message": "Successfully signed out"
        })),
    )
        .into_response();

    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());

    Ok(response)
}

pub async fn auth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    let mut authenticated = false;
    let mut email: Option<String> = None;
    let mut is_admin = false;
    let mut user_id: Option<String> = None;

    if let Some(token) = extract_session_token(&headers) {
        if verify_signed_token(&token, &state.config.session_secret).is_some() {
            if let Ok(conn) = state.pool.get() {
                if let Ok(Some((_session, user))) = get_session_with_user(&conn, &token) {
                    authenticated = true;
                    email = Some(user.email);
                    is_admin = user.is_admin;
                    user_id = Some(user.id);
                }
            }
        }
    }

    Ok(Json(json!({
        "status": "ok",
        "app": "evaNotes",
        "version": env!("CARGO_PKG_VERSION"),
        "authenticated": authenticated,
        "email": email,
        "is_admin": is_admin,
        "user_id": user_id,
        "google_login_enabled": state.config.google_client_id.is_some(),
        "google_client_id": state.config.google_client_id,
        "telegram_enabled": crate::services::notify::telegram_configured(&state.config),
        "email_enabled": crate::services::notify::email_configured(&state.config),
        "ai_enabled": crate::services::ai::ai_configured(&state.config),
        "ai_models": state.config.ai_models,
        "ai_default_model": state.config.ai_default_model,
        "ai_styles": crate::models::ai::AI_STYLES.iter().map(|(id, label)| json!({"id": id, "label": label})).collect::<Vec<_>>()
    })))
}
