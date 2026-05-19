//! Solana transaction history via JSON-RPC.
//!
//! Two-phase fetch:
//!
//! 1. `getSignaturesForAddress` lists recent signatures touching the wallet.
//! 2. A batched `getTransaction` JSON-RPC call (single round trip, one
//!    request object per signature) pulls the per-tx details needed to
//!    classify direction and amount from `preBalances`/`postBalances`.
//!
//! When the second call fails (RPC outage, batching unsupported by the
//! configured endpoint, etc.) we still return the signature list as
//! "unclassified" entries — better partial data than a hard error.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;
use url::Url;

use dontyeet_network::{HttpClient, ReqwestClient};
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::transaction::{TxConfirmation, TxDirection, TxHash, TxHistoryItem};
use dontyeet_primitives::{Address, Amount, DontYeetWalletError, Result};

/// SOL has 9 decimal places (1 SOL = 1e9 lamports).
const SOL_DECIMALS: u8 = 9;

/// Fetches Solana transaction history from a JSON-RPC endpoint.
pub struct SolHistoryFetcher {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl SolHistoryFetcher {
    /// Create a new fetcher sharing the same API URLs as the chain plugin.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            api_urls: api_urls.clone(),
        }
    }

    /// Fetch transaction history for `address` on `network`.
    ///
    /// Returns up to `limit` signatures (defaulting to 25 when `limit ==
    /// 0`), each enriched with direction, lamport amount, counterparty
    /// pubkey, and confirmation status.
    ///
    /// # Errors
    /// Returns network or parsing errors from the first
    /// `getSignaturesForAddress` call. Failures of the per-tx batch are
    /// downgraded to "unclassified" entries rather than propagated.
    pub async fn fetch(
        &self,
        address: &str,
        network: &NetworkId,
        limit: usize,
    ) -> Result<Vec<TxHistoryItem>> {
        let urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::Network(format!("no API URLs for {network}")))?;
        let base = urls
            .first()
            .ok_or_else(|| DontYeetWalletError::Network("API URL list is empty".into()))?;

        let client = ReqwestClient::direct().map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

        let cap = if limit == 0 { 25 } else { limit };
        let signatures = fetch_signatures(&client, base, address, cap).await?;
        if signatures.is_empty() {
            return Ok(Vec::new());
        }

        // Best-effort enrichment: align by index, fall back to None on RPC
        // failure so we still return the signature list.
        let details = match fetch_tx_details_batch(&client, base, &signatures).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(
                    "Solana getTransaction batch failed; returning unclassified history: {e}"
                );
                (0..signatures.len()).map(|_| None).collect()
            }
        };

        let items = signatures
            .iter()
            .zip(details.iter())
            .map(|(sig, detail)| classify(address, sig, detail.as_ref()))
            .collect();
        Ok(items)
    }
}

/// Run `getSignaturesForAddress` and return the signature list.
async fn fetch_signatures(
    client: &ReqwestClient,
    base: &Url,
    address: &str,
    limit: usize,
) -> Result<Vec<SignatureInfo>> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [address, {"limit": limit}],
    });

    let response = client
        .post_json(base, &body)
        .await
        .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

    let rpc: SignaturesResponse = serde_json::from_slice(&response.body)
        .map_err(|e| DontYeetWalletError::Network(format!("parse signatures: {e}")))?;
    Ok(rpc.result.unwrap_or_default())
}

/// Batched `getTransaction` lookup, one entry per signature.
///
/// Solana's RPC supports JSON-RPC batching natively; this collapses N
/// follow-up calls into a single HTTP round trip. The response array can
/// arrive out of order, so we re-align by `id` before returning.
async fn fetch_tx_details_batch(
    client: &ReqwestClient,
    base: &Url,
    signatures: &[SignatureInfo],
) -> Result<Vec<Option<TxDetail>>> {
    let batch: Vec<Value> = signatures
        .iter()
        .enumerate()
        .map(|(idx, sig)| {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": idx,
                "method": "getTransaction",
                "params": [
                    sig.signature,
                    {
                        "encoding": "jsonParsed",
                        "maxSupportedTransactionVersion": 0,
                    },
                ],
            })
        })
        .collect();

    let body = Value::Array(batch);
    let response = client
        .post_json(base, &body)
        .await
        .map_err(|e| DontYeetWalletError::Network(e.to_string()))?;

    let raw: Vec<RpcEnvelope<TxDetail>> = serde_json::from_slice(&response.body)
        .map_err(|e| DontYeetWalletError::Network(format!("parse batch: {e}")))?;

    let mut out: Vec<Option<TxDetail>> = (0..signatures.len()).map(|_| None).collect();
    for entry in raw {
        if let (Some(id), Some(detail)) = (entry.id, entry.result)
            && let Some(slot) = out.get_mut(id)
        {
            *slot = Some(detail);
        }
    }
    Ok(out)
}

