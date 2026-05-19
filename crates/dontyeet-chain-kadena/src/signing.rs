//! Pure-crypto Pact `coin.transfer` envelope builder for Kadena.
//!
//! Builds a signed `coin.transfer` transaction envelope ready for the
//! Chainweb `/send` endpoint. The pipeline is:
//!
//! 1. Construct the Pact `cmd` payload (network, code, signers with the
//!    `coin.GAS` and `coin.TRANSFER` capabilities, gas meta, nonce).
//! 2. Serialize the payload to a JSON string. The exact bytes of that
//!    string are what the rest of the pipeline operates on; serializing
//!    a second time would produce a different hash on field reordering.
//! 3. Compute Blake2b-256 over the cmd string and encode it as
//!    URL-safe base64 without padding. This is Kadena's request key.
//! 4. Sign the *raw 32 hash bytes* with Ed25519 (Pact verifies signatures
//!    against the same hash, not the base64 form).
//! 5. Wrap into the `{hash, sigs, cmd}` envelope and serialize for the
//!    broadcaster.
//!
//! Network-agnostic — the [`build_signed_transfer`] function takes
//! `network_version` as a parameter (`"mainnet01"` for both the
//! community fork and the legacy chain, `"testnet05"` for community
//! testnet) so the same signing core serves all three networks
//! configured in [`crate::plugin::kadena_plugin`]. The browser-side
//! [`crate::wasm::send`] defaults to `mainnet01` against the community
//! Chainweb endpoint; legacy and testnet calls fall back to the
//! server-proxied Path A path until a follow-up phase wires up
//! per-network browser routing.
//!
//! Lives outside the `feature = "rpc"` gate (Phase M.4.8) so browser
//! consumers (`default-features = false`) can sign without pulling in
//! `reqwest`, `tokio`, or any other server-only dependency.

use base64::Engine;
use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use ed25519_dalek::{Signer, SigningKey, Verifier};
use serde::Serialize;

use dontyeet_primitives::{Address, Amount, DontYeetWalletError, PrivateKey, Result};

/// Default Chainweb chain to broadcast on (Kadena has 20 parallel chains).
///
/// Chain 0 is the canonical default for single-chain user wallets and
/// matches the choice made by [`crate::balance`] and
/// [`crate::broadcast`].
const DEFAULT_CHAIN: &str = "0";

/// How long the signed transaction stays valid for inclusion (seconds).
///
/// 10 minutes is the value the official `chainweaver` wallet uses; long
/// enough to survive normal mempool latency, short enough that abandoned
/// transactions don't linger.
const DEFAULT_TTL_SECONDS: u64 = 600;

/// Default gas limit for a `coin.transfer`.
///
/// Mirrors `crate::fees::SIMPLE_TRANSFER_GAS_LIMIT` (600 gas). The fee
/// estimator is the surface for tuning; this builder accepts the
/// resolved values.
const DEFAULT_GAS_LIMIT: u64 = 600;

/// Default gas price (network minimum, 1e-8 KDA).
const DEFAULT_GAS_PRICE: f64 = 1e-8;

