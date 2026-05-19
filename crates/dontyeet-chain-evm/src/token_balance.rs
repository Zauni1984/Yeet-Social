//! ERC-20 token balance fetching via `eth_call`.
//!
//! Calls `balanceOf(address)`, `decimals()`, and `symbol()` on the
//! token contract to return a fully-typed [`Amount`] and symbol string.

use std::collections::HashMap;

use url::Url;

use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::{Amount, DontYeetWalletError, Result};

/// Fetches ERC-20 token balances using standard `eth_call` RPCs.
pub struct EvmTokenBalanceFetcher {
    rpc_urls: HashMap<NetworkId, Vec<Url>>,
}

impl EvmTokenBalanceFetcher {
    /// Create a new fetcher sharing the same RPC URLs as the chain plugin.
    #[must_use]
    pub fn new(rpc_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            rpc_urls: rpc_urls.clone(),
        }
    }

    /// Fetch the token balance of `owner_address` for the ERC-20 at
    /// `contract_address` on the given `network`.
    ///
    /// Returns `(amount, symbol)`.
    ///
    /// # Errors
    /// Returns network or parsing errors from the RPC calls.
    pub async fn fetch_balance(
        &self,
        owner_address: &str,
        contract_address: &str,
        network: &NetworkId,
    ) -> Result<(Amount, String)> {
        // Reject malformed contract addresses before forwarding to the RPC.
        // A non-conforming string would otherwise be sent verbatim as the
        // `to` field of an `eth_call` JSON payload.
        validate_evm_address(contract_address)?;

        let urls = self
            .rpc_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::Network(format!("no RPC URLs for {network}")))?;

        // 1. balanceOf(address) — selector 0x70a08231
        let balance_data = encode_balance_of(owner_address);
        let raw_hex: String = eth_call(urls, contract_address, &balance_data).await?;
        let raw_balance = crate::rpc::parse_hex_u128(&raw_hex)?;

        // 2. decimals() — selector 0x313ce567
        let decimals_hex: String = eth_call(urls, contract_address, "0x313ce567").await?;
        let decimals_u64 = crate::rpc::parse_hex_u64(&decimals_hex)?;
        // ERC-20 decimals fit in u8; clamp to prevent overflow.
        let decimals = u8::try_from(decimals_u64).unwrap_or(18);

        // 3. symbol() — selector 0x95d89b41 (best-effort)
        let symbol = match eth_call(urls, contract_address, "0x95d89b41").await {
            Ok(hex) => decode_abi_string(&hex).unwrap_or_else(|| "TOKEN".into()),
            Err(_) => "TOKEN".into(),
        };

        let amount = Amount::from_raw(raw_balance, decimals);
        Ok((amount, symbol))
    }
}

/// Reject any string that isn't `0x` followed by exactly 40 hex characters.
fn validate_evm_address(address: &str) -> Result<()> {
    let stripped = address
        .strip_prefix("0x")
        .ok_or_else(|| DontYeetWalletError::Validation("EVM address must start with 0x".into()))?;
    if stripped.len() != 40 {
        return Err(DontYeetWalletError::Validation(format!(
            "EVM address must be 40 hex chars, got {}",
            stripped.len()
        )));
    }
    if !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(DontYeetWalletError::Validation(
            "EVM address contains non-hex characters".into(),
        ));
    }
    Ok(())
}

/// Encode `balanceOf(address)` calldata.
///
/// Layout: `0x70a08231` + address left-padded to 32 bytes.
fn encode_balance_of(address: &str) -> String {
    let addr = address.strip_prefix("0x").unwrap_or(address).to_lowercase();
    format!("0x70a08231{addr:0>64}")
}

/// Perform an `eth_call` with the given contract and calldata.
async fn eth_call(urls: &[Url], contract: &str, data: &str) -> Result<String> {
    let params = serde_json::json!([
        { "to": contract, "data": data },
        "latest"
    ]);
    crate::rpc::rpc_call(urls, "eth_call", params).await
}

/// Decode an ABI-encoded `string` return value.
///
/// Standard ABI encoding for a single string return:
/// - bytes 0..32: offset (always 0x20)
/// - bytes 32..64: string length
/// - bytes 64..: UTF-8 data (right-padded to 32-byte boundary)
fn decode_abi_string(hex: &str) -> Option<String> {
    let stripped = hex.strip_prefix("0x").unwrap_or(hex);
    let bytes = hex::decode(stripped).ok()?;

    // Minimum: offset (32) + length (32) = 64 bytes.
    if bytes.len() < 64 {
        return None;
    }

    // Read length from bytes[32..64].
    let mut len_buf = [0u8; 32];
    len_buf.copy_from_slice(&bytes[32..64]);
    let len = u128::from_be_bytes(len_buf[16..32].try_into().ok()?) as usize;

    if bytes.len() < 64 + len {
        return None;
    }

    String::from_utf8(bytes[64..64 + len].to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_balance_of_formats_correctly() {
        let data = encode_balance_of("0xdAC17F958D2ee523a2206206994597C13D831ec7");
        assert!(data.starts_with("0x70a08231"));
        // Address is 40 hex chars, padded to 64 with leading zeros.
        assert_eq!(data.len(), 2 + 8 + 64); // "0x" + selector + padded addr
    }

    #[test]
    fn decode_abi_string_usdc() {
        // ABI-encoded "USDC"
        let hex = "0x\
            0000000000000000000000000000000000000000000000000000000000000020\
            0000000000000000000000000000000000000000000000000000000000000004\
            5553444300000000000000000000000000000000000000000000000000000000";
        let result = decode_abi_string(hex);
        assert_eq!(result, Some("USDC".into()));
    }

    #[test]
    fn decode_abi_string_short_returns_none() {
        assert_eq!(decode_abi_string("0x1234"), None);
    }

    #[test]
    fn validate_evm_address_accepts_well_formed() {
        assert!(validate_evm_address("0xdAC17F958D2ee523a2206206994597C13D831ec7").is_ok());
        // All-lowercase
        assert!(validate_evm_address("0x0000000000000000000000000000000000000000").is_ok());
    }

    #[test]
    fn validate_evm_address_rejects_missing_prefix() {
        assert!(validate_evm_address("dAC17F958D2ee523a2206206994597C13D831ec7").is_err());
    }

    #[test]
    fn validate_evm_address_rejects_wrong_length() {
        assert!(validate_evm_address("0x1234").is_err());
        assert!(validate_evm_address(&format!("0x{}", "a".repeat(41))).is_err());
    }

    #[test]
    fn validate_evm_address_rejects_non_hex() {
        assert!(validate_evm_address("0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ").is_err());
        // JSON-special chars in the middle
        assert!(validate_evm_address("0x\"00000000000000000000000000000000000000").is_err());
    }
}

// Rust guideline compliant 2026-05-02
