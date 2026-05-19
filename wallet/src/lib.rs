//! DontYeetWallet — non-custodial WASM wallet for Yeet Social.
//!
//! Thin wasm-bindgen surface over an audited, non-custodial cryptographic
//! engine. The only logic this crate owns is the JS boundary and a
//! browser-localStorage backend (see [`storage`]); every primitive —
//! BIP-39 mnemonic generation, BIP-44 derivation, EIP-55 address
//! encoding, EIP-155 transaction signing, AES-GCM at-rest encryption,
//! Argon2id password hashing — delegates to the upstream engine.
//!
//! ## What this crate is
//!
//! - A small bundle (built via `wasm-pack`) loaded by Yeet's `index.html`.
//! - The crypto layer behind email-signup wallets, login, tipping, NFT
//!   mints, and mnemonic export.
//!
//! ## What this crate is not
//!
//! - An RPC client. The browser talks to BSC directly via `fetch` from JS.
//! - A UI. Yeet's `index.html` owns presentation; this crate owns crypto.
//! - A re-implementation of anything in the engine. New primitives belong
//!   upstream, not here.
//!
//! ## Public name
//!
//! User-facing strings always say "Wallet". The `DontYeetWallet` name
//! appears only in this crate's code as a developer joke ("don't yeet
//! your funds"). The exported wasm-bindgen surface is plain free
//! functions — see [`create_wallet`], [`login`], [`sign_evm_transfer`], etc.

#![deny(clippy::all, clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]
// `wasm_bindgen` generates unused-async warnings on some stubs; tolerate them.
#![allow(clippy::unused_async)]
// Brand names (DontYeetWallet, WebAuthn) read as prose in the narrative docs;
// backticking them everywhere clutters the rendered output without value.
#![expect(
    clippy::doc_markdown,
    reason = "brand names are intentionally written without backticks in prose"
)]

mod storage;

use std::sync::OnceLock;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use dontyeet_account::AccountManager;
use dontyeet_chain_evm::{
    derive_address,
    keys::derive_keypair,
    signing::{rlp_encode_unsigned, sign_legacy_tx},
};
use dontyeet_crypto::cipher::AesGcmCipher;
use dontyeet_crypto::derivation::paths as bip_paths;
use dontyeet_crypto::mnemonic::{Bip39Generator, WordCount};
use dontyeet_primitives::{Mnemonic, Seed};
use dontyeet_storage::{EncryptedStore, KeyValueBackend};

use crate::storage::BrowserStorage;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Developer-only tag. Appears in console logs and debug strings.
///
/// User-facing UI always says "Wallet". This constant is the internal joke
/// surface and is the only place the project name appears in code.
pub const DONTYEET_WALLET_TAG: &str = "DontYeetWallet";

/// BIP-44 derivation path for the primary BSC account.
///
/// Standard EVM path — `m/44'/60'/0'/0/0`, where coin-type 60 is the
/// Ethereum-family registration ([SLIP-44]). All Yeet user wallets share
/// this path; multi-account support would index further into the
/// `change`/`address_index` segments. Do not change without a migration
/// story — every existing user's address is derived from this path.
///
/// [SLIP-44]: https://github.com/satoshilabs/slips/blob/master/slip-0044.md
const BIP44_EVM_PATH: &str = "m/44'/60'/0'/0/0";

/// localStorage sub-namespace for unlock-factor blobs.
///
/// Each enabled factor stores its encrypted-password ciphertext at
/// `factor:<label>` (which becomes `dontyeet:factor:<label>` in the raw
/// localStorage view, after [`storage::BrowserStorage`]'s prefix is
/// applied).
const FACTOR_KEY_PREFIX: &str = "factor:";

/// Required length of an externally-supplied factor key, in bytes.
///
/// AES-256-GCM takes a 32-byte key. The wallet shim is deliberately
/// strict about this so JS can't accidentally supply a truncated or
/// padded blob from a misconfigured WebAuthn PRF / signature hash.
const FACTOR_KEY_LEN: usize = 32;

/// Concrete `AccountManager` parameterized for the browser.
///
/// `AccountManager` is generic over a storage backend and a cipher; this
/// crate monomorphizes it to the browser localStorage backend and AES-GCM.
/// Per M-DI-HIERARCHY, prefer concrete types over generics at the API
/// boundary — wasm-bindgen can't expose generics to JS anyway.
type DontYeetWallet = AccountManager<BrowserStorage, AesGcmCipher>;