/// Blake2b-256 (32-byte output) used for the Pact command hash.
type Blake2b256 = Blake2b<U32>;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Build a signed Pact `coin.transfer` envelope ready for broadcast.
///
/// Returns the JSON envelope bytes that
/// [`crate::broadcast::KadenaBroadcaster`] (server) and
/// [`crate::wasm::send`] (browser) post to the Chainweb
/// `/pact/api/v1/send` endpoint:
/// `{"hash": ..., "sigs": [...], "cmd": ...}`.
///
/// `network_version` is the on-the-wire `networkId` field — pass
/// `"mainnet01"` for both the community fork and the legacy chain
/// (they share the network id but different endpoints; the caller
/// picks the endpoint), `"testnet05"` for community testnet.
///
/// `creation_time_secs` is the Unix epoch second the transaction was
/// stamped at; the broadcaster will reject it after `creation_time +
/// ttl`. Tests pass a fixed value; production callers should pass the
/// current wall-clock time.
///
/// # Errors
/// Returns [`DontYeetWalletError::Validation`] on malformed addresses, zero
/// or overflowing amounts, or non-hex sender addresses;
/// [`DontYeetWalletError::Crypto`] on signing failure.
pub fn build_signed_transfer(
    network_version: &str,
    from: &Address,
    to: &Address,
    amount: &Amount,
    private_key: &PrivateKey,
    creation_time_secs: u64,
) -> Result<Vec<u8>> {
    if amount.is_zero() {
        return Err(DontYeetWalletError::Validation(
            "transfer amount must be greater than zero".into(),
        ));
    }

    let from_pubkey = pubkey_hex_from_k_address(from)?;
    validate_k_address(to)?;
    let amount_str = format_decimal(amount);

    let code = format!(
        "(coin.transfer \"{from_acc}\" \"{to_acc}\" {amt})",
        from_acc = from.as_str(),
        to_acc = to.as_str(),
        amt = amount_str,
    );

    let signer = PactSigner {
        scheme: "ED25519",
        pub_key: from_pubkey.clone(),
        addr: from_pubkey,
        clist: vec![
            PactCap {
                name: "coin.GAS".into(),
                args: Vec::new(),
            },
            PactCap {
                name: "coin.TRANSFER".into(),
                args: vec![
                    PactCapArg::Str(from.as_str().to_owned()),
                    PactCapArg::Str(to.as_str().to_owned()),
                    PactCapArg::Decimal {
                        decimal: amount_str.clone(),
                    },
                ],
            },
        ],
    };

    let cmd = PactCmd {
        network_id: network_version,
        payload: PactPayload {
            exec: PactExec {
                data: serde_json::json!({}),
                code,
            },
        },
        signers: vec![signer],
        meta: PactMeta {
            chain_id: DEFAULT_CHAIN,
            sender: from.as_str(),
            gas_limit: DEFAULT_GAS_LIMIT,
            gas_price: DEFAULT_GAS_PRICE,
            ttl: DEFAULT_TTL_SECONDS,
            creation_time: creation_time_secs,
        },
        nonce: format!("dontyeet-{creation_time_secs}"),
    };

    let cmd_json = serde_json::to_string(&cmd)
        .map_err(|e| DontYeetWalletError::Chain(format!("serialize Pact cmd: {e}")))?;

    let hash_bytes = blake2b_256(cmd_json.as_bytes());
    let hash_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash_bytes);

    let signature = sign_hash(&hash_bytes, private_key)?;
    let sig_hex = hex::encode(signature);

    let envelope = PactEnvelope {
        hash: hash_b64,
        sigs: vec![PactSig { sig: sig_hex }],
        cmd: cmd_json,
    };

    serde_json::to_vec(&envelope)
        .map_err(|e| DontYeetWalletError::Chain(format!("serialize Pact envelope: {e}")))
}

/// Sign 32 hash bytes with Ed25519 and return the 64-byte signature.
///
/// Used by [`build_signed_transfer`] internally and by the
/// [`crate::tx::KadenaTransactionSigner`] trait impl that satisfies
/// [`dontyeet_primitives::traits::ChainPlugin::signer`] on the server
/// side. Verifies the produced signature against the signer's public
/// key before returning, mirroring the M.4.1 EVM safety pattern.
///
/// `hash` is expected to be the Blake2b-256 of the canonical Pact
/// `cmd` string — Pact verifies signatures against this exact value.
///
/// # Errors
/// Returns [`DontYeetWalletError::Crypto`] if `private_key` isn't 32 bytes
/// or post-sign verification fails.
pub fn sign_hash(hash: &[u8], private_key: &PrivateKey) -> Result<[u8; 64]> {
    let key_bytes: [u8; 32] = private_key.as_bytes().try_into().map_err(|_| {
        DontYeetWalletError::Crypto(format!(
            "invalid Ed25519 key length: {} (expected 32)",
            private_key.as_bytes().len()
        ))
    })?;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let signature = signing_key.sign(hash);

    // Post-sign verification: catches faulty signing hardware or
    // memory errors before the bad sig reaches the network.
    signing_key
        .verifying_key()
        .verify(hash, &signature)
        .map_err(|e| DontYeetWalletError::Crypto(format!("post-sign verification failed: {e}")))?;

    Ok(signature.to_bytes())
}

