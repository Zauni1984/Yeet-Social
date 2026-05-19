# dontyeet-chain-kadena

**Layer:** Integration | **Deps:** primitives, crypto, chain, network

## Purpose

Kadena Chainweb chain plugin (community edition, forked Nov 2025).
Implements `ChainPlugin` for the 20-parallel-PoW-chains design. The
crate is split between always-on pure-crypto modules (compile under
`wasm32`) and `rpc`-feature-gated modules that use the Pact API.

## Chain Details

- **Protocol:** Chainweb PoW (20 parallel chains, 30s blocks)
- **Signing:** ed25519
- **Address format:** `k:` + hex-encoded ed25519 public key (32 bytes / 64 hex)
- **Native token:** KDA (12 decimals)
- **BIP-44 coin type:** 626
- **Derivation path:** `m/44'/626'/0'/0/0`
- **Tx format:** Pact command (JSON), Blake2b-256 hashed, URL-safe base64-encoded
- **API:** Chainweb Pact API at `https://api.chainweb.com` (mainnet);
  fallback `https://chainweb-data.com` for cross-chain history aggregation
- **Explorer:** `https://explorer.chainweb.com`

## Public API

### Always-on re-exports (also available under `wasm32`)

```rust
pub use config::KadenaConfig;
pub use error::{KadenaError, KadenaResult};
pub use keys::{KadenaAddressEncoder, KadenaKeyDeriver, derive_address};
```

### `rpc`-feature re-exports (server-side only)

```rust
pub use fees::KadenaFees;
pub use plugin::{KadenaChainPlugin, kadena_plugin};
```

Signing primitives are reachable via the plugin's `TransactionSigner`
component; there is no top-level `*TransactionSigner` re-export.

### Factory

```rust
/// Create a KadenaChainPlugin for mainnet.
#[cfg(feature = "rpc")]
pub fn kadena_plugin() -> KadenaChainPlugin;
```

### Key Types

| Type | Trait | Notes |
|------|-------|-------|
| `KadenaChainPlugin` | `ChainPlugin` | Main plugin (rpc-only) |
| `KadenaConfig` | — | Chain config: API URLs, explorer URLs, Chainweb network versions |
| `KadenaKeyDeriver` | `KeyDeriver` | BIP-44 `m/44'/626'/0'/0/0` |
| `KadenaAddressEncoder` | `AddressEncoder` | `k:` + hex pubkey |
| `KadenaFees` | — | Pact gas params (gasLimit, gasPrice) |
| `KadenaError` | `Error` | 5 variants: `KeyDerivation`, `Address`, `Transaction`, `Signing`, `Network` |

### Networks

| `NetworkId` | Label | Category |
|-------------|-------|----------|
| `kadena-mainnet` | Kadena Mainnet | Mainnet |
| `kadena-testnet` | Kadena Testnet (testnet05) | Testnet |

## Module Map

```
src/
├── error.rs      — KadenaError enum (thiserror)
├── config.rs     — KadenaConfig: per-NetworkId API/explorer/version maps
├── keys.rs       — KadenaKeyDeriver + KadenaAddressEncoder + derive_address()
├── signing.rs    — Pact-cmd build_signed_transfer (Blake2b-256 + base64);
                    ed25519 TransactionSigner impl lives here, not in a
                    separate tx.rs
├── wasm.rs       — Browser-only: in-browser balance reads via gloo_net (cfg wasm32)
├── fees.rs       — KadenaFees + fee estimator (rpc-only)
├── balance.rs    — Balance fetcher via Pact API (rpc-only)
├── broadcast.rs  — Transaction broadcaster (rpc-only)
├── rest.rs       — Internal Pact REST helpers (rpc-only, private)
├── plugin.rs     — KadenaChainPlugin + kadena_plugin() factory (rpc-only).
                    NetworkProvider impl is built inline here, not in a
                    separate network.rs
└── lib.rs        — Module decls + cfg-gated re-exports
```

## Notes

- **Signing pipeline.** `signing::build_signed_transfer` is shared
  between the in-browser `wasm::send` and the server-side
  `plugin::send_transfer` — both Blake2b-256 hash the Pact cmd
  string and URL-safe-base64 the digest.
- **Always-on always-needed deps.** `blake2`, `base64`, `ed25519-dalek`,
  `zeroize`, `hex` are always-on (not behind `rpc`) because the WASM
  signing pipeline uses them.
- **Community fork.** This re-implements the Kadena LLC community
  client design after the LLC shutdown; the Pact protocol itself is
  unchanged.
