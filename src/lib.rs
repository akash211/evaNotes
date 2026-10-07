pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod handlers;
pub mod models;
pub mod services;

pub use config::AppConfig;
pub use error::{AppError, Result};

use axum::{routing::get, Router};
use tower_http::trace::TraceLayer;

pub fn create_app(_config: AppConfig) -> Router {
    Router::new()
        .route("/api/auth/status", get(handlers::health_check))
        .layer(TraceLayer::new_for_http())
}