// ---------------------------------------------------------------------------
// Singleton state
// ---------------------------------------------------------------------------
//
// M-AVOID-STATICS says libraries should avoid `static` items if a consistent
// view matters for correctness. The rationale is cross-crate version mismatch
// causing duplicated statics in a single binary.
//
// That risk does not apply here:
//   - This crate is a `cdylib` — exactly one wasm artifact per browser tab.
//   - JS is single-threaded; there are no thread-locality concerns.
//   - The "wallet" is the user's single wallet for this origin.
//
// The alternative (export a `Wallet` JS class and have JS hold a handle)
// adds friction without buying anything in this environment.

/// Process-wide wallet handle.
///
/// Initialized on the first call to [`init_wallet`]; subsequent operations
/// borrow through [`wallet`]. The `OnceLock` makes initialization
/// idempotent and panic-free under JS's single-threaded event loop.
static WALLET: OnceLock<DontYeetWallet> = OnceLock::new();

/// Borrow the initialized wallet handle.
///
/// # Errors
/// Returns an error when [`init_wallet`] has not yet been called.
fn wallet() -> Result<&'static DontYeetWallet, JsError> {
    WALLET
        .get()
        .ok_or_else(|| JsError::new("wallet not initialised; call init_wallet() first"))
}

// ---------------------------------------------------------------------------
// wasm-bindgen exports
// ---------------------------------------------------------------------------

/// Module entry point — runs when the wasm bundle is instantiated.
///
/// Sets the panic hook so panics surface as readable console errors. Does
/// not initialize the wallet — that is the explicit job of [`init_wallet`].
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    let msg = format!(
        "{DONTYEET_WALLET_TAG} v{} loaded",
        env!("CARGO_PKG_VERSION")
    );
    web_sys::console::log_1(&msg.into());
}

/// Initialize the singleton wallet. Idempotent.
///
/// Probes localStorage to determine whether an account exists and sets the
/// session state to `Locked` (account present) or `NoAccount` (fresh
/// browser). Must be called once at page load before any other function.
///
/// # Errors
/// Returns a JS error if localStorage is unavailable (e.g. private-mode
/// browsers with storage disabled).
#[wasm_bindgen]
pub async fn init_wallet() -> Result<(), JsError> {
    let mgr = WALLET.get_or_init(|| {
        let store = EncryptedStore::new(BrowserStorage::new(), AesGcmCipher);
        AccountManager::new(store)
    });
    mgr.initialize().await.map_err(jserr)
}

/// Returns true if an encrypted wallet exists in this browser's storage.
///
/// # Errors
/// Returns a JS error if localStorage cannot be read.
#[wasm_bindgen]
pub async fn has_wallet() -> Result<bool, JsError> {
    wallet()?.exists().await.map_err(jserr)
}

/// Returns true if the wallet is currently unlocked.
///
/// # Errors
/// Returns a JS error if the internal session lock is poisoned.
#[wasm_bindgen]
pub fn is_unlocked() -> Result<bool, JsError> {
    wallet()?.is_logged_in().map_err(jserr)
}

/// Generate a fresh 12-word wallet and encrypt it under `password`.
///
/// Returns a JS object `{ address, mnemonic }`. The mnemonic is returned
/// **once** — after this call the only retrieval path is [`export_mnemonic`]
/// (which requires an unlocked session). The caller must show the mnemonic
/// to the user for backup before discarding the return value.
///
/// Leaves the session unlocked. No follow-up [`login`] is needed.
///
/// # Errors
/// Returns a JS error if an account already exists, the cipher rejects the
/// derived key, or localStorage cannot be written.
#[wasm_bindgen]
pub async fn create_wallet(password: String) -> Result<JsValue, JsError> {
    let mnemonic = Bip39Generator::generate(WordCount::Twelve).map_err(jserr)?;
    let phrase = mnemonic.as_str().to_string();
    let mgr = wallet()?;
    mgr.create(&mnemonic, &password).await.map_err(jserr)?;
    let address = derive_address_for(&mnemonic)?;
    let out = CreatedWallet {
        address,
        mnemonic: phrase,
    };
    serde_wasm_bindgen::to_value(&out).map_err(|e| JsError::new(&e.to_string()))
}

/// Import an existing BIP-39 mnemonic and encrypt it under `password`.
///
/// Used by the "Restore wallet" flow and by paper-wallet redemption. Fails
/// if a wallet already exists in this browser — the caller should clear
/// the existing wallet first (via [`logout`] + delete UX) or import on a
/// fresh browser.
///
/// Returns the derived BSC address.
///
/// # Errors
/// Returns a JS error if the mnemonic is not valid BIP-39, an account
/// already exists, or localStorage cannot be written.
#[wasm_bindgen]
pub async fn import_wallet(mnemonic: String, password: String) -> Result<String, JsError> {
    Bip39Generator::validate(&mnemonic).map_err(jserr)?;
    let m = Mnemonic::new(mnemonic);
    let mgr = wallet()?;
    mgr.create(&m, &password).await.map_err(jserr)?;
    derive_address_for(&m)
}

