use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::AppState;
use shared::ApiResponse;

#[derive(Serialize)]
pub struct NonceResponse {
    pub nonce: String,
    pub message: String,
}

#[derive(Deserialize)]
pub struct WalletLoginRequest {
    pub wallet_address: String,
    pub signature: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user_id: Uuid,
    pub wallet_address: String,
    pub is_new_user: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub wallet: String,
    pub exp: i64,
}

pub async fn get_nonce(
    State(state): State<AppState>,
    Path(wallet): Path<String>,
) -> Result<Json<ApiResponse<NonceResponse>>, StatusCode> {
    let nonce: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let message = format!(
        "Welcome to Yeet!\nSign to login.\nWallet: {wallet}\nNonce: {nonce}\nTime: {}",
        Utc::now().timestamp()
    );

    // Cache nonce in Redis (5 min TTL)
    let mut conn = state.redis.get().await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let key = format!("nonce:{}", wallet.to_lowercase());
    redis::cmd("SETEX")
        .arg(&key).arg(300).arg(&nonce)
        .query_async::<_, ()>(&mut conn).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::ok(NonceResponse { nonce, message })))
}

pub async fn wallet_login(
    State(state): State<AppState>,
    Json(req): Json<WalletLoginRequest>,
) -> Result<Json<ApiResponse<AuthResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let wallet = req.wallet_address.to_lowercase();

    // 1. Verify BSC wallet signature
    let valid = state.bsc
        .verify_signature(&wallet, &req.message, &req.signature)
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(ApiResponse::err("Bad signature format"))))?;

    if !valid {
        return Err((StatusCode::UNAUTHORIZED, Json(ApiResponse::err("Invalid signature"))));
    }

    // 2. Upsert user in DB
    let user = sqlx::query!(
        r#"
        INSERT INTO users (id, wallet_address, username, created_at)
        VALUES ($1, $2, $3, NOW())
        ON CONFLICT (wallet_address)
        DO UPDATE SET wallet_address = EXCLUDED.wallet_address
        RETURNING id, username,
            (xmax = 0) AS "is_new_user!"
        "#,
        Uuid::new_v4(),
        wallet,
        format!("user_{}", &wallet[2..8])
    )
    .fetch_one(&state.db.pool)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::err("DB error"))))?;

    // 3. Reward daily login tokens (if new day)
    if user.is_new_user {
        let _ = crate::services::tokens::reward_action(
            &state, user.id,
            shared::RewardAction::DailyLogin
        ).await;
    }

    // 4. Issue JWT
    let claims = Claims {
        sub: user.id.to_string(),
        wallet: wallet.clone(),
        exp: (Utc::now() + Duration::days(7)).timestamp(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    ).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::err("Token error"))))?;

    Ok(Json(ApiResponse::ok(AuthResponse {
        token,
        user_id: user.id,
        wallet_address: wallet,
        is_new_user: user.is_new_user,
    })))
}

pub async fn refresh_token(
    State(_state): State<AppState>,
) -> Json<ApiResponse<String>> {
    Json(ApiResponse::err("Not implemented"))
}
