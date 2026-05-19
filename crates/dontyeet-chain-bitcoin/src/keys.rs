//! Bitcoin-style key derivation and address encoding.
//!
//! Implements [`KeyDeriver`] and [`AddressEncoder`] for any UTXO chain
//! that uses BIP-84 segwit + `Base58Check` legacy (Bitcoin, Litecoin, and
//! Bitcoin testnet variants today). The per-network bech32 HRP and
//! `Base58Check` version bytes come from the [`AddressFormat`] map carried
//! on [`BtcConfig`], which is what makes the same code path serve every
//! configured chain without recompiling.

use std::collections::HashMap;
use std::sync::Arc;

use k256::ecdsa::SigningKey;
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};

use dontyeet_crypto::Bip44Deriver;
use dontyeet_primitives::address::Address;
use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::secret::{KeyPair, PrivateKey, Seed};
use dontyeet_primitives::traits::{AddressEncoder, KeyDeriver};

use crate::config::{AddressFormat, BtcConfig};

/// Derive the P2WPKH segwit address for the given seed, BIP path, and
/// bech32 HRP.
///
/// Pure-crypto, no I/O — drop-in replacement for the [`BtcKeyDeriver`]
/// machinery when the caller doesn't care about the full
/// [`BtcConfig`] (e.g. the WASM frontend that just needs to display a
/// receive address for one chain on its mainnet).
///
/// HRP examples: `"bc"` for Bitcoin mainnet, `"tb"` for testnet,
/// `"ltc"` for Litecoin mainnet.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or secp256k1
/// key construction fails, or `DontYeetWalletError::Chain` if the bech32
/// HRP is malformed.
pub fn derive_address(seed: &Seed, derivation_path: &str, bech32_hrp: &str) -> Result<Address> {
    let private_key = Bip44Deriver::derive(seed, derivation_path)?;
    let compressed_pub = compressed_pubkey(&private_key)?;
    let format = AddressFormat {
        bech32_hrp: bech32_hrp.to_string(),
        // Legacy version bytes are unused by P2WPKH derivation; only
        // bech32 emission is exercised. Filling with zero is fine.
        base58_p2pkh_version: 0,
        base58_p2sh_version: 0,
    };
    encode_p2wpkh_address(&format, &compressed_pub)
}

// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------

/// Derives keypairs for a Bitcoin-style chain using BIP-84 segwit by
/// default (or whatever path is configured).
pub struct BtcKeyDeriver {
    derivation_path: String,
    formats: Arc<HashMap<NetworkId, AddressFormat>>,
}

impl BtcKeyDeriver {
    /// Create a new key deriver from the config's derivation path and
    /// per-network address-format map.
    #[must_use]
    pub fn new(config: &BtcConfig) -> Self {
        Self {
            derivation_path: config.derivation_path.clone(),
            formats: Arc::new(config.address_formats.clone()),
        }
    }
}

impl KeyDeriver for BtcKeyDeriver {
    /// Derive a [`KeyPair`] from the master seed.
    ///
    /// Uses BIP-84 for segwit P2WPKH addresses by default; the bech32 HRP
    /// is taken from the configured `AddressFormat` for the requested
    /// network.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Crypto` if BIP-44 derivation or secp256k1
    /// key construction fails, or `DontYeetWalletError::Chain` if no
    /// address-format entry exists for the network.
    fn derive_keypair(&self, seed: &Seed, network: &NetworkId) -> Result<KeyPair> {
        let private_key = Bip44Deriver::derive(seed, &self.derivation_path)?;
        let compressed_pub = compressed_pubkey(&private_key)?;
        let format = format_for(&self.formats, network)?;
        let address = encode_p2wpkh_address(format, &compressed_pub)?;
        Ok(KeyPair::new(address, private_key))
    }
}

// ---------------------------------------------------------------------------
// Address encoding
// ---------------------------------------------------------------------------

/// Encodes UTXO-chain addresses in P2WPKH (segwit) or validates
/// P2PKH/P2SH (legacy `Base58Check`) per the configured network format.
///
/// New addresses are always emitted as P2WPKH bech32 — that's what the
/// wallet derives from its seed. Validation accepts both segwit and
/// legacy formats so users can paste recipient addresses from any
/// wallet.
#[derive(Clone)]
pub struct BtcAddressEncoder {
    formats: Arc<HashMap<NetworkId, AddressFormat>>,
}

impl BtcAddressEncoder {
    /// Build an encoder bound to the per-network address formats in
    /// `config`.
    #[must_use]
    pub fn new(config: &BtcConfig) -> Self {
        Self {
            formats: Arc::new(config.address_formats.clone()),
        }
    }
}

