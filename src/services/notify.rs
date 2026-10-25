use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::models::reminder::Reminder;
use lettre::{
    message::header::ContentType, transport::smtp::authentication::Credentials, Message,
    SmtpTransport, Transport,
};

/// Whether SMTP-based email delivery is configured.
pub fn email_configured(config: &AppConfig) -> bool {
    config.smtp_host.is_some()
        && config.smtp_user.is_some()
        && config.smtp_password.is_some()
        && config.smtp_from.is_some()
}

/// Whether Telegram delivery is configured.
pub fn telegram_configured(config: &AppConfig) -> bool {
    config.telegram_bot_token.is_some() && config.telegram_chat_id.is_some()
}

/// Build an SMTP transport with the correct TLS mode for the configured port:
/// implicit TLS (SMTPS) on 465, STARTTLS everywhere else (typically 587).
pub fn build_smtp_transport(config: &AppConfig) -> Result<SmtpTransport> {
    let host = config.smtp_host.as_deref().unwrap_or_default();
    let user = config.smtp_user.as_deref().unwrap_or_default();
    let password = config.smtp_password.as_deref().unwrap_or_default();

    let builder = if config.smtp_port == 465 {
        SmtpTransport::relay(host)
    } else {
        SmtpTransport::starttls_relay(host)
    }
    .map_err(|e| AppError::Internal(format!("Failed to configure SMTP relay: {e}")))?;

    Ok(builder
        .port(config.smtp_port)
        .credentials(Credentials::new(user.to_string(), password.to_string()))
        .build())
}

fn reminder_body(reminder: &Reminder) -> String {
    let mut body = format!("⏰ evaNotes Reminder\n\n📌 {}\n", reminder.title);
    if let Some(desc) = reminder
        .description
        .as_ref()
        .filter(|d| !d.trim().is_empty())
    {
        body.push_str(&format!("\n{desc}\n"));
    }
    body.push_str(&format!("\nScheduled for: {}\n", reminder.remind_at));
    if reminder.is_recurring {
        if let Some(cron) = reminder.cron_expr.as_ref() {
            body.push_str(&format!("Repeats: {cron}\n"));
        }
    }
    body
}

/// Send a reminder by email to the user's login email.
pub async fn send_email_reminder(
    config: &AppConfig,
    to_email: &str,
    reminder: &Reminder,
) -> Result<()> {
    if !email_configured(config) {
        return Err(AppError::Internal(
            "Email reminder requested but SMTP is not configured".into(),
        ));
    }

    let (from,) = (config.smtp_from.as_deref().unwrap_or_default(),);

    let email = Message::builder()
        .from(
            from.parse()
                .map_err(|e| AppError::Internal(format!("Invalid SMTP from address: {e}")))?,
        )
        .to(to_email
            .parse()
            .map_err(|e| AppError::Internal(format!("Invalid recipient email: {e}")))?)
        .subject(format!("evaNotes Reminder: {}", reminder.title))
        .header(ContentType::TEXT_PLAIN)
        .body(reminder_body(reminder))
        .map_err(|e| AppError::Internal(format!("Failed to build email message: {e}")))?;

    let mailer = build_smtp_transport(config)?;

    mailer
        .send(&email)
        .map_err(|e| AppError::Internal(format!("Failed to send SMTP email: {e}")))?;

    tracing::info!("Sent email reminder '{}' to {}", reminder.title, to_email);
    Ok(())
}

/// Send a reminder via the Telegram Bot API.
pub async fn send_telegram_reminder(
    config: &AppConfig,
    reminder: &Reminder,
    chat_id_override: Option<&str>,
) -> Result<()> {
    if !telegram_configured(config) {
        return Err(AppError::Internal(
            "Telegram reminder requested but TELEGRAM_BOT_TOKEN / TELEGRAM_CHAT_ID is not set"
                .into(),
        ));
    }

    let token = config.telegram_bot_token.as_deref().unwrap_or_default();
    let chat_id = chat_id_override
        .filter(|s| !s.trim().is_empty())
        .or(config.telegram_chat_id.as_deref())
        .unwrap_or_default();

    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let text = reminder_body(reminder);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Internal(format!("Failed to build HTTP client: {e}")))?;

    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "disable_web_page_preview": true,
        }))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to reach Telegram API: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(
            "Telegram sendMessage failed ({}): {}",
            status,
            body.chars().take(200).collect::<String>()
        );
        return Err(AppError::Internal(format!(
            "Telegram API returned {status}"
        )));
    }

    tracing::info!(
        "Sent Telegram reminder '{}' to chat {}",
        reminder.title,
        chat_id
    );
    Ok(())
}