/// Unlock the wallet with the user's password.
///
/// Returns the wallet's BSC address on success.
///
/// # Errors
/// Returns a JS error if no wallet exists, the password is wrong, or
/// localStorage cannot be read.
#[wasm_bindgen]
pub async fn login(password: String) -> Result<String, JsError> {
    let mgr = wallet()?;
    mgr.login(&password).await.map_err(jserr)?;
    let m = mgr.get_mnemonic().await.map_err(jserr)?;
    derive_address_for(&m)
}

/// Lock the session, zeroizing the in-memory encryption key.
///
/// # Errors
/// Returns a JS error if the session lock is poisoned.
#[wasm_bindgen]
pub fn logout() -> Result<(), JsError> {
    wallet()?.logout().map_err(jserr)
}

/// Return the wallet's BSC address. Requires the session to be unlocked.
///
/// # Errors
/// Returns a JS error if the session is locked or no wallet exists.
#[wasm_bindgen]
pub async fn current_address() -> Result<String, JsError> {
    let mgr = wallet()?;
    let m = mgr.get_mnemonic().await.map_err(jserr)?;
    derive_address_for(&m)
}

/// Reveal the seed phrase. Requires the session to be unlocked.
///
/// Sensitive — callers must gate this behind a confirmation UI (and
/// ideally a WebAuthn assertion). The plaintext crosses the JS boundary
/// and lives in JS-side strings until garbage-collected.
///
/// # Errors
/// Returns a JS error if the session is locked.
#[wasm_bindgen]
pub async fn export_mnemonic() -> Result<String, JsError> {
    let mgr = wallet()?;
    let m = mgr.get_mnemonic().await.map_err(jserr)?;
    Ok(m.as_str().to_string())
}

/// Change the wallet's password, re-encrypting the stored mnemonic.
///
/// Requires the session to be unlocked (so the caller knows the current
/// password). Leaves the session unlocked under the new password.
///
/// # Errors
/// Returns a JS error if `current_password` is wrong or storage fails.
#[wasm_bindgen]
pub async fn change_password(
    current_password: String,
    new_password: String,
) -> Result<(), JsError> {
    wallet()?
        .change_password(&current_password, &new_password)
        .await
        .map_err(jserr)
}

/// Sign a legacy EIP-155 EVM transfer and return the broadcast-ready hex.
///
/// All large numeric values cross as decimal strings rather than `u64` to
/// dodge JS's 53-bit integer precision limit (1 BNB = 10^18 wei does not
/// fit). `to` is a 0x-prefixed 20-byte hex address. The returned string
/// is `0x`-prefixed RLP-encoded signed-tx hex, ready to pass to
/// `eth_sendRawTransaction`.
///
/// Requires the session to be unlocked.
///
/// # Errors
/// Returns a JS error if the session is locked, `to` is not a valid
/// 20-byte hex address, `value_wei` or `gas_price_wei` are not decimal
/// integers within `u128`, or if signing fails.
#[wasm_bindgen]
pub async fn sign_evm_transfer(
    to: String,
    value_wei: String,
    gas_price_wei: String,
    gas_limit: u64,
    nonce: u64,
    chain_id: u64,
) -> Result<String, JsError> {
    let mgr = wallet()?;
    let mnemonic = mgr.get_mnemonic().await.map_err(jserr)?;
    let seed = Bip39Generator::to_seed(&mnemonic, "").map_err(jserr)?;
    let kp = derive_keypair(&seed, BIP44_EVM_PATH).map_err(jserr)?;
    let pk = kp
        .private_key()
        .ok_or_else(|| JsError::new("derive_keypair returned no private key"))?;

    let to_bytes = parse_evm_address(&to)?;
    let value: u128 = value_wei
        .parse()
        .map_err(|e: std::num::ParseIntError| JsError::new(&format!("value_wei: {e}")))?;
    let gas_price: u128 = gas_price_wei
        .parse()
        .map_err(|e: std::num::ParseIntError| JsError::new(&format!("gas_price_wei: {e}")))?;
    let unsigned = rlp_encode_unsigned(
        nonce, gas_price, gas_limit, &to_bytes, value, &[], chain_id,
    );
    let signed = sign_legacy_tx(&unsigned, pk, chain_id).map_err(jserr)?;
    Ok(format!("0x{}", hex::encode(signed)))
}

