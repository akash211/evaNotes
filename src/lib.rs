pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod handlers;
pub mod models;
pub mod services;

pub use auth::OtpRateLimiter;
pub use config::AppConfig;
pub use db::DbPool;
pub use error::{AppError, Result};

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub config: AppConfig,
    pub otp_limiter: OtpRateLimiter,
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        // Auth endpoints
        .route("/api/auth/status", get(handlers::auth::auth_status))
        .route("/api/auth/otp/request", post(handlers::auth::request_otp))
        .route("/api/auth/otp/verify", post(handlers::auth::verify_otp))
        .route("/api/auth/google", post(handlers::auth::google_login))
        .route("/api/auth/logout", post(handlers::auth::logout))
        .route("/api/auth/me", get(handlers::protected_me))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
