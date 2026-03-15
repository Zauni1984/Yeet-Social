//! JWT + EIP-191 wallet signature authentication service.
//! Uses k256 + sha3 for signature verification — no heavy ethers dependency.
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
    let claims = AccessClaims {
        sub: address.to_lowercase(),
        iat: now,
        exp: now + ttl,
        jti: Uuid::new_v4().to_string(),
        token_type,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(config.secret.as_bytes()))
        .context("JWT encode failed")
}

pub fn verify_access_token(token: &str, config: &JwtConfig) -> Result<AccessClaims> {
    let mut v = Validation::default();
    v.validate_exp = true;
    let data = decode::<AccessClaims>(token, &DecodingKey::from_secret(config.secret.as_bytes()), &v)
        .context("Invalid or expired token")?;
    if data.claims.token_type != TokenType::Access { anyhow::bail!("Expected access token"); }
    Ok(data.claims)
}

pub fn verify_refresh_token(token: &str, config: &JwtConfig) -> Result<AccessClaims> {
    let mut v = Validation::default();
    v.validate_exp = true;
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
    format!(
        "Welcome to Yeet Social!\n\nSign this message to authenticate.\nThis will not trigger a blockchain transaction.\n\nNonce: {nonce}"
    )
}

/// Verify an EIP-191 personal_sign signature and recover the signer address.
/// Uses k256 (secp256k1) + sha3 (keccak256) — no ethers dependency.
pub fn recover_signer(message: &str, signature_hex: &str) -> Result<String> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    use sha3::{Digest, Keccak256};

    // Parse signature bytes (65 bytes: r[32] + s[32] + v[1])
    let sig_bytes = hex::decode(signature_hex.strip_prefix("0x").unwrap_or(signature_hex))
        .context("Invalid hex in signature")?;
    anyhow::ensure!(sig_bytes.len() == 65, "Signature must be 65 bytes");

    // EIP-191 prefix
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
    let mut hasher = Keccak256::new();
    hasher.update(prefix.as_bytes());
    hasher.update(message.as_bytes());
    let msg_hash = hasher.finalize();

    // Recovery id: last byte is 27 or 28 for eth (normalize to 0/1)
    let recovery_byte = sig_bytes[64];
    let recovery_id_val = if recovery_byte >= 27 { recovery_byte - 27 } else { recovery_byte };
    let recovery_id = RecoveryId::from_byte(recovery_id_val)
        .context("Invalid recovery id")?;

    let sig = Signature::from_slice(&sig_bytes[..64])
        .context("Invalid signature")?;
    let verifying_key = VerifyingKey::recover_from_prehash(&msg_hash, &sig, recovery_id)
        .context("Failed to recover key from signature")?;

    // Convert public key to Ethereum address (keccak256 of uncompressed pubkey, last 20 bytes)
    let uncompressed = verifying_key.to_encoded_point(false);
    let pubkey_bytes = &uncompressed.as_bytes()[1..]; // skip 0x04 prefix
    let mut hasher2 = Keccak256::new();
    hasher2.update(pubkey_bytes);
    let addr_hash = hasher2.finalize();
    let addr_bytes = &addr_hash[12..]; // last 20 bytes
    Ok(format!("0x{}", hex::encode(addr_bytes)))
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs()
}