/// Sign an arbitrary contract call (data-carrying EVM transaction).
///
/// Identical to [`sign_evm_transfer`] but carries a hex-encoded `data`
/// payload — the encoded function call (e.g. `transfer(address,uint256)`
/// for an ERC-20 / BEP-20 token send). `value_wei` is the native-token
/// value attached to the call (usually `"0"` for token transfers).
///
/// Requires the session to be unlocked.
///
/// # Errors
/// Returns a JS error if the session is locked, address/data hex is
/// malformed, numeric strings overflow, or signing fails.
#[wasm_bindgen]
pub async fn sign_evm_call(
    to: String,
    value_wei: String,
    data_hex: String,
    gas_price_wei: String,
    gas_limit: u64,
    nonce: u64,
    chain_id: u64,
) -> Result<String, JsError> {
    let mgr = wallet()?;
    let mnemonic = mgr.get_mnemonic().await.map_err(jserr)?;
    let seed = Bip39Generator::to_seed(&mnemonic, "").map_err(jserr)?;
    let kp = derive_keypair(&seed, BIP44_EVM_PATH).map_err(jserr)?;
    let pk = kp
        .private_key()
        .ok_or_else(|| JsError::new("derive_keypair returned no private key"))?;

    let to_bytes = parse_evm_address(&to)?;
    let data = parse_hex(&data_hex)?;
    let value: u128 = value_wei
        .parse()
        .map_err(|e: std::num::ParseIntError| JsError::new(&format!("value_wei: {e}")))?;
    let gas_price: u128 = gas_price_wei
        .parse()
        .map_err(|e: std::num::ParseIntError| JsError::new(&format!("gas_price_wei: {e}")))?;
    let unsigned = rlp_encode_unsigned(
        nonce, gas_price, gas_limit, &to_bytes, value, &data, chain_id,
    );
    let signed = sign_legacy_tx(&unsigned, pk, chain_id).map_err(jserr)?;
    Ok(format!("0x{}", hex::encode(signed)))
}

// ---------------------------------------------------------------------------
// Multichain receive — address derivation for every supported chain
// ---------------------------------------------------------------------------

/// Bech32 human-readable prefix for Bitcoin mainnet P2WPKH addresses.
///
/// Switch to `"tb"` for testnet / `"bcrt"` for regtest when we wire a
/// network selector on the receive view. Hardcoded to mainnet for now.
const BITCOIN_BECH32_HRP: &str = "bc";

/// Derive the user's address on `chain_id` from their stored mnemonic.
///
/// `chain_id` is the same string used in [`supported_chains`] (e.g.
/// `"bitcoin"`, `"ethereum"`, `"solana"`). EVM-compatible chains
/// (Ethereum, BSC, Polygon, Avalanche, Sonic) all share the standard
/// Ethereum BIP-44 path so they resolve to the same address — that's
/// the expected behaviour for every EVM wallet on the market.
///
/// Requires the session to be unlocked.
///
/// # Errors
/// Returns a JS error if the session is locked, `chain_id` is not
/// recognised, or address derivation fails for the chosen chain.
#[wasm_bindgen]
pub async fn get_chain_address(chain_id: String) -> Result<String, JsError> {
    let mgr = wallet()?;
    let mnemonic = mgr.get_mnemonic().await.map_err(jserr)?;
    let seed = Bip39Generator::to_seed(&mnemonic, "").map_err(jserr)?;
    derive_chain_address(&seed, &chain_id)
}

/// List the chains the wallet can derive addresses for.
///
/// Returned as a JS array of `{ id, name }` objects, in the order
/// chains should appear in the UI's receive view.
///
/// # Errors
/// Returns a JS error only if the underlying JSON conversion fails
/// (which should never happen for this static list).
#[wasm_bindgen]
pub fn supported_chains() -> Result<JsValue, JsError> {
    #[derive(Serialize)]
    struct ChainInfo {
        id: &'static str,
        name: &'static str,
        symbol: &'static str,
        decimals: u32,
    }
    let chains = SUPPORTED_CHAINS
        .iter()
        .map(|&(id, name, symbol, decimals)| ChainInfo {
            id,
            name,
            symbol,
            decimals,
        })
        .collect::<Vec<_>>();
    serde_wasm_bindgen::to_value(&chains).map_err(|e| JsError::new(&e.to_string()))
}

/// Display order for the multichain wallet view. The leading entry is
/// the platform's native chain so the user lands on it by default;
/// everything else follows market-cap-ish order. Each row carries the
/// native asset symbol and base-10 decimals so the frontend can
/// convert user-typed amounts to smallest-unit without a separate RPC
/// round-trip.
const SUPPORTED_CHAINS: &[(&str, &str, &str, u32)] = &[
    ("bsc", "BNB Smart Chain (YEET)", "BNB", 18),
    ("ethereum", "Ethereum", "ETH", 18),
    ("polygon", "Polygon", "MATIC", 18),
    ("avalanche", "Avalanche C-Chain", "AVAX", 18),
    ("sonic", "Sonic", "S", 18),
    ("bitcoin", "Bitcoin", "BTC", 8),
    ("solana", "Solana", "SOL", 9),
    ("cardano", "Cardano", "ADA", 6),
    ("xrp", "XRP Ledger", "XRP", 6),
    ("algorand", "Algorand", "ALGO", 6),
    ("tron", "Tron", "TRX", 6),
    ("kaspa", "Kaspa", "KAS", 8),
    ("kadena", "Kadena", "KDA", 12),
];

