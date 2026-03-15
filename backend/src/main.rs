mod api;
mod blockchain;
mod db;
mod models;
mod services;

use anyhow::Result;
use axum::{Router, http::Method};
use dotenvy::dotenv;
use std::sync::Arc;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    blockchain::BscClient,
    db::{Database, RedisPool},
};

/// Shared application state — cloned cheaply via Arc
#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub redis: RedisPool,
    pub bsc: Arc<BscClient>,
    pub jwt_secret: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    // Tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "backend=debug,tower_http=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("🚀 Starting Yeet API server");

    // Config from env
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let bsc_rpc = std::env::var("BSC_RPC_URL")
        .unwrap_or_else(|_| "https://bsc-dataseed.binance.org/".into());
    let jwt_secret = std::env::var("JWT_SECRET")
        .expect("JWT_SECRET must be set");
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse::<u16>()?;

    // Init connections
    let db = Database::connect(&database_url).await?;
    db.migrate().await?;

    let redis = RedisPool::new(&redis_url)?;
    let bsc = Arc::new(BscClient::new(&bsc_rpc).await?);

    let state = AppState { db, redis, bsc, jwt_secret };

    // CORS — allow frontend origin
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    // Router
    let app = Router::new()
        .nest("/api/v1", api::router())
        .layer(cors)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("Listening on http://0.0.0.0:{port}");

    // Start background jobs
    tokio::spawn(crate::services::batch_rewards::start_reward_batch_job(state.clone()));
    tokio::spawn(crate::services::batch_rewards::start_cleanup_job(state.clone()));
    tokio::spawn(crate::services::webboard::start_webboard_sync(state.clone()));

    axum::serve(listener, app).await?;

    Ok(())
}
