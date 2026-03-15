//! Yeet Social Media — Production API Server
//! Web + Android + iOS compatible REST API
//! BSC blockchain integration, JWT wallet auth, PostgreSQL, Redis

use std::{net::SocketAddr, time::Duration};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::{get, post, delete, patch}, Json, Router};
use serde_json::json;
use tower::ServiceBuilder;
use tower_http::{cors::{Any, CorsLayer}, timeout::TimeoutLayer, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod api;
mod db;
mod error;
mod models;
mod services;
mod state;

pub use error::{AppError, AppResult};
pub use state::AppState;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "🚀 Yeet API starting");

    let state = AppState::from_env().await.expect("Failed to initialise state");
    state.db.run_migrations().await.expect("DB migrations failed");
    info!("✅ Migrations applied");

    let app = build_router(state);

    let port: u16 = std::env::var("PORT").unwrap_or_else(|_| "8080".into()).parse().unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!(address = %addr, "🌐 Listening");

    let listener = tokio::net::TcpListener::bind(addr).await.expect("Bind failed");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await.expect("Server error");
    info!("🛑 Graceful shutdown complete");
}

fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .max_age(Duration::from_secs(86400));

    Router::new()
        // Health
        .route("/api/v1/health",           get(health_handler))
        .route("/api/v1/version",          get(version_handler))
        // Auth — wallet login (web + Android + iOS)
        .route("/api/v1/auth/nonce",       post(api::auth::get_nonce))
        .route("/api/v1/auth/verify",      post(api::auth::verify_signature))
        .route("/api/v1/auth/refresh",     post(api::auth::refresh_token))
        // Feed
        .route("/api/v1/feed",             get(api::feed::get_feed))
        .route("/api/v1/feed/following",   get(api::feed::get_following_feed))
        // Posts
        .route("/api/v1/posts",            post(api::posts::create_post))
        .route("/api/v1/posts/:id",        get(api::posts::get_post))
        .route("/api/v1/posts/:id",        delete(api::posts::delete_post))
        .route("/api/v1/posts/:id/like",   post(api::posts::like_post))
        .route("/api/v1/posts/:id/reshare",post(api::posts::reshare_post))
        .route("/api/v1/posts/:id/comments", get(api::posts::get_comments))
        .route("/api/v1/posts/:id/comments", post(api::posts::add_comment))
        .route("/api/v1/posts/:id/nft",    post(api::posts::mint_nft))
        // Users
        .route("/api/v1/users/me",         get(api::users::get_my_profile))
        .route("/api/v1/users/me",         patch(api::users::update_profile))
        .route("/api/v1/users/:address",   get(api::users::get_profile))
        .route("/api/v1/users/:address/follow",   post(api::users::follow_user))
        .route("/api/v1/users/:address/unfollow", post(api::users::unfollow_user))
        // Tips & Tokens
        .route("/api/v1/tips",             post(api::tips::send_tip))
        .route("/api/v1/tokens/balance",   get(api::tokens::get_balance))
        .route("/api/v1/tokens/rewards",   get(api::tokens::get_rewards))
        .layer(ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
            .layer(cors)
            .layer(TimeoutLayer::new(Duration::from_secs(30))))
        .with_state(state)
}

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = state.db.ping().await.is_ok();
    let cache_ok = state.cache.ping().await.is_ok();
    let status = if db_ok && cache_ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(json!({
        "status": if db_ok && cache_ok { "ok" } else { "degraded" },
        "checks": { "database": db_ok, "cache": cache_ok },
        "version": env!("CARGO_PKG_VERSION"),
        "platforms": ["web", "android", "ios"]
    })))
}

async fn version_handler() -> Json<serde_json::Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION"), "name": "yeet-api" }))
}

async fn shutdown_signal() {
    let ctrl_c = async { tokio::signal::ctrl_c().await.expect("CTRL+C failed") };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM failed").recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("backend=info,tower_http=warn,sqlx=warn"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}
