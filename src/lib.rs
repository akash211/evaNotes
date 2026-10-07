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
pub use services::NotificationHub;

use axum::{
    routing::{get, post, put},
    Router,
};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub config: AppConfig,
    pub otp_limiter: OtpRateLimiter,
    pub hub: NotificationHub,
}

pub fn create_app(state: AppState) -> Router {
    let static_dir = if std::path::Path::new("src/web/static").is_dir() {
        "src/web/static"
    } else if std::path::Path::new("web/static").is_dir() {
        "web/static"
    } else {
        "static"
    };

    let serve_dir =
        ServeDir::new(static_dir).fallback(ServeFile::new(format!("{}/index.html", static_dir)));

    Router::new()
        // Auth endpoints
        .route("/api/auth/status", get(handlers::auth::auth_status))
        .route("/api/auth/otp/request", post(handlers::auth::request_otp))
        .route("/api/auth/otp/verify", post(handlers::auth::verify_otp))
        .route("/api/auth/google", post(handlers::auth::google_login))
        .route("/api/auth/logout", post(handlers::auth::logout))
        .route("/api/auth/me", get(handlers::protected_me))
        // Notes endpoints
        .route(
            "/api/notes",
            get(handlers::notes::list_notes).post(handlers::notes::create_note),
        )
        .route(
            "/api/notes/{id}",
            get(handlers::notes::get_note)
                .put(handlers::notes::update_note)
                .delete(handlers::notes::delete_note),
        )
        .route("/api/notes/{id}/pin", put(handlers::notes::toggle_pin))
        .route(
            "/api/notes/{id}/archive",
            put(handlers::notes::toggle_archive),
        )
        .route("/api/notes/{id}/export", get(handlers::notes::export_note))
        // Reminders endpoints
        .route(
            "/api/reminders",
            get(handlers::reminders::list_reminders).post(handlers::reminders::create_reminder),
        )
        .route(
            "/api/reminders/{id}",
            get(handlers::reminders::get_reminder)
                .put(handlers::reminders::update_reminder)
                .delete(handlers::reminders::delete_reminder),
        )
        .route(
            "/api/reminders/{id}/snooze",
            post(handlers::reminders::snooze_reminder),
        )
        .route(
            "/api/reminders/{id}/acknowledge",
            post(handlers::reminders::acknowledge_reminder),
        )
        // WebSocket endpoint
        .route("/ws", get(handlers::ws::ws_handler))
        .fallback_service(serve_dir)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
