use evanotes::{
    auth::OtpRateLimiter, create_app, db::init_db, services::run_reminder_engine, AppConfig,
    AppState, NotificationHub, PresenceHub,
};
use std::time::Duration;
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
    let hub = NotificationHub::new();

    // Spawn background reminder scheduler (polling every 10 seconds)
    let engine_pool = pool.clone();
    let engine_hub = hub.clone();
    let engine_config = config.clone();
    tokio::spawn(async move {
        run_reminder_engine(
            engine_pool,
            engine_hub,
            engine_config,
            Duration::from_secs(10),
        )
        .await;
    });

    let state = AppState {
        pool,
        config: config.clone(),
        otp_limiter,
        hub,
        presence: PresenceHub::new(),
    };

    let app = create_app(state);
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on http://{}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
