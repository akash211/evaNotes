use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

pub fn generate_signed_token(user_id: &str, secret: &str) -> String {
    let nonce = Uuid::new_v4().to_string();
    let payload = format!("{}:{}", user_id, nonce);

    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    format!("{}.{}", payload, signature)
}

pub fn verify_signed_token(token: &str, secret: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return None;
    }

    let payload = parts[0];
    let signature_hex = parts[1];

    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    let expected_sig = hex::encode(mac.finalize().into_bytes());

    if signature_hex
        .as_bytes()
        .ct_eq(expected_sig.as_bytes())
        .into()
    {
        let payload_parts: Vec<&str> = payload.split(':').collect();
        if !payload_parts.is_empty() {
            return Some(payload_parts[0].to_string());
        }
    }

    None
}

pub fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    // 1. Try Cookie header: session=<token>
    if let Some(cookie_header) = headers.get(axum::http::header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for part in cookie_str.split(';') {
                let trimmed = part.trim();
                if let Some(token) = trimmed.strip_prefix("session=") {
                    let clean_token = token.trim();
                    if !clean_token.is_empty() {
                        return Some(clean_token.to_string());
                    }
                }
            }
        }
    }

    // 2. Try Authorization: Bearer <token>
    if let Some(auth_header) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                let clean_token = token.trim();
                if !clean_token.is_empty() {
                    return Some(clean_token.to_string());
                }
            }
        }
    }

    None
}

pub fn build_session_cookie(token: &str, secure: bool, max_age_days: i64) -> String {
    let max_age_secs = max_age_days * 86400;
    let mut cookie = format!(
        "session={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        token, max_age_secs
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub fn build_logout_cookie(secure: bool) -> String {
    let mut cookie = "session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT".to_string();
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

// Hex encoding helper
mod hex {
    pub fn encode(data: impl AsRef<[u8]>) -> String {
        data.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}
