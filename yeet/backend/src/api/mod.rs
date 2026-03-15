pub mod auth;
pub mod feed;
pub mod middleware;
pub mod posts;
pub mod users;
pub mod tips;
pub mod tokens;

use axum::{Router, routing::{get, post, delete}};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        // Auth
        .route("/auth/wallet-login", post(auth::wallet_login))
        .route("/auth/nonce/:wallet", get(auth::get_nonce))
        .route("/auth/refresh", post(auth::refresh_token))
        // Users
        .route("/users/:username", get(users::get_profile))
        .route("/users/me", get(users::get_me))
        .route("/users/me", axum::routing::put(users::update_me))
        .route("/users/:id/follow", post(users::follow))
        .route("/users/:id/unfollow", delete(users::unfollow))
        // Posts
        .route("/posts", post(posts::create_post))
        .route("/posts/:id", get(posts::get_post))
        .route("/posts/:id", delete(posts::delete_post))
        .route("/posts/:id/like", post(posts::like_post))
        .route("/posts/:id/reshare", post(posts::reshare_post))
        .route("/posts/:id/comments", get(posts::get_comments))
        .route("/posts/:id/comments", post(posts::add_comment))
        .route("/posts/:id/nft", post(posts::mint_nft))
        // Feed
        .route("/feed", get(feed::get_feed))
        // Tips & Tokens
        .route("/tips", post(tips::send_tip))
        .route("/tokens/balance", get(tokens::get_balance))
        .route("/tokens/rewards", get(tokens::get_rewards))
}