// ---------------------------------------------------------------------------
// Multichain send + balance (full pipeline)
// ---------------------------------------------------------------------------

/// Send a native transfer of `amount_raw` (smallest unit, decimal
/// string) on `chain_id` to address `to`.
///
/// Builds and broadcasts entirely in-browser via each chain crate's
/// `wasm::send`. Returns the transaction hash on success.
///
/// `amount_raw` is the smallest unit of the chain — wei for EVM,
/// satoshi for Bitcoin, lamports for Solana, drops for XRP, etc. The
/// frontend converts user-typed "0.5 ETH" → "500000000000000000" wei
/// before calling. Keeping the boundary at the smallest-unit avoids
/// float-precision bugs at the wasm seam.
///
/// Requires the session to be unlocked.
///
/// # Errors
/// Returns a JS error if the session is locked, `chain_id` is not
/// supported by the dispatcher, or the chain's build/broadcast fails
/// (insufficient balance, RPC down, invalid recipient).
#[wasm_bindgen]
pub async fn chain_send(
    chain_id: String,
    to: String,
    amount_raw: String,
) -> Result<String, JsError> {
    let mgr = wallet()?;
    let mnemonic = mgr.get_mnemonic().await.map_err(jserr)?;
    let seed = Bip39Generator::to_seed(&mnemonic, "").map_err(jserr)?;
    dispatch_send(&chain_id, &seed, &to, &amount_raw).await
}

/// Fetch the native balance for `address` on `chain_id`.
///
/// Returns a JS object `{ raw, display, symbol, decimals }`:
///
/// - `raw` — decimal string of the smallest unit (no precision loss)
/// - `display` — human-readable decimal with trailing zeros trimmed
/// - `symbol` — chain native asset symbol (BTC, ETH, SOL, …)
/// - `decimals` — base-10 decimals used by this chain
///
/// Does not require the session to be unlocked — balance is public
/// data, queried directly from the chain's public RPC. Pass any
/// address (the caller's own, a recipient's, an arbitrary watch
/// address).
///
/// # Errors
/// Returns a JS error if `chain_id` is unknown or the RPC call fails
/// (e.g. CORS blocked, public RPC down).
#[wasm_bindgen]
pub async fn chain_balance(chain_id: String, address: String) -> Result<JsValue, JsError> {
    let amount = dispatch_balance(&chain_id, &address).await?;
    let symbol = native_symbol(&chain_id).to_string();
    let decimals = u32::from(amount.decimals());
    let out = ChainBalanceJs {
        raw: amount.raw().to_string(),
        display: amount.to_display_string(),
        symbol,
        decimals,
    };
    serde_wasm_bindgen::to_value(&out).map_err(|e| JsError::new(&e.to_string()))
}

/// JS-side balance shape returned by [`chain_balance`].
#[derive(Serialize)]
struct ChainBalanceJs {
    raw: String,
    display: String,
    symbol: String,
    decimals: u32,
}

/// Per-chain send dispatch — calls each chain crate's `wasm::send`.
async fn dispatch_send(
    chain_id: &str,
    seed: &dontyeet_primitives::secret::Seed,
    to: &str,
    amount_raw: &str,
) -> Result<String, JsError> {
    // Some chain crates use `bnb` for what Yeet UI calls `bsc`. Map at
    // the boundary so the rest of the codebase can stay on the
    // user-facing label.
    let mapped = map_chain_id(chain_id);
    match mapped {
        "ethereum" | "polygon" | "bnb" | "avalanche" | "sonic" => {
            dontyeet_chain_evm::wasm::send(mapped, seed, to, amount_raw)
                .await
                .map_err(jserr)
        }
        "bitcoin" => dontyeet_chain_bitcoin::wasm::send(mapped, seed, to, amount_raw)
            .await
            .map_err(jserr),
        "solana" => dontyeet_chain_solana::wasm::send(mapped, seed, to, amount_raw)
            .await
            .map_err(jserr),
        "cardano" => dontyeet_chain_cardano::wasm::send(mapped, seed, to, amount_raw)
            .await
            .map_err(jserr),
        "xrp" => dontyeet_chain_xrp::wasm::send(mapped, seed, to, amount_raw)
            .await
            .map_err(jserr),
        "algorand" => dontyeet_chain_algorand::wasm::send(mapped, seed, to, amount_raw)
            .await
            .map_err(jserr),
        "tron" => dontyeet_chain_tron::wasm::send(mapped, seed, to, amount_raw)
            .await
            .map_err(jserr),
        "kaspa" => dontyeet_chain_kaspa::wasm::send(mapped, seed, to, amount_raw)
            .await
            .map_err(jserr),
        "kadena" => dontyeet_chain_kadena::wasm::send(mapped, seed, to, amount_raw)
            .await
            .map_err(jserr),
        other => Err(JsError::new(&format!("unsupported chain: {other}"))),
    }
}

