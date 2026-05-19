# dontyeet-chain-cardano

**Layer:** Integration | **Deps:** primitives, crypto, chain, network

## Purpose

Cardano chain plugin. CIP-1852 / Ed25519 derivation, blake2b-224
enterprise addresses (no staking), Blockfrost REST v0 backend.
Mainnet, preprod, and preview networks.

## Factories

```rust
pub fn cardano_plugin() -> CardanoChainPlugin;
pub fn CardanoChainPlugin::new(config: CardanoConfig) -> CardanoChainPlugin;
```

The no-arg factory builds the default mainnet profile. The app
server (`dontyeet-app`) prefers `CardanoChainPlugin::new(...)`
because it needs to inject the Blockfrost project_id.

## Configuration

`CardanoConfig::blockfrost_project_id: Option<String>` — required for
Blockfrost calls to succeed. Falls back to:

1. `[cardano] blockfrost_project_id = "..."` in `dontyeet.toml`
2. `BLOCKFROST_PROJECT_ID` environment variable
3. None — every call returns `403 Forbidden` from Blockfrost.

The app server logs a `warn!` at startup if the project_id is missing
so the operator notices before users hit broken Cardano flows.

## Key Types

| Type | Module | Description |
|------|--------|-------------|
| `CardanoChainPlugin` | `plugin` | The `ChainPlugin` impl |
| `CardanoConfig` | `config` | Per-network base URLs + Blockfrost auth |
| `CardanoKeyDeriver` | `keys` | CIP-1852 → Ed25519 keypair |
| `CardanoAddressEncoder` | `keys` | bech32 (`addr1` / `addr_test1`) + blake2b-224 + structural validate |
| `CardanoFees` | `fees` | Per-byte fee + flat constant per protocol params |

## Module Layout

```
src/
  lib.rs          Re-exports
  error.rs        CardanoError
  config.rs       CardanoConfig + per-network base URLs + default builder
  auth.rs         Blockfrost project_id header injection
  keys.rs         Derivation + bech32 enterprise-address encode/decode
  fees.rs         CardanoFees + protocol-param fetch
  balance.rs      Blockfrost `/addresses/{addr}` lump-sum lovelace
  transfer.rs     UTXO selection + raw-tx CBOR assembly
  signing.rs      Ed25519 signer
  broadcast.rs    Blockfrost `/tx/submit` POST
  history.rs      Koios-style `/address_txs` + batched `/tx_info`
  plugin.rs       CardanoChainPlugin glue
  wasm.rs         wasm32 entry points
```

## Networks

- `cardano-mainnet` — `https://cardano-mainnet.blockfrost.io/api/v0`
- `cardano-preprod` — `https://cardano-preprod.blockfrost.io/api/v0`
- `cardano-preview` — `https://cardano-preview.blockfrost.io/api/v0`

## Why enterprise (no staking) addresses

The wallet is a payments-first surface today. Enterprise addresses
(payment-key only, no stake credential) are the simplest CIP-1852
output and avoid the entire delegation / withdrawal path. Adding a
stake credential is a future extension — it changes the address
prefix and the balance-fetch endpoint but not the rest of the plugin.
