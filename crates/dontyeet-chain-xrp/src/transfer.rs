//! Server-side XRP Ledger native Payment transfer.
//!
//! Fetches account sequence + current ledger index from the XRP
//! Ledger JSON-RPC, then hands a [`signing::PaymentParams`] to
//! [`signing::build_signed_payment`] for the pure-crypto pipeline
//! (binary serialization, SHA-512 Half + ECDSA, re-serialize with
//! `TxnSignature`). Phase M.4.4 moved everything after the network
//! calls into the always-on [`crate::signing`] module so the
//! in-browser send pipeline goes through the same code.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

use crate::keys::compressed_pubkey;
use crate::signing::{self, PaymentParams};

/// Ledger validity window: ~80 seconds at ~4 s / ledger.
const LEDGER_OFFSET: u64 = 20;

/// XRPL base fee in drops on mainnet.
const BASE_FEE_DROPS: u64 = 12;

// ---- RPC response shapes ----

#[derive(serde::Deserialize)]
struct AccountInfoResponse {
    result: AccountInfoResult,
}

#[derive(serde::Deserialize)]
struct AccountInfoResult {
    account_data: Option<AccountData>,
    error: Option<String>,
}

#[derive(serde::Deserialize)]
#[expect(
    non_snake_case,
    reason = "struct mirrors XRPL JSON-RPC schema where field names use PascalCase (Sequence)"
)]
struct AccountData {
    Sequence: u64,
}

#[derive(serde::Deserialize)]
struct LedgerCurrentResponse {
    result: LedgerCurrentResult,
}

#[derive(serde::Deserialize)]
struct LedgerCurrentResult {
    ledger_current_index: u64,
}

// ---- Public API ----

/// Build and sign a native-XRP Payment transaction.
///
/// Fetches the sender's account sequence and the current ledger index
/// via JSON-RPC, then delegates to [`signing::build_signed_payment`]
/// for binary serialization + signing. Returns the raw signed blob
/// for the broadcaster to hex-encode and `submit`.
///
/// # Errors
/// Returns [`DontYeetWalletError`] if the API URL list is missing for
/// `network`, the JSON-RPC calls fail, address decoding fails, or
/// signing fails.
#[expect(
    clippy::implicit_hasher,
    reason = "callers always pass HashMap with default RandomState; generic hasher would not improve API ergonomics"
)]
pub async fn build_signed_transfer(
    api_urls: &HashMap<NetworkId, Vec<Url>>,
    from: &Address,
    to: &Address,
    amount: &Amount,
    private_key: &PrivateKey,
    network: &NetworkId,
) -> Result<Vec<u8>> {
    let drops = u64::try_from(amount.raw())
        .map_err(|_| DontYeetWalletError::Validation("amount too large for XRP transfer".into()))?;

    // 1. Pure crypto: derive accounts + signing pubkey.
    let from_account_id = signing::address_to_account_id(from.as_str())?;
    let to_account_id = signing::address_to_account_id(to.as_str())?;
    let signing_pub_key = compressed_pubkey(private_key)?;

    // 2. Network: fetch sequence + current ledger.
    let (sequence, ledger_index) = fetch_account_state(api_urls, from, network).await?;
    let last_ledger_seq = ledger_index.saturating_add(LEDGER_OFFSET);

    // 3. Build + sign via the shared signing core.
    let params = PaymentParams {
        from_account_id,
        to_account_id,
        drops,
        fee_drops: BASE_FEE_DROPS,
        sequence,
        last_ledger_seq,
        signing_pub_key,
    };
    signing::build_signed_payment(&params, private_key)
}

// ---- RPC plumbing ----

/// Fetch account sequence number and current ledger index.
async fn fetch_account_state(
    api_urls: &HashMap<NetworkId, Vec<Url>>,
    account: &Address,
    network: &NetworkId,
) -> Result<(u64, u64)> {
    let urls = api_urls
        .get(network)
        .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;
    let base = urls
        .first()
        .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

    let acct_body = serde_json::json!({
        "method": "account_info",
        "params": [{ "account": account.as_str() }]
    });
    let acct_resp = client
        .post_json(base, &acct_body)
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("account_info: {e}")))?;
    let acct: AccountInfoResponse = acct_resp
        .json()
        .map_err(|e| DontYeetWalletError::Network(format!("account_info parse: {e}")))?;

    if let Some(err) = acct.result.error {
        return Err(DontYeetWalletError::Network(format!("account_info error: {err}")));
    }
    let sequence = acct
        .result
        .account_data
        .ok_or_else(|| DontYeetWalletError::Network("account_info: missing account_data".into()))?
        .Sequence;

    let ledger_body = serde_json::json!({
        "method": "ledger_current",
        "params": [{}]
    });
    let ledger_resp = client
        .post_json(base, &ledger_body)
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("ledger_current: {e}")))?;
    let ledger: LedgerCurrentResponse = ledger_resp
        .json()
        .map_err(|e| DontYeetWalletError::Network(format!("ledger_current parse: {e}")))?;

    Ok((sequence, ledger.result.ledger_current_index))
}

// Rust guideline compliant 2026-02-21
