# dontyeet-chain-kaspa

**Layer:** Integration | **Deps:** primitives, crypto, chain, network

## Purpose

Kaspa `BlockDAG` chain plugin (GHOSTDAG protocol). Implements `ChainPlugin` for mainnet and testnet.

## Chain Details

- **Protocol:** `BlockDAG` (GHOSTDAG) — 1-second blocks
- **Signing:** secp256k1 (ECDSA, Schnorr planned)
- **Address format:** `kaspa:` + hex-encoded 20-byte SHA-256 pubkey hash
- **Native token:** KAS (8 decimals, 1 KAS = 100,000,000 SOMPI)
- **BIP-44 coin type:** 111111
- **Derivation path:** `m/44'/111111'/0'/0/0`
- **Fee model:** Mass-based (SOMPI per gram)
- **API:** REST at `https://api.kaspa.org` (mainnet), `https://api-tn.kaspa.org` (testnet)
- **Explorer:** `https://explorer.kaspa.org`

## Public API

### Re-exports (from `lib.rs`)

```rust
pub use config::KaspaConfig;
pub use error::{KaspaError, KaspaResult};
pub use fees::KaspaFees;
pub use keys::{KaspaAddressEncoder, KaspaKeyDeriver};
pub use plugin::{KaspaChainPlugin, kaspa_plugin};
```

`KaspaFeeEstimator` exists in `src/fees.rs` but is not re-exported at
the crate root — it's reached through `KaspaChainPlugin`'s typed
getter. Same for the signer: secp256k1 signing lives in
`src/signing.rs` and is wired into `KaspaChainPlugin` directly; there
is no separate `KaspaTransactionSigner` type to re-export.

### Factory

```rust
/// Create a KaspaChainPlugin for mainnet + testnet.
pub fn kaspa_plugin() -> KaspaChainPlugin;
```

### Key Types

| Type | Trait | Notes |
|------|-------|-------|
| `KaspaChainPlugin` | `ChainPlugin` | Main plugin struct |
| `KaspaConfig` | — | Chain configuration |
| `KaspaKeyDeriver` | `KeyDeriver` | BIP-44 `m/44'/111111'/0'/0/0` |
| `KaspaAddressEncoder` | `AddressEncoder` | `kaspa:` / `kaspatest:` prefix + hex hash |
| `KaspaFees` | — | `sompi_per_gram` + `mass` |
| `KaspaError` | `Error` | 5 variants: `KeyDerivation`, `InvalidAddress`, `TransactionBuild`, `Signing`, `Network` |

The plugin's typed components (`FeeEstimator<KaspaFees>`,
`BalanceFetcher`, `TransactionBroadcaster`, `TransactionSigner`,
`NetworkProvider` + `RpcEndpointProvider`) are concrete types in
`src/{fees,balance,broadcast,signing}.rs` but not individually
re-exported. Reach them through `KaspaChainPlugin`'s typed getters
(`fee_estimator`, `balance_fetcher`, `broadcaster`, `signer`,
`network_provider`).

### Networks

| `NetworkId` | Label | Category |
|-------------|-------|----------|
| `kaspa-mainnet` | Kaspa Mainnet | Mainnet |
| `kaspa-testnet` | Kaspa Testnet | Testnet |

## Module Map

```
src/
├── error.rs      — KaspaError enum (thiserror)
├── config.rs     — KaspaConfig struct
├── keys.rs       — KaspaKeyDeriver + KaspaAddressEncoder
├── fees.rs       — KaspaFees + KaspaFeeEstimator (not re-exported)
├── balance.rs    — KaspaBalanceFetcher (REST API)
├── broadcast.rs  — KaspaBroadcaster (REST API)
├── transfer.rs   — UTXO-walk + raw-tx assembly (rpc-only)
├── signing.rs    — secp256k1 ECDSA TransactionSigner (no separate tx.rs)
├── history.rs    — `api.kaspa.org` full-transactions adapter (rpc-only)
├── rest.rs       — Internal REST helpers (rest_get, rest_post)
├── plugin.rs     — KaspaChainPlugin + kaspa_plugin() factory.
                    NetworkProvider impl is built inline here, not in a
                    separate network.rs
├── wasm.rs       — wasm32 entry points
└── lib.rs        — Module declarations + re-exports
```
