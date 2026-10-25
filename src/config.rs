use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub rust_log: String,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub admin_email: Option<String>,
    pub session_secret: String,
    pub google_client_id: Option<String>,
    pub secure_cookies: bool,
    pub enforce_https: bool,
    pub smtp_host: Option<String>,
    pub smtp_port: u16,
    pub smtp_user: Option<String>,
    pub smtp_password: Option<String>,
    pub smtp_from: Option<String>,
    pub telegram_bot_token: Option<String>,
    pub telegram_chat_id: Option<String>,
    /// Max OTP request attempts per IP per minute.
    pub otp_rate_limit_max: u32,
    /// AI assistant: OpenAI-compatible API base URL.
    pub ai_api_base: String,
    /// AI assistant: API key (None = AI disabled).
    pub ai_api_key: Option<String>,
    /// AI assistant: allowed model ids.
    pub ai_models: Vec<String>,
    /// AI assistant: default model id.
    pub ai_default_model: String,
    /// Version history: minimum seconds between snapshots.
    pub version_min_interval_secs: i64,
    /// Version history: max snapshots kept per note.
    pub version_max_keep: usize,
    pub cors_origins: Vec<String>,
}

impl AppConfig {
    pub fn from_env() -> Self {
        // Attempt to load .env if present
        let _ = dotenvy::dotenv();

        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);
        let rust_log = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

        let data_dir = if let Ok(val) = std::env::var("EVA_DATA_DIR") {
            PathBuf::from(val)
        } else if std::path::Path::new("/data").exists() {
            PathBuf::from("/data")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".evanotes")
        } else {
            PathBuf::from("./data")
        };

        let db_path = data_dir.join("evanotes.db");

        let admin_email = std::env::var("ADMIN_EMAIL")
            .ok()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        let session_secret = std::env::var("SESSION_SECRET")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                // Generate a random 32-byte secret if not provided
                use rand::RngCore;
                let mut bytes = [0u8; 32];
                rand::rngs::OsRng.fill_bytes(&mut bytes);
                let hex_secret = hex_encode(&bytes);
                tracing::info!("No SESSION_SECRET provided; generated ephemeral session key");
                hex_secret
            });

        let google_client_id = std::env::var("GOOGLE_CLIENT_ID")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let secure_cookies = std::env::var("SECURE_COOKIES")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        let enforce_https = std::env::var("ENFORCE_HTTPS")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        let smtp_host = std::env::var("SMTP_HOST")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let smtp_port = std::env::var("SMTP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(587);
        let smtp_user = std::env::var("SMTP_USER")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let smtp_password = std::env::var("SMTP_PASSWORD")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let smtp_from = std::env::var("SMTP_FROM")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let telegram_bot_token = std::env::var("TELEGRAM_BOT_TOKEN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let telegram_chat_id = std::env::var("TELEGRAM_CHAT_ID")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let otp_rate_limit_max = std::env::var("OTP_RATE_LIMIT_MAX")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(5);

        let ai_api_base = std::env::var("AI_API_BASE")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());

        let ai_api_key = std::env::var("AI_API_KEY")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let ai_models: Vec<String> = std::env::var("AI_MODELS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let ai_default_model = std::env::var("AI_DEFAULT_MODEL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                ai_models
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "nvidia/nemotron-3.5-super-vl:free".to_string())
            });

        let version_min_interval_secs = std::env::var("VERSION_MIN_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(900);

        let version_max_keep = std::env::var("VERSION_MAX_KEEP")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(10);

        let cors_origins: Vec<String> = std::env::var("CORS_ORIGINS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        Self {
            host,
            port,
            rust_log,
            data_dir,
            db_path,
            admin_email,
            session_secret,
            google_client_id,
            secure_cookies,
            enforce_https,
            smtp_host,
            smtp_port,
            smtp_user,
            smtp_password,
            smtp_from,
            telegram_bot_token,
            telegram_chat_id,
            otp_rate_limit_max,
            ai_api_base,
            ai_api_key,
            ai_models,
            ai_default_model,
            version_min_interval_secs,
            version_max_keep,
            cors_origins,
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