/// Compute Blake2b-256 of `data` and return the 32 raw output bytes.
///
/// Exposed publicly so callers (e.g. tests) can re-derive the hash
/// from a serialized cmd string and verify envelope integrity.
#[must_use]
pub fn blake2b_256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2b256::new();
    hasher.update(data);
    let out = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&out);
    bytes
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract the 32-byte Ed25519 public key (hex) from a `k:` account.
fn pubkey_hex_from_k_address(addr: &Address) -> Result<String> {
    let s = addr.as_str();
    let hex_part = s.strip_prefix("k:").ok_or_else(|| {
        DontYeetWalletError::Validation(format!(
            "Kadena sender address must start with \"k:\", got {s:?}"
        ))
    })?;
    if hex_part.len() != 64 {
        return Err(DontYeetWalletError::Validation(format!(
            "Kadena pubkey must be 64 hex chars, got {}",
            hex_part.len()
        )));
    }
    if !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(DontYeetWalletError::Validation(
            "Kadena address contains non-hex characters".into(),
        ));
    }
    Ok(hex_part.to_owned())
}

/// Reject anything that isn't a well-formed `k:` address.
///
/// Pact does support other account formats (`w:`, vanity names) but
/// our wallet only ever derives `k:` addresses, so accepting other
/// forms here would silently let users send to accounts they cannot
/// themselves receive on. Validation is conservative on purpose.
fn validate_k_address(addr: &Address) -> Result<()> {
    pubkey_hex_from_k_address(addr).map(|_| ())
}

/// Format an amount with KDA's 12-decimal precision, always emitting a
/// fractional component so the Pact parser sees a decimal literal.
///
/// `Amount::to_display_string` strips trailing zeros and may produce an
/// integer (`"100"`); Pact would then parse that as the integer type
/// and reject the `coin.transfer` invocation.
fn format_decimal(amount: &Amount) -> String {
    let raw = amount.raw();
    let decimals = amount.decimals();
    if decimals == 0 {
        return format!("{raw}.0");
    }
    let divisor = 10u128.pow(u32::from(decimals));
    let whole = raw / divisor;
    let frac = raw % divisor;
    let frac_str = format!("{frac:0>width$}", width = decimals as usize);
    let trimmed = frac_str.trim_end_matches('0');
    if trimmed.is_empty() {
        format!("{whole}.0")
    } else {
        format!("{whole}.{trimmed}")
    }
}

// ---------------------------------------------------------------------------
// Pact wire types
// ---------------------------------------------------------------------------
//
// Field declaration order matters: `serde_json` serializes structs in
// declaration order, which is the order Pact expects on the wire. The
// hash is computed over this serialized form, so reordering would
// invalidate signatures even though the JSON is semantically identical.

#[derive(Serialize)]
struct PactEnvelope {
    hash: String,
    sigs: Vec<PactSig>,
    cmd: String,
}

#[derive(Serialize)]
struct PactSig {
    sig: String,
}

#[derive(Serialize)]
struct PactCmd<'a> {
    #[serde(rename = "networkId")]
    network_id: &'a str,
    payload: PactPayload,
    signers: Vec<PactSigner>,
    meta: PactMeta<'a>,
    nonce: String,
}

#[derive(Serialize)]
struct PactPayload {
    exec: PactExec,
}

#[derive(Serialize)]
struct PactExec {
    data: serde_json::Value,
    code: String,
}

#[derive(Serialize)]
struct PactSigner {
    scheme: &'static str,
    #[serde(rename = "pubKey")]
    pub_key: String,
    addr: String,
    clist: Vec<PactCap>,
}

