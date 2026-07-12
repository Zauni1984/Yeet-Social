#![allow(unused_imports, dead_code, unused_variables, unused_mut)]
//! Yeet Social Media — Production API Server
//! Web + Android + iOS compatible REST API
//! BSC blockchain integration, JWT wallet auth, PostgreSQL, Redis

use std::{net::SocketAddr, time::Duration};
use axum::{extract::{DefaultBodyLimit, State}, http::StatusCode, response::IntoResponse, routing::{get, post, delete, patch}, Json, Router};
use serde_json::json;
use tower::ServiceBuilder;
use tower_http::{cors::{Any, CorsLayer}, services::ServeDir, timeout::TimeoutLayer, trace::TraceLayer};
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

    let app = build_router(state.clone());

    let port: u16 = std::env::var("PORT").unwrap_or_else(|_| "8080".into()).parse().unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!(address = %addr, "🌐 Listening");

    let listener = tokio::net::TcpListener::bind(addr).await.expect("Bind failed");
    // Start background jobs
    tokio::spawn(services::batch_rewards::start_reward_batch_job(state.clone()));
    tokio::spawn(services::batch_rewards::start_cleanup_job(state.clone()));
    tokio::spawn(services::batch_rewards::start_message_cleanup_job(state.clone()));
    tokio::spawn(services::batch_rewards::start_scheduled_publish_job(state.clone()));
    tokio::spawn(services::batch_rewards::start_lives_sweep_job(state.clone()));
    info!(" Background jobs started (batch rewards + cleanup + message-cleanup)");

    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await.expect("Server error");
    info!("🛑 Graceful shutdown complete");
}

fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .max_age(Duration::from_secs(86400));

    let uploads_dir = api::uploads::uploads_dir();
    if let Err(e) = std::fs::create_dir_all(&uploads_dir) {
        tracing::warn!(?e, dir = ?uploads_dir, "failed to ensure uploads dir exists");
    }
    Router::new()
        // Static uploaded media (avatars, covers)
        .nest_service("/uploads", ServeDir::new(uploads_dir))
        // Health
        .route("/api/v1/link-preview",  get(api::link_preview::get_link_preview))
        .route("/api/v1/health",           get(health_handler))
        .route("/api/v1/version",          get(version_handler))
        // Auth — wallet login (web + Android + iOS)
        .route("/api/v1/auth/nonce",       post(api::auth::get_nonce))
        .route("/api/v1/auth/verify",      post(api::auth::verify_signature))
        .route("/api/v1/auth/email-register", post(api::email_auth::register))
        .route("/api/v1/auth/email-login",     post(api::email_auth::login))
        .route("/api/v1/auth/email-verify",    post(api::email_auth::verify_email))
        .route("/api/v1/auth/email-resend",    post(api::email_auth::resend_verification))
        .route("/api/v1/auth/link-email",      post(api::email_auth::link_email))
        .route("/api/v1/auth/link-wallet/nonce",  post(api::email_auth::link_wallet_nonce))
        .route("/api/v1/auth/link-wallet/verify", post(api::email_auth::link_wallet_verify))
        .route("/api/v1/auth/refresh",     post(api::auth::refresh_token))
        .route("/api/v1/auth/logout",      post(api::auth::logout))
        // Feed
        .route("/api/v1/feed",             get(api::feed::get_feed))
        .route("/api/v1/feed/adult",      get(api::feed::get_adult_feed))
        .route("/api/v1/feed/following",   get(api::feed::get_following_feed))
        // Posts
        .route("/api/v1/posts",            post(api::posts::create_post))
        .route("/api/v1/posts/:id",        get(api::posts::get_post))
        .route("/api/v1/posts/:id",        delete(api::posts::delete_post))
        .route("/api/v1/posts/:id/like",   post(api::posts::like_post))
        .route("/api/v1/posts/:id/unlock", post(api::posts::unlock_post))
        .route("/api/v1/posts/:id/reshare",post(api::posts::reshare_post))
        .route("/api/v1/posts/:id/comments", get(api::posts::get_comments))
        .route("/api/v1/posts/:id/comments", post(api::posts::add_comment))
        .route("/api/v1/posts/:id/nft",    post(api::posts::mint_nft))
        .route("/api/v1/posts/:id/repost",  post(api::permanent::repost_post))
        .route("/api/v1/posts/:id/visibility", patch(api::permanent::update_post_visibility))
        .route("/api/v1/posts/:id/unlike",  post(api::posts::unlike_post))
        .route("/api/v1/posts/:id/report",  post(api::report::report_post))
        .route("/api/v1/profile/:user_id/permanent", get(api::permanent::get_permanent_posts))
        .route("/api/v1/me/permanent",     get(api::permanent::get_my_permanent_posts))
        // Users
        .route("/api/v1/users/me",         get(api::users::get_my_profile))
        .route("/api/v1/users/me",         patch(api::users::update_profile))
        .route("/api/v1/users/me",         delete(api::users::delete_my_account))
        .route("/api/v1/users/me/export",  get(api::users::export_my_data))
        .route("/api/v1/users/me/verify-age", post(api::users::verify_age))
        .route("/api/v1/me/age-verification/status",   get(api::age_verification::get_status))
        .route("/api/v1/me/age-verification/submit",   post(api::age_verification::submit))
        .route("/api/v1/me/age-verification/withdraw", post(api::age_verification::withdraw))
        .route("/api/v1/me/age-badge",                 patch(api::age_verification::set_badge_visibility))
        .route("/api/v1/users/me/avatar",  post(api::uploads::upload_avatar))
        .route("/api/v1/users/me/cover",   post(api::uploads::upload_cover))
        .route("/api/v1/uploads/post-media", post(api::uploads::upload_post_media))
        // Live broadcasts
        .route("/api/v1/lives",              post(api::lives::create_live))
        .route("/api/v1/lives/active",       get(api::lives::list_active))
        .route("/api/v1/lives/scheduled",    get(api::lives::list_scheduled))
        .route("/api/v1/lives/mine",         get(api::lives::list_mine))
        .route("/api/v1/lives/:id",          get(api::lives::get_live))
        .route("/api/v1/lives/:id/start",    post(api::lives::start_live))
        .route("/api/v1/lives/:id/end",      post(api::lives::end_live))
        .route("/api/v1/lives/:id/cancel",   post(api::lives::cancel_live))
        .route("/api/v1/lives/:id/viewers",  post(api::lives::ping_viewer_count))
        .route("/api/v1/lives/:id/tip",      post(api::lives::tip_live))
        .route("/api/v1/lives/:id/promote",  post(api::lives::book_promotion))
        .route("/api/v1/lives/:id/promotion", get(api::lives::get_promotion))
        .route("/api/v1/lives/:id/viewer-token", post(api::lives::viewer_token))
        .route("/api/v1/lives/config",       get(api::lives::live_config_status))
        // Scheduled posts
        .route("/api/v1/scheduled-posts",       post(api::scheduled_posts::create))
        .route("/api/v1/scheduled-posts/mine",  get(api::scheduled_posts::list_mine))
        .route("/api/v1/scheduled-posts/:id",   delete(api::scheduled_posts::cancel))
        .route("/api/v1/users/:address",   get(api::users::get_profile))
        .route("/api/v1/users/:address/posts",     get(api::feed::get_user_posts))
        .route("/api/v1/users/:address/followers", get(api::users::list_followers))
        .route("/api/v1/users/:address/following", get(api::users::list_following))
        .route("/api/v1/users/:address/follow",    post(api::users::follow_user))
        // Settings
        .route("/api/v1/settings",         get(api::settings::get_settings))
        .route("/api/v1/settings",         patch(api::settings::update_settings))
        // Boards / Webboards
        .route("/api/v1/boards",                    get(api::boards::get_boards))
        .route("/api/v1/boards/:id",                get(api::boards::get_board))
        .route("/api/v1/webboards",                 get(api::boards::get_boards))
        // Notifications
        .route("/api/v1/search",                        get(api::search::search))
        .route("/api/v1/notifications",                 get(api::notifications::get_notifications))
        .route("/api/v1/notifications/unread-count",    get(api::notifications::unread_count))
        .route("/api/v1/notifications/read",            post(api::notifications::mark_notifications_read))
        .route("/api/v1/notifications/:id/read",        post(api::notifications::mark_one_read))
        .route("/api/v1/users/:address/unfollow", post(api::users::unfollow_user))
        .route("/api/v1/users/:address/block",    post(api::blocks::block))
        .route("/api/v1/users/:address/unblock",  post(api::blocks::unblock))
        .route("/api/v1/me/blocks",               get(api::blocks::list_mine))
        .route("/api/v1/me/e2ee/keys",            get(api::e2ee::get_my_keys))
        .route("/api/v1/me/e2ee/keys",            post(api::e2ee::upload_keys))
        .route("/api/v1/users/:address/e2ee/pubkey", get(api::e2ee::get_peer_pubkey))
        .route("/api/v1/me/e2ee/prekeys",         post(api::e2ee::upload_prekeys))
        .route("/api/v1/me/e2ee/prekeys/count",   get(api::e2ee::prekey_count))
        .route("/api/v1/users/:address/e2ee/bundles", get(api::e2ee::get_prekey_bundles))
        .route("/api/v1/me/devices",                  get(api::e2ee::list_my_devices))
        .route("/api/v1/me/devices/:device_id",       patch(api::e2ee::rename_my_device))
        .route("/api/v1/me/devices/:device_id",       delete(api::e2ee::revoke_my_device))
        .route("/api/v1/conversations",              get(api::conversations::list_mine))
        .route("/api/v1/conversations/dm",           post(api::conversations::create_dm))
        .route("/api/v1/conversations/:id/hide",     post(api::conversations::hide))
        .route("/api/v1/conversations/:id/messages", get(api::messages::list))
        .route("/api/v1/conversations/:id/messages", post(api::messages::send))
        .route("/api/v1/messages/:id",               axum::routing::delete(api::messages::delete_one))
        .route("/api/v1/conversations/:id/messages/image", post(api::messages::upload_image))
        .route("/api/v1/messages/:id/blob",           get(api::messages::get_blob))
        .route("/api/v1/me/dm-retention",            get(api::conversations::get_retention))
        .route("/api/v1/me/dm-retention",            post(api::conversations::update_retention))
        .route("/api/v1/conversations/group",        post(api::invitations::create_group))
        .route("/api/v1/conversations/:id/invite",   post(api::invitations::invite))
        .route("/api/v1/conversations/:id/leave",    post(api::invitations::leave))
        .route("/api/v1/conversations/:id/rotate-key", post(api::invitations::rotate_key))
        .route("/api/v1/conversations/:id/members/:user_id/kick", post(api::invitations::kick))
        .route("/api/v1/me/invitations",             get(api::invitations::list_mine))
        .route("/api/v1/invitations/:id/accept",     post(api::invitations::accept))
        .route("/api/v1/invitations/:id/decline",    post(api::invitations::decline))
        // Messaging hardening: edit, delete-for-all, receipts,
        // mute/archive/self-destruct, message reports, sessions.
        .route("/api/v1/messages/:id/edit",        patch(api::messages::edit_message))
        .route("/api/v1/messages/:id/all",         delete(api::messages::delete_for_all))
        .route("/api/v1/messages/:id/report",      post(api::message_reports::report_message))
        .route("/api/v1/messages/deliveries",      post(api::messages::mark_delivered))
        .route("/api/v1/messages/reads",           post(api::messages::mark_read))
        .route("/api/v1/messages/receipts",        get(api::messages::get_receipts))
        .route("/api/v1/conversations/:id/mute",   post(api::conversations::mute))
        .route("/api/v1/conversations/:id/archive",post(api::conversations::archive))
        .route("/api/v1/conversations/:id/self-destruct", post(api::conversations::set_self_destruct))
        .route("/api/v1/conversations/:id/members",       get(api::conversations::list_members))
        .route("/api/v1/me/sessions",              get(api::sessions::list_mine))
        .route("/api/v1/me/sessions/:id",          delete(api::sessions::revoke_one))
        .route("/api/v1/me/sessions",              delete(api::sessions::revoke_all))
        .route("/api/v1/me/messaging-prefs",       get(api::messaging_prefs::get_prefs))
        .route("/api/v1/me/messaging-prefs",       patch(api::messaging_prefs::update_prefs))
        .route("/api/v1/ws",                       get(api::ws::ws_upgrade))
        .route("/api/v1/presence",                 post(api::ws::presence_query))
        .route("/api/v1/push/config",              get(api::push::config_status))
        .route("/api/v1/me/push/subscribe",        post(api::push::subscribe))
        .route("/api/v1/me/push/subscribe",        delete(api::push::unsubscribe))
        .route("/sw.js",                           get(api::push::service_worker_js))
        .route("/api/v1/admin/message-reports",    get(api::message_reports::admin_list_reports))
        .route("/api/v1/admin/message-reports/:id/resolve",
               post(api::message_reports::admin_resolve_report))
        // Tips & Tokens
        .route("/api/v1/admin/posts",          get(api::report::admin_list_posts))
        .route("/api/v1/admin/reports",        get(api::report::admin_list_reports))
        // Transaction ledger (admin-only; secret gated)
        .route("/api/v1/admin/ledger",          get(api::ledger::list))
        .route("/api/v1/admin/ledger/export",   get(api::ledger::export_csv))
        .route("/api/v1/admin/ledger/summary",  get(api::ledger::summary))
        .route("/api/v1/admin/ledger/verify",   get(api::ledger::verify))
        // Public YEET token explorer (read-only; for third-party providers)
        .route("/api/v1/explorer/token",        get(api::explorer::token_info))
        .route("/api/v1/explorer/richlist",     get(api::explorer::richlist))
        .route("/api/v1/explorer/holders/:address", get(api::explorer::holder))
        .route("/api/v1/explorer/transfers",    get(api::explorer::transfers))
        .route("/api/v1/explorer/tx/:hash",     get(api::explorer::tx_transfers))
        .route("/api/v1/admin/posts/:id",      delete(api::report::admin_remove_post))
        .route("/api/v1/admin/posts/:id/hard", delete(api::report::admin_hard_delete_post))
        .route("/api/v1/admin/posts/:id/unflag", post(api::report::admin_unflag_post))
        .route("/api/v1/admin/posts/:id/restore", post(api::report::admin_restore_post))
        .route("/api/v1/admin/users/:address/ban-post",   post(api::admin_mod::ban_post))
        .route("/api/v1/admin/users/:address/unban-post", post(api::admin_mod::unban_post))
        .route("/api/v1/admin/users/:address/delete",     post(api::admin_mod::delete_user))
        .route("/api/v1/admin/age-verification",          get(api::age_verification::admin_list))
        .route("/api/v1/admin/age-verification/:case_id/blob",
               get(api::age_verification::admin_get_blob))
        .route("/api/v1/admin/age-verification/:case_id/approve",
               post(api::age_verification::admin_approve))
        .route("/api/v1/admin/age-verification/:case_id/reject",
               post(api::age_verification::admin_reject))
        .route("/api/v1/admin/users/:address/age-verify/revoke",
               post(api::age_verification::admin_revoke_verification))
        .route("/api/v1/admin/actions",                   get(api::admin_mod::list_actions))
        .route("/api/v1/admin/stats",                     get(api::admin_mod::stats))
        .route("/api/v1/admin/ping",                      get(api::admin_mod::ping))
        .route("/api/v1/admin/user-lookup",               get(api::admin_mod::lookup_user))
        .route("/api/v1/admin/users",                     get(api::admin_mod::list_users))
        .route("/api/v1/tips",             post(api::tips::send_tip))
        .route("/api/v1/tokens/balance",   get(api::tokens::get_balance))
        .route("/api/v1/tokens/rewards",   get(api::tokens::get_rewards))
        .route("/api/v1/points/convert",   post(api::points::convert))
        // Paper wallets — printable YEET banknotes
        .route("/api/v1/paper-wallets",          post(api::paper_wallets::create))
        .route("/api/v1/paper-wallets",          get(api::paper_wallets::list_mine))
        .route("/api/v1/paper-wallets/redeem",   post(api::paper_wallets::redeem))
        .route("/api/v1/paper-wallets/:id/void", post(api::paper_wallets::void))
        // Sized for the largest accepted upload: a 32 MB video via the
        // multipart endpoint (`/api/v1/uploads/post-media`) plus envelope
        // overhead. Base64-encoded JSON image bodies (≤7 MB) easily fit too.
        .layer(DefaultBodyLimit::max(40 * 1024 * 1024))
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