/// Per-chain balance dispatch — calls each chain crate's
/// `wasm::fetch_balance` and returns the typed `Amount`.
async fn dispatch_balance(
    chain_id: &str,
    address: &str,
) -> Result<dontyeet_primitives::Amount, JsError> {
    let mapped = map_chain_id(chain_id);
    match mapped {
        "ethereum" | "polygon" | "bnb" | "avalanche" | "sonic" => {
            dontyeet_chain_evm::wasm::fetch_balance(mapped, address)
                .await
                .map_err(jserr)
        }
        "bitcoin" => dontyeet_chain_bitcoin::wasm::fetch_balance(mapped, address)
            .await
            .map_err(jserr),
        "solana" => dontyeet_chain_solana::wasm::fetch_balance(mapped, address)
            .await
            .map_err(jserr),
        "cardano" => dontyeet_chain_cardano::wasm::fetch_balance(mapped, address)
            .await
            .map_err(jserr),
        "xrp" => dontyeet_chain_xrp::wasm::fetch_balance(mapped, address)
            .await
            .map_err(jserr),
        "algorand" => dontyeet_chain_algorand::wasm::fetch_balance(mapped, address)
            .await
            .map_err(jserr),
        "tron" => dontyeet_chain_tron::wasm::fetch_balance(mapped, address)
            .await
            .map_err(jserr),
        "kaspa" => dontyeet_chain_kaspa::wasm::fetch_balance(mapped, address)
            .await
            .map_err(jserr),
        "kadena" => dontyeet_chain_kadena::wasm::fetch_balance(mapped, address)
            .await
            .map_err(jserr),
        other => Err(JsError::new(&format!("unsupported chain: {other}"))),
    }
}

/// Map the user-facing chain id to the canonical one each chain
/// crate's wasm module expects. The frontend UI uses `bsc` everywhere
/// for the YEET-host chain; the EVM crate names it `bnb` internally
/// (BNB Smart Chain). All other chains use the same id end-to-end.
fn map_chain_id(chain_id: &str) -> &str {
    match chain_id {
        "bsc" => "bnb",
        other => other,
    }
}

/// Display symbol for the chain's native asset.
fn native_symbol(chain_id: &str) -> &'static str {
    match map_chain_id(chain_id) {
        "ethereum" => "ETH",
        "polygon" => "MATIC",
        "bnb" => "BNB",
        "avalanche" => "AVAX",
        "sonic" => "S",
        "bitcoin" => "BTC",
        "solana" => "SOL",
        "cardano" => "ADA",
        "xrp" => "XRP",
        "algorand" => "ALGO",
        "tron" => "TRX",
        "kaspa" => "KAS",
        "kadena" => "KDA",
        _ => "",
    }
}

/// Pure-crypto core for [`get_chain_address`]. Split out so the
/// multichain switch lives in one obvious place — adding a new chain
/// is one match arm here plus one row in [`SUPPORTED_CHAINS`].
fn derive_chain_address(seed: &Seed, chain_id: &str) -> Result<String, JsError> {
    let addr = match chain_id {
        // All EVM-compatible chains share the Ethereum BIP-44 path,
        // matching every standard EVM wallet (MetaMask, Trust, etc.).
        "ethereum" | "bsc" | "polygon" | "avalanche" | "sonic" => {
            derive_address(seed, bip_paths::ETHEREUM).map_err(jserr)?
        }
        "bitcoin" => {
            dontyeet_chain_bitcoin::derive_address(seed, bip_paths::BITCOIN_SEGWIT, BITCOIN_BECH32_HRP)
                .map_err(jserr)?
        }
        "solana" => dontyeet_chain_solana::derive_address(seed, bip_paths::SOLANA).map_err(jserr)?,
        "cardano" => {
            dontyeet_chain_cardano::derive_address(seed, bip_paths::CARDANO).map_err(jserr)?
        }
        "xrp" => dontyeet_chain_xrp::derive_address(seed, bip_paths::XRP).map_err(jserr)?,
        "algorand" => {
            dontyeet_chain_algorand::derive_address(seed, bip_paths::ALGORAND).map_err(jserr)?
        }
        "tron" => dontyeet_chain_tron::derive_address(seed, bip_paths::TRON).map_err(jserr)?,
        "kaspa" => dontyeet_chain_kaspa::derive_address(seed, bip_paths::KASPA).map_err(jserr)?,
        "kadena" => dontyeet_chain_kadena::derive_address(seed, bip_paths::KADENA).map_err(jserr)?,
        other => {
            return Err(JsError::new(&format!("unsupported chain: {other}")));
        }
    };
    Ok(addr.as_str().to_string())
}