impl AddressEncoder for BtcAddressEncoder {
    /// Encode a 33-byte compressed public key into a P2WPKH bech32 address.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Chain` if the public key length is invalid,
    /// no address-format entry exists for the network, or bech32
    /// encoding fails.
    fn encode(&self, public_key: &[u8], network: &NetworkId) -> Result<Address> {
        let format = format_for(&self.formats, network)?;
        encode_p2wpkh_address(format, public_key)
    }

    /// Validate an address string against this network's format.
    ///
    /// Bech32 segwit addresses must use the network's configured HRP;
    /// legacy `Base58Check` addresses must carry the network's P2PKH or
    /// P2SH version byte. Cross-network addresses (e.g. a Bitcoin
    /// mainnet address pasted on a testnet) are rejected.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Validation` if the address is malformed,
    /// or `DontYeetWalletError::Chain` if no address-format entry exists for
    /// the network.
    fn validate(&self, address: &str, network: &NetworkId) -> Result<()> {
        let format = format_for(&self.formats, network)?;
        let segwit_prefix = format!("{}1", format.bech32_hrp);
        if address.starts_with(&segwit_prefix) {
            validate_bech32(address, &format.bech32_hrp)
        } else {
            // Legacy `Base58Check`: prefix character is determined by the
            // version byte, so we can't pre-filter on first-letter and
            // must let the decoder do the work.
            validate_base58check(
                address,
                format.base58_p2pkh_version,
                format.base58_p2sh_version,
            )
        }
    }
}

/// Look up the per-network format, surfacing a clear error for a missing
/// entry rather than panicking.
fn format_for<'a>(
    formats: &'a HashMap<NetworkId, AddressFormat>,
    network: &NetworkId,
) -> Result<&'a AddressFormat> {
    formats.get(network).ok_or_else(|| {
        DontYeetWalletError::Chain(format!(
            "no address format configured for network {network}"
        ))
    })
}

/// Encode a 33-byte compressed pubkey as a P2WPKH bech32 address using
/// the given format's HRP.
fn encode_p2wpkh_address(format: &AddressFormat, public_key: &[u8]) -> Result<Address> {
    if public_key.len() != 33 {
        return Err(DontYeetWalletError::Chain(format!(
            "expected 33-byte compressed pubkey, got {}",
            public_key.len()
        )));
    }
    let hash = hash160(public_key);
    let addr = encode_bech32_p2wpkh(&format.bech32_hrp, &hash)?;
    Ok(Address::new(addr))
}

// ---------------------------------------------------------------------------
// Hash helpers
// ---------------------------------------------------------------------------

/// HASH160: SHA-256 then RIPEMD-160 (standard Bitcoin hash).
pub(crate) fn hash160(data: &[u8]) -> [u8; 20] {
    let sha = Sha256::digest(data);
    let ripemd = Ripemd160::digest(sha);
    let mut out = [0u8; 20];
    out.copy_from_slice(&ripemd);
    out
}

/// Derive the 33-byte compressed public key from a private key.
///
/// # Errors
/// Returns `DontYeetWalletError::Crypto` if the key bytes are invalid.
pub(crate) fn compressed_pubkey(private_key: &PrivateKey) -> Result<Vec<u8>> {
    let signing_key = SigningKey::from_slice(private_key.as_bytes())
        .map_err(|e| DontYeetWalletError::Crypto(format!("invalid secp256k1 key: {e}")))?;
    let verifying_key = signing_key.verifying_key();
    let point = verifying_key.to_encoded_point(true);
    Ok(point.as_bytes().to_vec())
}

// ---------------------------------------------------------------------------
// Bech32 (P2WPKH) encoding + validation
// ---------------------------------------------------------------------------

/// Encode a 20-byte witness program as a bech32 P2WPKH address.
fn encode_bech32_p2wpkh(hrp: &str, witness_program: &[u8; 20]) -> Result<String> {
    let hrp_parsed = bech32::Hrp::parse(hrp)
        .map_err(|e| DontYeetWalletError::Chain(format!("invalid bech32 HRP: {e}")))?;

    // Encode as segwit v0 P2WPKH using the bech32 segwit module.
    bech32::segwit::encode_v0(hrp_parsed, witness_program)
        .map_err(|e| DontYeetWalletError::Chain(format!("bech32 encode error: {e}")))
}

