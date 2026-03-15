//! JWT + EIP-191 wallet signature authentication service.
use anyhow::{Context, Result};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use crate::state::JwtConfig;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccessClaims {
    pub sub: String,
    pub iat: u64,
    pub exp: u64,
    pub jti: String,
    pub token_type: TokenType,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenType { Access, Refresh }

pub fn issue_token_pair(address: &str, config: &JwtConfig) -> Result<(String, String)> {
    let access  = issue_token(address, TokenType::Access,  config.access_ttl_secs,  config)?;
    let refresh = issue_token(address, TokenType::Refresh, config.refresh_ttl_secs, config)?;
    Ok((access, refresh))
}

fn issue_token(address: &str, token_type: TokenType, ttl: u64, config: &JwtConfig) -> Result<String> {
    let now = unix_now();
    let claims = AccessClaims { sub: address.to_lowercase(), iat: now, exp: now + ttl, jti: Uuid::new_v4().to_string(), token_type };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(config.secret.as_bytes())).context("JWT encode failed")
}

pub fn verify_access_token(token: &str, config: &JwtConfig) -> Result<AccessClaims> {
    let mut v = Validation::default(); v.validate_exp = true;
    let data = decode::<AccessClaims>(token, &DecodingKey::from_secret(config.secret.as_bytes()), &v)
        .context("Invalid or expired token")?;
    if data.claims.token_type != TokenType::Access { anyhow::bail!("Expected access token"); }
    Ok(data.claims)
}

pub fn verify_refresh_token(token: &str, config: &JwtConfig) -> Result<AccessClaims> {
    let mut v = Validation::default(); v.validate_exp = true;
    let data = decode::<AccessClaims>(token, &DecodingKey::from_secret(config.secret.as_bytes()), &v)
        .context("Invalid or expired token")?;
    if data.claims.token_type != TokenType::Refresh { anyhow::bail!("Expected refresh token"); }
    Ok(data.claims)
}

pub fn generate_nonce() -> String {
    let bytes: [u8; 16] = rand::thread_rng().gen();
    format!("YEET-{}", hex::encode(bytes))
}

pub fn sign_message(nonce: &str) -> String {
    format!("Welcome to Yeet Social!\n\nSign this message to authenticate.\nThis request will not trigger any blockchain transaction.\n\nNonce: {nonce}")
}

pub fn recover_signer(message: &str, signature: &str) -> Result<String> {
    use ethers::core::types::Signature;
    use std::str::FromStr;
    let sig = Signature::from_str(signature).context("Invalid signature format")?;
    let hash = ethers::core::utils::hash_message(message);
    let recovered = sig.recover(hash).context("Failed to recover signer")?;
    Ok(format!("{:?}", recovered).to_lowercase())
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs()
}
