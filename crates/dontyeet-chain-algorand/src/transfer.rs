//! Server-side Algorand native ALGO Payment transfer.
//!
//! Fetches suggested parameters from the Algod REST v2 API, then
//! delegates to [`signing::build_signed_payment`] for the pure-crypto
//! pipeline (canonical msgpack encoding, `"TX"`-prefixed Ed25519
//! signing, signed-tx envelope). Phase M.4.6 moved everything after
//! the network call into the always-on [`crate::signing`] module so
//! the in-browser send pipeline goes through the same code.

use std::collections::HashMap;

use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::address::Address;
use dontyeet_primitives::amount::Amount;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::PrivateKey;

use crate::signing::{self, PaymentParams};

/// Validity window: `last_valid = first_valid + this offset` (~50
/// minutes at ~3 s / round).
const VALIDITY_ROUNDS: u64 = 1000;

// ---- RPC response shape ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
struct SuggestedParams {
    genesis_hash: String,
    genesis_id: String,
    last_round: u64,
    min_fee: u64,
}

// ---- Public API ----

/// Build and sign a native-ALGO Payment transaction.
///
/// Fetches suggested parameters via JSON, then delegates to
/// [`signing::build_signed_payment`] for canonical-msgpack encoding,
/// signing, and envelope wrap. Returns the raw bytes for the
/// broadcaster to ship to `POST /v2/transactions`.
///
/// # Errors
/// Returns [`DontYeetWalletError`] if the API URL list is missing for
/// `network`, the suggested-params fetch fails, address decoding
/// fails, or signing fails.
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
    let micro_algos = u64::try_from(amount.raw())
        .map_err(|_| DontYeetWalletError::Validation("amount too large for ALGO transfer".into()))?;

    let sender = signing::address_to_pubkey(from.as_str())?;
    let receiver = signing::address_to_pubkey(to.as_str())?;

    let params = fetch_tx_params(api_urls, network).await?;

    let genesis_hash_bytes = data_encoding::BASE64
        .decode(params.genesis_hash.as_bytes())
        .map_err(|e| DontYeetWalletError::Chain(format!("genesis hash decode: {e}")))?;
    let genesis_hash: [u8; 32] = genesis_hash_bytes
        .as_slice()
        .try_into()
        .map_err(|_| DontYeetWalletError::Chain("genesis hash is not 32 bytes".into()))?;

    let payment = PaymentParams {
        sender,
        receiver,
        micro_algos,
        fee: params.min_fee,
        first_valid: params.last_round,
        last_valid: params.last_round.saturating_add(VALIDITY_ROUNDS),
        genesis_id: params.genesis_id,
        genesis_hash,
    };
    signing::build_signed_payment(&payment, private_key)
}

// ---- RPC plumbing ----

/// Fetch suggested transaction parameters from Algod.
async fn fetch_tx_params(
    api_urls: &HashMap<NetworkId, Vec<Url>>,
    network: &NetworkId,
) -> Result<SuggestedParams> {
    let urls = api_urls
        .get(network)
        .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;
    let base = urls
        .first()
        .ok_or_else(|| DontYeetWalletError::NotFound("API URL list is empty".into()))?;

    let params_url = base
        .join("/v2/transactions/params")
        .map_err(|e| DontYeetWalletError::Network(format!("URL join error: {e}")))?;

    let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;
    let response = client
        .get(&params_url)
        .await
        .map_err(|e| DontYeetWalletError::Network(format!("fetch tx params: {e}")))?;

    response
        .json()
        .map_err(|e| DontYeetWalletError::Network(format!("tx params parse: {e}")))
}

// Rust guideline compliant 2026-02-21
