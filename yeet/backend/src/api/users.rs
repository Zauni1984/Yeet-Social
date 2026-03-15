use axum::{extract::{Path, State}, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::AppState;
use shared::{ApiResponse, User};
use chrono::Utc;

pub async fn get_profile(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<ApiResponse<User>>, StatusCode> {
    let row = sqlx::query!(
        r#"
        SELECT id, username, display_name, bio, avatar_url,
               wallet_address, country_code, is_verified,
               age_verified, yeet_token_balance, created_at
        FROM users WHERE username = $1
        "#,
        username
    )
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match row {
        Some(r) => Ok(Json(ApiResponse::ok(User {
            id: r.id,
            username: r.username,
            display_name: r.display_name.unwrap_or_default(),
            bio: r.bio,
            avatar_url: r.avatar_url,
            wallet_address: r.wallet_address,
            country_code: r.country_code,
            is_verified: r.is_verified.unwrap_or(false),
            age_verified: r.age_verified.unwrap_or(false),
            yeet_token_balance: r.yeet_token_balance.unwrap_or(0.0),
            created_at: r.created_at,
        }))),
        None => Ok(Json(ApiResponse::err("User not found"))),
    }
}

pub async fn get_me(State(s): State<AppState>) -> Json<ApiResponse<String>> {
    Json(ApiResponse::ok("TODO: extract from JWT middleware".into()))
}

#[derive(Deserialize)]
pub struct UpdateProfileRequest {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
}

pub async fn update_me(
    State(state): State<AppState>,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let user_id = Uuid::new_v4(); // TODO: from JWT
    sqlx::query!(
        r#"
        UPDATE users
        SET display_name = COALESCE($2, display_name),
            bio = COALESCE($3, bio),
            avatar_url = COALESCE($4, avatar_url)
        WHERE id = $1
        "#,
        user_id, req.display_name, req.bio, req.avatar_url,
    )
    .execute(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ApiResponse::ok(())))
}

pub async fn follow(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let follower_id = Uuid::new_v4(); // TODO: from JWT
    sqlx::query!(
        r#"
        INSERT INTO follows (follower_id, following_id, created_at)
        VALUES ($1, $2, NOW()) ON CONFLICT DO NOTHING
        "#,
        follower_id, id
    )
    .execute(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ApiResponse::ok(())))
}

pub async fn unfollow(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let follower_id = Uuid::new_v4(); // TODO: from JWT
    sqlx::query!(
        "DELETE FROM follows WHERE follower_id = $1 AND following_id = $2",
        follower_id, id
    )
    .execute(&state.db.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ApiResponse::ok(())))
}