/// Convert a signature plus optional tx detail into a [`TxHistoryItem`].
fn classify(our_address: &str, sig: &SignatureInfo, detail: Option<&TxDetail>) -> TxHistoryItem {
    let status = derive_status(sig, detail);
    let tx_hash = TxHash::new(&sig.signature);

    // No detail → fall back to unclassified, but still report the right
    // status so the UI can render "pending" / "failed" badges.
    let Some(detail) = detail else {
        return TxHistoryItem {
            tx_hash,
            direction: TxDirection::Unknown,
            counterparty: Address::new(""),
            amount: Amount::from_raw(0, SOL_DECIMALS),
            symbol: "SOL".into(),
            timestamp: sig.block_time,
            status,
        };
    };

    let Some(meta) = detail.meta.as_ref() else {
        return TxHistoryItem {
            tx_hash,
            direction: TxDirection::Unknown,
            counterparty: Address::new(""),
            amount: Amount::from_raw(0, SOL_DECIMALS),
            symbol: "SOL".into(),
            timestamp: sig.block_time,
            status,
        };
    };

    let account_keys = detail
        .transaction
        .as_ref()
        .and_then(|t| t.message.as_ref())
        .map(|m| m.account_keys.as_slice())
        .unwrap_or_default();

    let our_idx = account_keys.iter().position(|k| k.pubkey == our_address);

    let (direction, amount_lamports, counterparty) = match our_idx {
        Some(i) if balances_indexable(meta, account_keys.len(), i) => {
            classify_balance_change(i, meta, account_keys)
        }
        _ => (TxDirection::Unknown, 0u128, String::new()),
    };

    TxHistoryItem {
        tx_hash,
        direction,
        counterparty: Address::new(counterparty),
        amount: Amount::from_raw(amount_lamports, SOL_DECIMALS),
        symbol: "SOL".into(),
        timestamp: sig.block_time.or(detail.block_time),
        status,
    }
}

/// Pre-flight check that `meta.preBalances` / `postBalances` are
/// well-formed and indexable for our account position.
fn balances_indexable(meta: &TxMeta, num_accounts: usize, our_idx: usize) -> bool {
    meta.pre_balances.len() == num_accounts
        && meta.post_balances.len() == num_accounts
        && our_idx < num_accounts
}

/// Derive direction, |amount|, and counterparty from the balance vectors.
///
/// Direction is the sign of our account's lamport delta. When we are the
/// fee payer (account[0] in Solana convention) the fee is included in our
/// delta, so we subtract it from the reported amount to surface the true
/// transferred value rather than `transferred + fee`. Counterparty is the
/// first non-self account whose delta has the opposite sign — works
/// cleanly for `system::transfer`, best-effort for complex multi-leg txs.
fn classify_balance_change(
    our_idx: usize,
    meta: &TxMeta,
    account_keys: &[AccountKey],
) -> (TxDirection, u128, String) {
    let our_delta = i128::from(meta.post_balances[our_idx])
        .saturating_sub(i128::from(meta.pre_balances[our_idx]));
    let is_fee_payer = our_idx == 0;
    let fee = u128::from(meta.fee.unwrap_or(0));

    let (direction, amount) = match our_delta.cmp(&0) {
        // We received lamports — the delta itself is the transfer amount.
        // Fee payment never increases an account's balance.
        std::cmp::Ordering::Greater => (TxDirection::In, our_delta.unsigned_abs()),
        std::cmp::Ordering::Less => {
            let abs = our_delta.unsigned_abs();
            // For the fee payer, |delta| = transferred + fee. Surface the
            // transferred portion only; saturating_sub guards the rare
            // case where a malformed RPC reports |delta| < fee.
            let amount = if is_fee_payer {
                abs.saturating_sub(fee)
            } else {
                abs
            };
            (TxDirection::Out, amount)
        }
        // Net zero (rare: self-transfer paid by another, no-op tx, or
        // pure-token tx that didn't move SOL). Direction is genuinely
        // ambiguous.
        std::cmp::Ordering::Equal => (TxDirection::Unknown, 0),
    };

    let counterparty = account_keys
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != our_idx)
        .find_map(|(j, k)| {
            let other_delta =
                i128::from(meta.post_balances[j]).saturating_sub(i128::from(meta.pre_balances[j]));
            let opposite_sign =
                (our_delta > 0 && other_delta < 0) || (our_delta < 0 && other_delta > 0);
            opposite_sign.then(|| k.pubkey.clone())
        })
        .unwrap_or_default();

    (direction, amount, counterparty)
}

