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
    extract::{DefaultBodyLimit, Request},
    http::{
        header::{HeaderName, HeaderValue},
        Method,
    },
    middleware::{self, Next},
    response::Response,
    routing::{get, post, put},
    Router,
};
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

async fn security_headers_middleware(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        HeaderName::from_static("x-xss-protection"),
        HeaderValue::from_static("0"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' https://accounts.google.com; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self' ws: wss:; frame-src https://accounts.google.com;",
        ),
    );

    response
}

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

    let mut cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any);

    if !state.config.cors_origins.is_empty() {
        let origins: Vec<HeaderValue> = state
            .config
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        cors = cors.allow_origin(origins);
    }

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
        .layer(middleware::from_fn(security_headers_middleware))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
