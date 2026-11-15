//! Proof of possession for sensitive account actions (audit A-L6, MSG-H4):
//! the account password for email accounts, or a wallet signature over a
//! one-shot nonce for wallet accounts. A valid session token alone is not
//! enough to delete the account or replace the E2EE identity, so a stolen
//! token cannot do either.
use axum::{extract::State, Json};
use serde::Deserialize;
use uuid::Uuid;

use crate::{AppError, AppResult, AppState, models::ApiResponse};
use crate::api::middleware::AuthUser;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Reauth {
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    /// A current 2FA code (or recovery code), accepted when the account has
    /// 2FA enabled — a hijacked session without the authenticator fails.
    #[serde(default)]
    pub totp: Option<String>,
}

const NONCE_SCOPE: &str = "reauth";

/// POST /api/v1/me/reauth-nonce — a one-shot nonce (10 min) that a wallet
/// account signs (EIP-191) to prove possession before a sensitive action.
pub async fn nonce_endpoint(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<ApiResponse<crate::api::auth::NonceResponse>>> {
    let me = crate::api::users::resolve_user_id(&state, &auth.address).await?;
    let nonce = crate::services::auth::generate_nonce();
    let message = crate::services::auth::sign_message(&nonce);
    state.cache.set_nonce(NONCE_SCOPE, &me.to_string(), &nonce, std::time::Duration::from_secs(600)).await
        .map_err(|e| AppError::Cache(e.to_string()))?;
    Ok(Json(ApiResponse::ok(crate::api::auth::NonceResponse { nonce, message })))
}

/// Verify the proof for `user_id`. `None` or a wrong proof is refused with
/// 401 (`REAUTH_REQUIRED` / `REAUTH_FAILED`). Callers rate-limit on their
/// own so password guessing through this path stays bounded.
pub async fn verify(state: &AppState, user_id: Uuid, r: Option<&Reauth>) -> AppResult<()> {
    let r = r.ok_or_else(|| AppError::Unauthorised(
        "REAUTH_REQUIRED: this action needs your password or a wallet signature".into()))?;
    let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT password_hash, password_salt, wallet_address FROM users WHERE id = $1"
    ).bind(user_id).fetch_optional(state.db.pool()).await.map_err(AppError::Database)?;
    let (pw_hash, pw_salt, wallet) = row.unwrap_or_default();
    let mut ok = false;
    if let Some(code) = r.totp.as_deref() {
        if crate::api::twofa::verify_second_factor(state, user_id, code).await.unwrap_or(false) {
            ok = true;
        }
    }
    if !ok {
    if let (Some(pw), Some(h)) = (r.password.as_deref(), pw_hash.as_deref()) {
        ok = crate::api::email_auth::verify_password(pw, h, pw_salt.as_deref().unwrap_or("")).0;
    }
    if !ok {
        if let (Some(n), Some(sig), Some(w)) = (r.nonce.as_deref(), r.signature.as_deref(), wallet.as_deref()) {
            let stored = state.cache.consume_nonce(NONCE_SCOPE, &user_id.to_string()).await
                .map_err(|e| AppError::Cache(e.to_string()))?;
            if stored.as_deref() == Some(n) {
                let msg = crate::services::auth::sign_message(n);
                ok = crate::services::auth::recover_signer(&msg, sig)
                    .map(|a| a.eq_ignore_ascii_case(w)).unwrap_or(false);
            }
        }
    }
    }
    if ok { Ok(()) } else {
        Err(AppError::Unauthorised("REAUTH_FAILED: password, 2FA code or wallet signature did not verify".into()))
    }
}