#[derive(Serialize)]
struct PactCap {
    name: String,
    args: Vec<PactCapArg>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum PactCapArg {
    Str(String),
    Decimal {
        #[serde(rename = "decimal")]
        decimal: String,
    },
}

#[derive(Serialize)]
struct PactMeta<'a> {
    #[serde(rename = "chainId")]
    chain_id: &'a str,
    sender: &'a str,
    #[serde(rename = "gasLimit")]
    gas_limit: u64,
    #[serde(rename = "gasPrice")]
    gas_price: f64,
    ttl: u64,
    #[serde(rename = "creationTime")]
    creation_time: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::VerifyingKey;

    /// Deterministic test key (sender pubkey is derived from this).
    const TEST_PRIVATE_KEY: [u8; 32] = [42u8; 32];

    fn test_sender() -> Address {
        let signing_key = SigningKey::from_bytes(&TEST_PRIVATE_KEY);
        let pub_hex = hex::encode(signing_key.verifying_key().as_bytes());
        Address::new(format!("k:{pub_hex}"))
    }

    fn test_recipient() -> Address {
        Address::new(format!("k:{}", "ab".repeat(32)))
    }

    #[test]
    fn format_decimal_always_has_fractional_part() {
        assert_eq!(format_decimal(&Amount::from_raw(0, 12)), "0.0");
        assert_eq!(
            format_decimal(&Amount::from_raw(1_000_000_000_000, 12)),
            "1.0"
        );
        assert_eq!(
            format_decimal(&Amount::from_raw(1_500_000_000_000, 12)),
            "1.5"
        );
        assert_eq!(
            format_decimal(&Amount::from_raw(123_456_789_012_345, 12)),
            "123.456789012345"
        );
    }

    #[test]
    fn pubkey_hex_extracted_from_k_address() {
        let addr = Address::new(format!("k:{}", "cd".repeat(32)));
        assert_eq!(
            pubkey_hex_from_k_address(&addr).expect("ok"),
            "cd".repeat(32)
        );
    }

    #[test]
    fn pubkey_hex_rejects_wrong_prefix_length_and_chars() {
        assert!(pubkey_hex_from_k_address(&Address::new("foo")).is_err());
        assert!(pubkey_hex_from_k_address(&Address::new("k:short")).is_err());
        assert!(
            pubkey_hex_from_k_address(&Address::new(format!("k:{}", "zz".repeat(32)))).is_err()
        );
    }

    #[test]
    fn rejects_zero_amount() {
        let pk = PrivateKey::new(TEST_PRIVATE_KEY.to_vec());
        let result = build_signed_transfer(
            "mainnet01",
            &test_sender(),
            &test_recipient(),
            &Amount::from_raw(0, 12),
            &pk,
            1_700_000_000,
        );
        assert!(matches!(result, Err(DontYeetWalletError::Validation(_))));
    }

    #[test]
    fn rejects_invalid_recipient() {
        let pk = PrivateKey::new(TEST_PRIVATE_KEY.to_vec());
        let result = build_signed_transfer(
            "mainnet01",
            &test_sender(),
            &Address::new("not-a-k-address"),
            &Amount::from_raw(1_000_000_000_000, 12),
            &pk,
            1_700_000_000,
        );
        assert!(matches!(result, Err(DontYeetWalletError::Validation(_))));
    }

