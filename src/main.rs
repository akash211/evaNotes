use evanotes::{auth::OtpRateLimiter, create_app, db::init_db, AppConfig, AppState};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::from_env();

    // Initialize structured tracing
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.rust_log)),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!(
        "Starting evaNotes v{} on {}:{}",
        env!("CARGO_PKG_VERSION"),
        config.host,
        config.port
    );

    // Initialize database pool & migrations
    let pool = init_db(&config.db_path)?;
    let otp_limiter = OtpRateLimiter::new();

    let state = AppState {
        pool,
        config: config.clone(),
        otp_limiter,
    };

    let app = create_app(state);
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on http://{}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