/// Generate a fresh paper-wallet mnemonic and address pair.
///
/// Used by the paper-cheque flow: returns a brand-new wallet that is
/// **not** persisted to localStorage. The caller prints the mnemonic +
/// address as a physical cheque; the funded YEET is sent to the address
/// by the issuer (using their main wallet, signed via
/// [`sign_evm_call`]).
///
/// Returns a JS object `{ address, mnemonic }`. The mnemonic is the
/// bearer's only key — losing the paper loses the funds.
///
/// # Errors
/// Returns a JS error if entropy generation or address derivation fails.
#[wasm_bindgen]
pub fn generate_paper_wallet() -> Result<JsValue, JsError> {
    let mnemonic = Bip39Generator::generate(WordCount::Twelve).map_err(jserr)?;
    let address = derive_address_for(&mnemonic)?;
    let out = CreatedWallet {
        address,
        mnemonic: mnemonic.as_str().to_string(),
    };
    serde_wasm_bindgen::to_value(&out).map_err(|e| JsError::new(&e.to_string()))
}

// ---------------------------------------------------------------------------
// Unlock-factor exports
// ---------------------------------------------------------------------------
//
// The wallet's mnemonic is encrypted once under a primary password (the
// thing [`create_wallet`] / [`login`] use). Additional unlock paths —
// biometric, MetaMask signature, hardware passkey — are implemented as
// "factors": small encrypted-password blobs stored under
// `dontyeet:factor:<label>`. Calling [`unlock_with_factor`] decrypts the
// blob with the externally-supplied key, recovers the primary password,
// and feeds it to [`login`] internally.
//
// The shim is factor-agnostic on purpose. WebAuthn ceremonies and
// MetaMask `personal_sign` calls live in JS where the relevant browser
// APIs already are; JS hashes the result to 32 bytes and hands it in.
// That keeps the Rust API surface small and lets new factor sources be
// added without touching this crate.

/// Enable an alternative unlock factor for the wallet.
///
/// Encrypts the current primary `password` under the externally-supplied
/// `factor_key_hex` and stores the ciphertext under `label`. Subsequent
/// calls to [`unlock_with_factor`] with the same `label` and
/// `factor_key_hex` unlock the wallet without the caller re-typing the
/// password.
///
/// `factor_key_hex` is a 32-byte (64-hex-char) AES-256 key the caller
/// derived externally — e.g. SHA-256 of a MetaMask `personal_sign`
/// signature, or the 32-byte WebAuthn PRF output. `label` is a free-form
/// identifier ("biometric", "metamask", "passkey"); re-enrolling the
/// same label overwrites the previous blob.
///
/// `password` is verified against the wallet's stored hash before
/// storage so a wrong password cannot silently poison the factor.
///
/// # Errors
/// Returns a JS error if no wallet exists, the password is wrong,
/// `factor_key_hex` is not exactly 32 bytes of hex, or storage fails.
#[wasm_bindgen]
pub async fn enable_factor(
    label: String,
    factor_key_hex: String,
    password: String,
) -> Result<(), JsError> {
    let mgr = wallet()?;
    mgr.verify_password(&password).await.map_err(jserr)?;
    let factor_key = decode_factor_key(&factor_key_hex)?;
    let store = EncryptedStore::new(BrowserStorage::new(), AesGcmCipher);
    store
        .set(&factor_storage_key(&label), &password, &factor_key)
        .await
        .map_err(jserr)
}

/// Unlock the wallet using a previously enrolled factor.
///
/// Decrypts the stored password blob using `factor_key_hex`, then feeds
/// the recovered password to the regular unlock path. Returns the
/// wallet's BSC address on success.
///
/// # Errors
/// Returns a JS error if `label` was never enrolled, `factor_key_hex`
/// is the wrong length or fails to decrypt the stored blob, the
/// recovered password is rejected by the wallet (e.g. primary password
/// was changed without re-enrolling the factor), or storage fails.
#[wasm_bindgen]
pub async fn unlock_with_factor(
    label: String,
    factor_key_hex: String,
) -> Result<String, JsError> {
    let mgr = wallet()?;
    let factor_key = decode_factor_key(&factor_key_hex)?;
    let store = EncryptedStore::new(BrowserStorage::new(), AesGcmCipher);
    let stored: Option<String> = store
        .get(&factor_storage_key(&label), &factor_key)
        .await
        .map_err(jserr)?;
    let password =
        stored.ok_or_else(|| JsError::new(&format!("factor '{label}' is not enrolled")))?;
    mgr.login(&password).await.map_err(jserr)?;
    let m = mgr.get_mnemonic().await.map_err(jserr)?;
    derive_address_for(&m)
}

