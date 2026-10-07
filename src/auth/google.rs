use crate::config::AppConfig;
use crate::error::{AppError, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GoogleTokenInfo {
    email: Option<String>,
    email_verified: Option<serde_json::Value>,
    aud: Option<String>,
    error_description: Option<String>,
}

pub async fn verify_google_id_token(id_token: &str, config: &AppConfig) -> Result<String> {
    if id_token.trim().is_empty() {
        return Err(AppError::BadRequest("ID token cannot be empty".into()));
    }

    let url = format!(
        "https://oauth2.googleapis.com/tokeninfo?id_token={}",
        urlencoding::encode(id_token.trim())
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Internal(format!("Failed to build HTTP client: {e}")))?;

    let resp = client.get(&url).send().await.map_err(|e| {
        AppError::Internal(format!("Failed to reach Google tokeninfo endpoint: {e}"))
    })?;

    if !resp.status().is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        tracing::warn!("Google token verification failed: {}", err_text);
        return Err(AppError::Unauthorized("Invalid Google ID token".into()));
    }

    let token_info: GoogleTokenInfo = resp.json().await.map_err(|e| {
        AppError::Internal(format!("Failed to parse Google tokeninfo response: {e}"))
    })?;

    if let Some(err) = token_info.error_description {
        return Err(AppError::Unauthorized(format!("Google auth error: {err}")));
    }

    // Verify audience if configured
    if let Some(configured_client_id) = &config.google_client_id {
        match &token_info.aud {
            Some(aud) if aud == configured_client_id => (),
            _ => {
                tracing::warn!(
                    "Google token aud mismatch: token aud={:?}, expected={:?}",
                    token_info.aud,
                    configured_client_id
                );
                return Err(AppError::Unauthorized(
                    "Google token audience does not match configured client ID".into(),
                ));
            }
        }
    }

    // Verify email_verified is true
    let is_verified = match token_info.email_verified {
        Some(serde_json::Value::Bool(b)) => b,
        Some(serde_json::Value::String(ref s)) => s == "true",
        _ => false,
    };

    if !is_verified {
        return Err(AppError::Unauthorized(
            "Google account email is not verified".into(),
        ));
    }

    let email = token_info
        .email
        .ok_or_else(|| AppError::Unauthorized("Google token missing email claim".into()))?
        .trim()
        .to_lowercase();

    Ok(email)
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut encoded = String::new();
        for byte in s.bytes() {
            match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(byte as char);
                }
                _ => {
                    encoded.push_str(&format!("%{:02X}", byte));
                }
            }
        }
        encoded
    }
}
