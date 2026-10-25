use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::services::notify::build_smtp_transport;
use lettre::{message::header::ContentType, Message, Transport};
use rand::Rng;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

pub fn generate_otp_code() -> String {
    let mut rng = rand::rngs::OsRng;
    let code: u32 = rng.gen_range(100_000..=999_999);
    code.to_string()
}

pub fn verify_otp_code(input: &str, stored: &str) -> bool {
    input
        .trim()
        .as_bytes()
        .ct_eq(stored.trim().as_bytes())
        .into()
}

#[derive(Clone, Default)]
pub struct OtpRateLimiter {
    // Stores (ip, Vec<Instant>)
    requests: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl OtpRateLimiter {
    pub fn new() -> Self {
        Self {
            requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn check_and_record(&self, ip: &str, max_requests: usize, window: Duration) -> Result<()> {
        let mut map = self
            .requests
            .lock()
            .map_err(|_| AppError::Internal("Rate limiter mutex poisoned".into()))?;

        let now = Instant::now();
        let timestamps = map.entry(ip.to_string()).or_default();

        // Retain timestamps within the sliding window
        timestamps.retain(|&t| now.duration_since(t) < window);

        if timestamps.len() >= max_requests {
            let oldest = timestamps[0];
            let elapsed = now.duration_since(oldest);
            let retry_after = window.saturating_sub(elapsed).as_secs().max(1);
            return Err(AppError::RateLimited {
                retry_after_secs: retry_after,
            });
        }

        timestamps.push(now);
        Ok(())
    }
}

pub async fn send_otp_email(config: &AppConfig, to_email: &str, code: &str) -> Result<()> {
    if let (Some(_host), Some(_user), Some(_password), Some(from)) = (
        &config.smtp_host,
        &config.smtp_user,
        &config.smtp_password,
        &config.smtp_from,
    ) {
        let email = Message::builder()
            .from(from.parse().map_err(|e| {
                AppError::Internal(format!("Invalid SMTP from address: {e}"))
            })?)
            .to(to_email.parse().map_err(|e| {
                AppError::BadRequest(format!("Invalid recipient email: {e}"))
            })?)
            .subject("Your evaNotes Login Code")
            .header(ContentType::TEXT_PLAIN)
            .body(format!(
                "Hello,\n\nYour 6-digit login verification code is: {}\n\nThis code is valid for 5 minutes. Do not share it with anyone.",
                code
            ))
            .map_err(|e| AppError::Internal(format!("Failed to build email message: {e}")))?;

        let mailer = build_smtp_transport(config)?;

        mailer
            .send(&email)
            .map_err(|e| AppError::Internal(format!("Failed to send SMTP email: {e}")))?;

        tracing::info!("Sent login OTP email to {}", to_email);
    } else {
        // Fallback: prominent console output matching evaTorrent convention
        println!("\n=======================================================");
        println!("[evaNotes AUTH] Login OTP for {}: {}", to_email, code);
        println!("=======================================================\n");
        tracing::info!("[evaNotes AUTH] Login OTP for {}: {}", to_email, code);
    }

    Ok(())
}