    #[test]
    fn envelope_round_trip_and_hash_matches_cmd() {
        let pk = PrivateKey::new(TEST_PRIVATE_KEY.to_vec());
        let bytes = build_signed_transfer(
            "mainnet01",
            &test_sender(),
            &test_recipient(),
            &Amount::from_raw(1_500_000_000_000, 12),
            &pk,
            1_700_000_000,
        )
        .expect("build");

        let env: serde_json::Value = serde_json::from_slice(&bytes).expect("parse");
        let cmd_str = env["cmd"].as_str().expect("cmd is string");
        let hash_b64 = env["hash"].as_str().expect("hash is string");

        // The advertised hash must equal Blake2b-256 of the cmd string.
        let recomputed = blake2b_256(cmd_str.as_bytes());
        let recomputed_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(recomputed);
        assert_eq!(hash_b64, recomputed_b64);

        // The cmd contains the expected coin.transfer call.
        let cmd: serde_json::Value = serde_json::from_str(cmd_str).expect("cmd JSON");
        let code = cmd["payload"]["exec"]["code"].as_str().expect("code");
        assert!(code.starts_with("(coin.transfer "));
        assert!(code.contains("1.5"));
        assert_eq!(cmd["meta"]["chainId"], "0");
        assert_eq!(cmd["meta"]["gasLimit"], 600);
        assert_eq!(cmd["networkId"], "mainnet01");
    }

    #[test]
    fn envelope_carries_testnet_network_id() {
        let pk = PrivateKey::new(TEST_PRIVATE_KEY.to_vec());
        let bytes = build_signed_transfer(
            "testnet05",
            &test_sender(),
            &test_recipient(),
            &Amount::from_raw(1_000_000_000_000, 12),
            &pk,
            1_700_000_000,
        )
        .expect("build");
        let env: serde_json::Value = serde_json::from_slice(&bytes).expect("parse");
        let cmd: serde_json::Value =
            serde_json::from_str(env["cmd"].as_str().expect("cmd")).expect("cmd JSON");
        assert_eq!(cmd["networkId"], "testnet05");
    }

    #[test]
    fn signature_verifies_against_hash_with_sender_pubkey() {
        let pk = PrivateKey::new(TEST_PRIVATE_KEY.to_vec());
        let bytes = build_signed_transfer(
            "mainnet01",
            &test_sender(),
            &test_recipient(),
            &Amount::from_raw(1_000_000_000_000, 12),
            &pk,
            1_700_000_000,
        )
        .expect("build");

        let env: serde_json::Value = serde_json::from_slice(&bytes).expect("parse");
        let cmd_str = env["cmd"].as_str().expect("cmd");
        let sig_hex = env["sigs"][0]["sig"].as_str().expect("sig");

        let hash = blake2b_256(cmd_str.as_bytes());
        let sig_bytes: [u8; 64] = hex::decode(sig_hex)
            .expect("hex")
            .try_into()
            .expect("64 bytes");

        let signing_key = SigningKey::from_bytes(&TEST_PRIVATE_KEY);
        let verifying: VerifyingKey = signing_key.verifying_key();
        assert!(
            verifying
                .verify(&hash, &ed25519_dalek::Signature::from_bytes(&sig_bytes))
                .is_ok()
        );
    }

    #[test]
    fn build_is_deterministic_given_same_inputs() {
        let pk = PrivateKey::new(TEST_PRIVATE_KEY.to_vec());
        let a = build_signed_transfer(
            "mainnet01",
            &test_sender(),
            &test_recipient(),
            &Amount::from_raw(1_000_000_000_000, 12),
            &pk,
            1_700_000_000,
        )
        .expect("a");
        let b = build_signed_transfer(
            "mainnet01",
            &test_sender(),
            &test_recipient(),
            &Amount::from_raw(1_000_000_000_000, 12),
            &pk,
            1_700_000_000,
        )
        .expect("b");
        assert_eq!(a, b);
    }

    #[test]
    fn sign_hash_returns_64_bytes() {
        let pk = PrivateKey::new(TEST_PRIVATE_KEY.to_vec());
        let hash = [7u8; 32];
        let sig = sign_hash(&hash, &pk).expect("sign");
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn sign_hash_rejects_short_key() {
        let pk = PrivateKey::new(vec![0u8; 16]);
        assert!(matches!(
            sign_hash(&[7u8; 32], &pk),
            Err(DontYeetWalletError::Crypto(_))
        ));
    }
}

// Rust guideline compliant 2026-02-21