/// Validate a bech32/bech32m segwit address against an expected HRP.
fn validate_bech32(address: &str, expected_hrp_str: &str) -> Result<()> {
    let expected_hrp = bech32::Hrp::parse(expected_hrp_str)
        .map_err(|e| DontYeetWalletError::Validation(format!("HRP parse error: {e}")))?;

    let (hrp, _version, _program) = bech32::segwit::decode(address)
        .map_err(|e| DontYeetWalletError::Validation(format!("invalid segwit address: {e}")))?;

    if hrp != expected_hrp {
        return Err(DontYeetWalletError::Validation(format!(
            "bech32 HRP mismatch: expected '{expected_hrp_str}', got '{hrp}'"
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `Base58Check` (P2PKH) validation
// ---------------------------------------------------------------------------

/// Validate a ``Base58Check`` legacy address against the expected P2PKH /
/// P2SH version bytes for the network.
fn validate_base58check(address: &str, expected_p2pkh: u8, expected_p2sh: u8) -> Result<()> {
    let decoded = bs58::decode(address)
        .into_vec()
        .map_err(|e| DontYeetWalletError::Validation(format!("base58 decode error: {e}")))?;

    if decoded.len() != 25 {
        return Err(DontYeetWalletError::Validation(format!(
            "base58check address must be 25 bytes, got {}",
            decoded.len()
        )));
    }

    // Verify checksum: SHA256(SHA256(payload)) first 4 bytes.
    let payload = &decoded[..21];
    let checksum = &decoded[21..25];
    let hash1 = Sha256::digest(payload);
    let hash2 = Sha256::digest(hash1);
    if checksum != &hash2[..4] {
        return Err(DontYeetWalletError::Validation(
            "base58check checksum mismatch".into(),
        ));
    }

    // Validate version byte against this network's allowed P2PKH or P2SH
    // values. An address using a different chain's version byte is
    // considered invalid for this network.
    let version = decoded[0];
    if version != expected_p2pkh && version != expected_p2sh {
        return Err(DontYeetWalletError::Validation(format!(
            "address version byte 0x{version:02x} does not match this network \
             (expected 0x{expected_p2pkh:02x} P2PKH or 0x{expected_p2sh:02x} P2SH)"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_btc_config;
    use dontyeet_primitives::chain::{ChainId, NetworkCategory};
    use dontyeet_primitives::network::BlockchainNetwork;

    /// Build an encoder around the default Bitcoin config (the byte-
    /// identical pre-refactor setup).
    fn default_encoder() -> BtcAddressEncoder {
        BtcAddressEncoder::new(&default_btc_config())
    }

    /// Build an encoder around a Litecoin-shaped config so the same code
    /// paths exercise a non-Bitcoin chain.
    fn litecoin_encoder() -> BtcAddressEncoder {
        let mainnet_id = NetworkId::new("litecoin-mainnet");
        let mut formats = HashMap::new();
        formats.insert(
            mainnet_id.clone(),
            AddressFormat {
                bech32_hrp: "ltc".into(),
                base58_p2pkh_version: 0x30,
                base58_p2sh_version: 0x32,
            },
        );
        let config = BtcConfig {
            chain_id: ChainId::Other("litecoin".into()),
            native_asset: dontyeet_primitives::asset::AssetInfo {
                name: "Litecoin".into(),
                symbol: "LTC".into(),
                kind: dontyeet_primitives::asset::AssetKind::Coin,
                chain_id: ChainId::Other("litecoin".into()),
                decimals: 8,
            },
            derivation_path: "m/84'/2'/0'/0/0".into(),
            networks: vec![BlockchainNetwork {
                id: mainnet_id,
                label: "Litecoin Mainnet".into(),
                chain_id: ChainId::Other("litecoin".into()),
                category: NetworkCategory::Mainnet,
                evm_chain_id: None,
            }],
            api_urls: HashMap::new(),
            explorer_urls: HashMap::new(),
            address_formats: formats,
        };
        BtcAddressEncoder::new(&config)
    }

    #[test]
    fn hash160_known_vector() {
        // SHA-256 then RIPEMD-160 of an empty byte slice.
        let result = hash160(&[]);
        let hex_str = hex::encode(result);
        // Known: RIPEMD160(SHA256("")) = b472a266d0bd89c13706a4132ccfb16f7c3b9fcb
        assert_eq!(hex_str, "b472a266d0bd89c13706a4132ccfb16f7c3b9fcb");
    }

    #[test]
    fn compressed_pubkey_is_33_bytes() {
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let compressed = compressed_pubkey(&pk).expect("derive pubkey");
        assert_eq!(compressed.len(), 33);
    }

    #[test]
    fn p2wpkh_from_known_pubkey() {
        // Private key = 1 => known compressed public key.
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let pub_key = compressed_pubkey(&pk).expect("pubkey");

        let encoder = default_encoder();
        let network = NetworkId::new("bitcoin-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");

        // Should be a valid bech32 mainnet address.
        assert!(addr.as_str().starts_with("bc1"));
        encoder.validate(addr.as_str(), &network).expect("valid");
    }

    #[test]
    fn validate_valid_mainnet_legacy() {
        let encoder = default_encoder();
        let network = NetworkId::new("bitcoin-mainnet");
        // Satoshi's genesis coinbase address.
        assert!(
            encoder
                .validate("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", &network)
                .is_ok()
        );
    }

    #[test]
    fn validate_invalid_address_fails() {
        let encoder = default_encoder();
        let network = NetworkId::new("bitcoin-mainnet");
        assert!(encoder.validate("not_an_address", &network).is_err());
    }

    #[test]
    fn validate_bad_checksum_legacy_fails() {
        let encoder = default_encoder();
        let network = NetworkId::new("bitcoin-mainnet");
        // Flipped last character.
        assert!(
            encoder
                .validate("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb", &network)
                .is_err()
        );
    }

    #[test]
    fn key_derivation_produces_32_bytes() {
        use dontyeet_crypto::mnemonic::{Bip39Generator, WordCount};

        let m = Bip39Generator::generate(WordCount::Twelve).expect("mnemonic");
        let seed = Bip39Generator::to_seed(&m, "").expect("seed");
        let pk = Bip44Deriver::derive(&seed, dontyeet_crypto::derivation::paths::BITCOIN_SEGWIT)
            .expect("derive");
        assert_eq!(pk.as_bytes().len(), 32);
    }

    // -- Refactor regression tests ---------------------------------------

    #[test]
    fn missing_format_for_network_is_a_clear_error() {
        let encoder = default_encoder();
        let unknown = NetworkId::new("not-a-real-network");
        let err = encoder
            .validate("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", &unknown)
            .expect_err("must reject unknown network");
        assert!(
            err.to_string().contains("no address format configured"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn bitcoin_mainnet_address_rejected_on_testnet() {
        // Cross-network paste check: a `bc1...` mainnet address should
        // not validate against bitcoin-testnet4 (different HRP).
        let encoder = default_encoder();
        let testnet = NetworkId::new("bitcoin-testnet4");
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let pub_key = compressed_pubkey(&pk).expect("pubkey");
        let mainnet_addr = encoder
            .encode(&pub_key, &NetworkId::new("bitcoin-mainnet"))
            .expect("encode");
        assert!(encoder.validate(mainnet_addr.as_str(), &testnet).is_err());
    }

    // -- Litecoin-shaped tests (different HRP + version bytes) -----------

    #[test]
    fn litecoin_p2wpkh_uses_ltc_hrp() {
        // Same private key, different chain → different HRP. This is
        // the load-bearing test for the refactor: if the encoder were
        // still hardcoded to `bc`/`tb`, this would fail.
        let key_bytes =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        let pk = PrivateKey::new(key_bytes);
        let pub_key = compressed_pubkey(&pk).expect("pubkey");

        let encoder = litecoin_encoder();
        let network = NetworkId::new("litecoin-mainnet");
        let addr = encoder.encode(&pub_key, &network).expect("encode");

        assert!(
            addr.as_str().starts_with("ltc1"),
            "expected ltc1... address, got {}",
            addr.as_str()
        );
        encoder.validate(addr.as_str(), &network).expect("valid");
    }

    #[test]
    fn litecoin_validates_known_legacy_address() {
        // Litecoin mainnet P2PKH addresses start with `L` (version
        // 0x30). Use a known on-chain example.
        let encoder = litecoin_encoder();
        let network = NetworkId::new("litecoin-mainnet");
        // Litecoin Foundation donation address (well-known historical).
        assert!(
            encoder
                .validate("LTpYZG19YmfvY2bBDYtCKpunVRw7nVgRHW", &network)
                .is_ok()
        );
    }

    #[test]
    fn litecoin_rejects_bitcoin_mainnet_legacy_address() {
        // A `1...` Bitcoin address has version byte 0x00, which Litecoin
        // does not accept as either P2PKH (0x30) or P2SH (0x32).
        let encoder = litecoin_encoder();
        let network = NetworkId::new("litecoin-mainnet");
        assert!(
            encoder
                .validate("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", &network)
                .is_err()
        );
    }
}

// Rust guideline compliant 2026-05-02
