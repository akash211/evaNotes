use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthMethod {
    #[serde(rename = "otp")]
    Otp,
    #[serde(rename = "google")]
    Google,
}

impl AuthMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Otp => "otp",
            Self::Google => "google",
        }
    }

    pub fn from_str_opt(s: &str) -> Self {
        match s {
            "google" => Self::Google,
            _ => Self::Otp,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub auth_method: String,
    pub is_admin: bool,
    pub created_at: String,
    pub last_login: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub user_id: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtpRecord {
    pub id: i64,
    pub email: String,
    pub code: String,
    pub created_at: String,
    pub expires_at: String,
    pub used: bool,
    pub attempts: i32,
}