/// Forget an enrolled factor. No-op if `label` was never enrolled.
///
/// Does **not** require the session to be unlocked — a user who has
/// forgotten their primary password can still revoke a leaked factor
/// (e.g. a stolen device's biometric enrollment) by clearing the blob.
///
/// # Errors
/// Returns a JS error if storage cannot be written.
#[wasm_bindgen]
pub async fn disable_factor(label: String) -> Result<(), JsError> {
    let store = EncryptedStore::new(BrowserStorage::new(), AesGcmCipher);
    store
        .delete(&factor_storage_key(&label))
        .await
        .map_err(jserr)
}

/// List the labels of currently enrolled factors as a JS string array.
///
/// Drives the "Connected sign-in methods" view in settings. Returns
/// an empty array when no factor is enrolled.
///
/// # Errors
/// Returns a JS error if storage cannot be listed.
#[wasm_bindgen]
pub async fn list_factors() -> Result<JsValue, JsError> {
    let backend = BrowserStorage::new();
    let keys = backend.list_keys().await.map_err(jserr)?;
    let factors: Vec<String> = keys
        .into_iter()
        .filter_map(|k| k.strip_prefix(FACTOR_KEY_PREFIX).map(str::to_string))
        .collect();
    serde_wasm_bindgen::to_value(&factors).map_err(|e| JsError::new(&e.to_string()))
}

// ---------------------------------------------------------------------------
// Internal types and helpers
// ---------------------------------------------------------------------------

/// Wallet-creation result returned to JS.
///
/// Private — only constructed by [`create_wallet`] / [`generate_paper_wallet`]
/// and immediately serialized into `JsValue` via `serde_wasm_bindgen`. It
/// is never named in any public Rust signature, which keeps it outside
/// the M-PUBLIC-DEBUG / sensitive-data guidance (the mnemonic field is
/// inherently sensitive but lives in this struct for less than one
/// function call).
#[derive(Serialize)]
struct CreatedWallet {
    address: String,
    mnemonic: String,
}

/// Derive the EIP-55 address for `mnemonic` at the standard EVM path.
///
/// # Errors
/// Returns a JS error if seed derivation or address derivation fails.
fn derive_address_for(mnemonic: &Mnemonic) -> Result<String, JsError> {
    let seed = Bip39Generator::to_seed(mnemonic, "").map_err(jserr)?;
    let addr = derive_address(&seed, BIP44_EVM_PATH).map_err(jserr)?;
    Ok(addr.as_str().to_string())
}

/// Decode an `0x`-prefixed 20-byte EVM address into raw bytes.
///
/// # Errors
/// Returns a JS error if the input is not valid hex or is the wrong length.
fn parse_evm_address(s: &str) -> Result<Vec<u8>, JsError> {
    let bytes = parse_hex(s)?;
    if bytes.len() == 20 {
        Ok(bytes)
    } else {
        Err(JsError::new(&format!(
            "expected 20-byte address, got {} bytes",
            bytes.len()
        )))
    }
}

/// Decode an optionally `0x`-prefixed hex string into raw bytes.
fn parse_hex(s: &str) -> Result<Vec<u8>, JsError> {
    let h = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    hex::decode(h).map_err(|e| JsError::new(&format!("invalid hex: {e}")))
}

/// Compose the logical storage key for a factor blob.
fn factor_storage_key(label: &str) -> String {
    format!("{FACTOR_KEY_PREFIX}{label}")
}

/// Decode a hex-encoded factor key, enforcing the [`FACTOR_KEY_LEN`] length.
///
/// # Errors
/// Returns a JS error if the input is not valid hex or is not exactly
/// [`FACTOR_KEY_LEN`] bytes long.
fn decode_factor_key(hex_str: &str) -> Result<Vec<u8>, JsError> {
    let bytes = parse_hex(hex_str)?;
    if bytes.len() == FACTOR_KEY_LEN {
        Ok(bytes)
    } else {
        Err(JsError::new(&format!(
            "factor key must be {FACTOR_KEY_LEN} bytes, got {}",
            bytes.len()
        )))
    }
}

/// Convert any `Display` error into a `JsError` for the wasm-bindgen surface.
fn jserr<E: std::fmt::Display>(e: E) -> JsError {
    JsError::new(&e.to_string())
}

// Rust guideline compliant 2026-02-21
