//! Public, unauthenticated runtime configuration for the frontend.
//!
//! Lets the single-file frontend pick up deploy-specific values from the
//! backend environment instead of edits to index.html: the chain id, the
//! deployed YEET token address and the WalletConnect Cloud project id.
//! Nothing here is secret — a WalletConnect project id is public by design
//! (it ends up in the browser either way).
use axum::Json;
use serde::Serialize;
use crate::models::ApiResponse;

#[derive(Debug, Serialize)]
pub struct PublicConfig {
    pub chain_id: u64,
    /// Deployed YEET token (BEP-20); `None` while the placeholder address is set.
    pub yeet_token_address: Option<String>,
    /// WalletConnect Cloud project id (`WALLETCONNECT_PROJECT_ID`); `None` hides
    /// the WalletConnect option in the frontend.
    pub walletconnect_project_id: Option<String>,
}

fn non_empty(var: &str) -> Option<String> {
    std::env::var(var).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// GET /api/v1/config
pub async fn get() -> Json<ApiResponse<PublicConfig>> {
    let chain_id: u64 = std::env::var("YEET_CHAIN_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(56);
    let yeet_token_address = non_empty("YEET_TOKEN_ADDRESS")
        .filter(|a| a != "0x0000000000000000000000000000000000000000");
    Json(ApiResponse::ok(PublicConfig {
        chain_id,
        yeet_token_address,
        walletconnect_project_id: non_empty("WALLETCONNECT_PROJECT_ID"),
    }))
}
