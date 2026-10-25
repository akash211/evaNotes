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
pub use services::{NotificationHub, PresenceHub};

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
    let path = req.uri().path().to_string();
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
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' https://accounts.google.com https://cdn.jsdelivr.net; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com https://cdn.jsdelivr.net; font-src 'self' https://fonts.gstatic.com data:; img-src 'self' data:; connect-src 'self' ws: wss:; frame-src https://accounts.google.com; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none';",
        ),
    );

    // API responses carry private notes: never let browsers/proxies cache them.
    if path.starts_with("/api/") {
        headers.insert(
            HeaderName::from_static("cache-control"),
            HeaderValue::from_static("no-store"),
        );
    }

    // Public share pages are unlisted links: keep them out of search engines.
    if path.starts_with("/share/") {
        headers.insert(
            HeaderName::from_static("x-robots-tag"),
            HeaderValue::from_static("noindex, nofollow"),
        );
    }

    response
}

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub config: AppConfig,
    pub otp_limiter: OtpRateLimiter,
    pub hub: NotificationHub,
    pub presence: PresenceHub,
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
        .route("/api/notes/{id}/tags", put(handlers::notes::set_note_tags))
        // Versions (auto-save history)
        .route(
            "/api/notes/{id}/versions",
            get(handlers::notes::list_versions),
        )
        .route(
            "/api/notes/{id}/versions/clear",
            post(handlers::notes::clear_versions),
        )
        .route(
            "/api/notes/{id}/restore",
            post(handlers::notes::restore_version),
        )
        // Public sharing
        .route(
            "/api/notes/{id}/share",
            get(handlers::notes::get_share)
                .post(handlers::notes::create_share)
                .delete(handlers::notes::delete_share),
        )
        // Multi-device presence
        .route(
            "/api/notes/{id}/presence",
            post(handlers::notes::note_presence_heartbeat)
                .delete(handlers::notes::note_presence_leave),
        )
        // Code highlighting preview (no persistence)
        .route("/api/highlight", post(handlers::notes::highlight_code))
        // AI assistant
        .route("/api/ai/rewrite", post(handlers::ai::rewrite_handler))
        // Folders
        .route(
            "/api/folders",
            get(handlers::folders::list_folders).post(handlers::folders::create_folder),
        )
        .route(
            "/api/folders/{id}",
            put(handlers::folders::update_folder).delete(handlers::folders::delete_folder),
        )
        // Public share pages (no auth)
        .route("/share/{slug}", get(handlers::public_share::share_page))
        .route(
            "/share/{slug}/md",
            get(handlers::public_share::share_markdown),
        )
        .route("/share/{slug}/pdf", get(handlers::public_share::share_pdf))
        // Tags endpoints
        .route(
            "/api/tags",
            get(handlers::tags::list_tags).post(handlers::tags::create_tag),
        )
        .route("/api/tags/suggest", get(handlers::tags::suggest_tags))
        .route(
            "/api/tags/{id}",
            put(handlers::tags::update_tag).delete(handlers::tags::delete_tag),
        )
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