/// Pick the most informative status from signature + tx detail.
///
/// The signature object is authoritative for confirmation; if either
/// source reports an error we surface `Failed`.
fn derive_status(sig: &SignatureInfo, detail: Option<&TxDetail>) -> TxConfirmation {
    let detail_failed = detail
        .and_then(|d| d.meta.as_ref())
        .is_some_and(|m| m.err.is_some());
    if sig.err.is_some() || detail_failed {
        return TxConfirmation::Failed;
    }
    if sig.confirmation_status.as_deref() == Some("finalized") {
        return TxConfirmation::Confirmed;
    }
    TxConfirmation::Pending
}

// ---------------------------------------------------------------------------
// JSON-RPC response shapes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SignaturesResponse {
    result: Option<Vec<SignatureInfo>>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SignatureInfo {
    signature: String,
    #[serde(default)]
    block_time: Option<i64>,
    #[serde(default)]
    err: Option<Value>,
    #[serde(default)]
    confirmation_status: Option<String>,
}

/// Generic envelope for a single response inside a JSON-RPC batch reply.
#[derive(Deserialize)]
struct RpcEnvelope<T> {
    id: Option<usize>,
    result: Option<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TxDetail {
    #[serde(default)]
    block_time: Option<i64>,
    #[serde(default)]
    meta: Option<TxMeta>,
    #[serde(default)]
    transaction: Option<TxEnvelope>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TxMeta {
    #[serde(default)]
    err: Option<Value>,
    #[serde(default)]
    fee: Option<u64>,
    #[serde(default)]
    pre_balances: Vec<u64>,
    #[serde(default)]
    post_balances: Vec<u64>,
}

#[derive(Deserialize)]
struct TxEnvelope {
    #[serde(default)]
    message: Option<TxMessage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TxMessage {
    #[serde(default)]
    account_keys: Vec<AccountKey>,
}

#[derive(Deserialize)]
struct AccountKey {
    pubkey: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal `TxDetail` with the given account keys and balances.
    fn detail(keys: &[&str], pre: &[u64], post: &[u64], fee: u64) -> TxDetail {
        TxDetail {
            block_time: Some(42),
            meta: Some(TxMeta {
                err: None,
                fee: Some(fee),
                pre_balances: pre.to_vec(),
                post_balances: post.to_vec(),
            }),
            transaction: Some(TxEnvelope {
                message: Some(TxMessage {
                    account_keys: keys
                        .iter()
                        .map(|k| AccountKey {
                            pubkey: (*k).into(),
                        })
                        .collect(),
                }),
            }),
        }
    }

    fn sig(s: &str) -> SignatureInfo {
        SignatureInfo {
            signature: s.into(),
            block_time: Some(100),
            err: None,
            confirmation_status: Some("finalized".into()),
        }
    }

    #[test]
    fn classify_outbound_fee_payer_subtracts_fee() {
        // We're at index 0 (fee payer), sent 1_000_000 lamports to index 1,
        // paid 5_000 fee. Our delta = -(1_000_000 + 5_000).
        let d = detail(
            &["us", "them"],
            &[2_000_000, 0],
            &[2_000_000 - 1_005_000, 1_000_000],
            5_000,
        );
        let item = classify("us", &sig("S1"), Some(&d));
        assert!(matches!(item.direction, TxDirection::Out));
        assert_eq!(item.amount.raw(), 1_000_000);
        assert_eq!(item.counterparty.as_str(), "them");
    }

    #[test]
    fn classify_inbound_uses_raw_delta() {
        // We're at index 1 (recipient), index 0 sent 250_000 lamports + paid
        // a 5_000 fee. Our delta is just +250_000.
        let d = detail(
            &["sender", "us"],
            &[1_000_000, 100_000],
            &[1_000_000 - 255_000, 100_000 + 250_000],
            5_000,
        );
        let item = classify("us", &sig("S2"), Some(&d));
        assert!(matches!(item.direction, TxDirection::In));
        assert_eq!(item.amount.raw(), 250_000);
        assert_eq!(item.counterparty.as_str(), "sender");
    }

    #[test]
    fn classify_outbound_non_fee_payer_does_not_subtract_fee() {
        // We're at index 1 (signer but not fee payer), sent 600 lamports.
        // Index 0 paid the 5_000 fee. Our delta should be -600 (no fee
        // adjustment).
        let d = detail(
            &["payer", "us", "them"],
            &[10_000, 1_000, 0],
            &[10_000 - 5_000, 1_000 - 600, 600],
            5_000,
        );
        let item = classify("us", &sig("S3"), Some(&d));
        assert!(matches!(item.direction, TxDirection::Out));
        assert_eq!(item.amount.raw(), 600);
        assert_eq!(item.counterparty.as_str(), "them");
    }

    #[test]
    fn classify_zero_delta_is_unknown() {
        // Our balance didn't change (e.g. a token-only tx where we only
        // appeared as a token-account owner, not a SOL holder).
        let d = detail(&["us", "other"], &[100, 200], &[100, 195], 5);
        let item = classify("us", &sig("S4"), Some(&d));
        assert!(matches!(item.direction, TxDirection::Unknown));
        assert_eq!(item.amount.raw(), 0);
    }

    #[test]
    fn classify_address_not_in_keys_is_unknown() {
        // Edge case: the signature listed our address (e.g. via lookup
        // table or token account) but the parsed message doesn't include
        // it directly.
        let d = detail(&["a", "b"], &[10, 20], &[5, 25], 0);
        let item = classify("us", &sig("S5"), Some(&d));
        assert!(matches!(item.direction, TxDirection::Unknown));
        assert_eq!(item.amount.raw(), 0);
        assert_eq!(item.counterparty.as_str(), "");
    }

    #[test]
    fn classify_no_detail_is_unknown_but_keeps_status() {
        // Pending sig (not yet finalized) with no detail available.
        let mut s = sig("S6");
        s.confirmation_status = Some("processed".into());
        let item = classify("us", &s, None);
        assert!(matches!(item.direction, TxDirection::Unknown));
        assert!(matches!(item.status, TxConfirmation::Pending));
    }

    #[test]
    fn classify_failed_tx_reports_failed_status() {
        let mut s = sig("S7");
        s.err = Some(serde_json::json!({"InstructionError": [0, "Custom"]}));
        let item = classify("us", &s, None);
        assert!(matches!(item.status, TxConfirmation::Failed));
    }

    #[test]
    fn classify_failed_status_from_meta_when_sig_lacks_err() {
        // The sig list sometimes reports null err while the full tx detail
        // carries the failure. Trust whichever surfaces a failure.
        let mut d = detail(&["us"], &[100], &[100], 0);
        d.meta.as_mut().expect("meta").err = Some(serde_json::json!("AccountInUse"));
        let item = classify("us", &sig("S8"), Some(&d));
        assert!(matches!(item.status, TxConfirmation::Failed));
    }

    #[test]
    fn classify_balance_length_mismatch_is_unknown() {
        // Malformed RPC response: account_keys says 2 accounts, balances
        // say 1. Don't index out of bounds.
        let d = TxDetail {
            block_time: Some(0),
            meta: Some(TxMeta {
                err: None,
                fee: Some(0),
                pre_balances: vec![100],
                post_balances: vec![50, 50],
            }),
            transaction: Some(TxEnvelope {
                message: Some(TxMessage {
                    account_keys: vec![
                        AccountKey {
                            pubkey: "us".into(),
                        },
                        AccountKey {
                            pubkey: "other".into(),
                        },
                    ],
                }),
            }),
        };
        let item = classify("us", &sig("S9"), Some(&d));
        assert!(matches!(item.direction, TxDirection::Unknown));
    }

    #[test]
    fn classify_fee_payer_with_underflow_safe_amount() {
        // Pathological: fee payer's |delta| < fee (RPC bug). Should not
        // overflow; surface zero amount and Out direction.
        let d = detail(&["us", "them"], &[10_000, 0], &[8_000, 0], 5_000);
        let item = classify("us", &sig("S10"), Some(&d));
        assert!(matches!(item.direction, TxDirection::Out));
        // |delta| = 2_000, fee = 5_000 → saturating_sub gives 0.
        assert_eq!(item.amount.raw(), 0);
    }
}

// Rust guideline compliant 2026-02-21
