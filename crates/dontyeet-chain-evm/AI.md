# dontyeet-chain-evm

**Layer:** Integration | **Deps:** primitives, crypto, chain, network

## Purpose

EVM chain plugin for `DontYeetWallet`. Handles Ethereum, Polygon, BNB, Avalanche, and Sonic via a single parameterized `EvmChainPlugin`.

## Factory Functions (crate root)

```rust
pub fn ethereum_plugin() -> EvmChainPlugin;
pub fn polygon_plugin() -> EvmChainPlugin;
pub fn bnb_plugin() -> EvmChainPlugin;
pub fn avalanche_plugin() -> EvmChainPlugin;
pub fn sonic_plugin() -> EvmChainPlugin;
```

## Key Types

| Type | Module | Description |
|------|--------|-------------|
| `EvmChainPlugin` | `plugin` | Implements `ChainPlugin` — bundles all EVM capabilities |
| `EvmChainConfig` | `config` | Chain config (chain ID, RPC URLs, networks, explorer URLs) |
| `EvmFees` | `fees` | Gas limit + gas price in wei |
| `EvmError` | `error` | EVM-specific error enum |
| `EvmKeyDeriver` | `keys` | Implements `KeyDeriver` via BIP-44 |
| `EvmAddressEncoder` | `keys` | Implements `AddressEncoder` with EIP-55 checksums |
| `EvmFeeEstimator`, `EvmBalanceFetcher`, `EvmTransactionBuilder`, `EvmBroadcaster`, `EvmNetworkProvider` | various | The other per-trait components composed by `EvmChainPlugin` |

## Trait Implementations

| Trait | Implementor |
|-------|-------------|
| `ChainPlugin` | `EvmChainPlugin` |
| `KeyDeriver` | `EvmKeyDeriver` |
| `AddressEncoder` | `EvmAddressEncoder` |
| `FeeEstimator<EvmFees>` | `EvmFeeEstimator` |
| `BalanceFetcher` | `EvmBalanceFetcher` |
| `TransactionBuilder<EvmFees>` | `EvmTransactionBuilder` |
| `TransactionSigner` | `dontyeet_chain::FnSigner` (closure-backed; no per-crate signer struct) |
| `TransactionBroadcaster` | `EvmBroadcaster` |
| `NetworkProvider` | `EvmNetworkProvider` |
| `RpcEndpointProvider` | `EvmNetworkProvider` |
| `TxHistoryFetcher` | EVM history adapter (Etherscan-family) |

`fetch_token_balance` is implemented directly on `EvmChainPlugin`
(plugin.rs ~226–246) using `eth_call` to the active network's mainnet
RPC.

## Module Layout

```
src/
  lib.rs, error.rs, config.rs
  keys.rs, fees.rs, balance.rs
  tx.rs, signing.rs, broadcast.rs
  history.rs, nonce.rs, token_balance.rs
  plugin.rs, rpc.rs (internal)
  wasm.rs                                — `wasm32` entry points: derive_address,
                                            balance, build_signed_payload,
                                            broadcast_signed, history,
                                            token_balance, fee_estimate, health
  chains/ — ethereum.rs, polygon.rs, bnb.rs, avalanche.rs, sonic.rs
```

A `rpc` Cargo feature gates the native build path (`balance`,
`broadcast`, `chains`, `fees`, `history`, `nonce`, `plugin`, `tx`)
out of the WASM build — the WASM bundle uses `wasm.rs` directly via
`fetch` rather than reqwest.

## Per-Network Chain ID Resolution (EIP-155)

The EIP-155 chain ID is captured into the signer at plugin-construction
time, not decoded at sign time:

- `EvmChainPlugin::new(config)` builds an `FnSigner` whose closure
  captures `config.evm_chain_id_mainnet` (plugin.rs:57–60). Every
  `sign(msg, key)` call routes through `signing::sign_legacy_tx(msg,
  key, evm_chain_id)` with the captured ID — no RLP field-7 decode at
  sign time.
- `EvmTransactionBuilder` still holds a `HashMap<NetworkId, u64>`
  built from `config.networks[*].evm_chain_id` and writes field 7 of
  the unsigned RLP at build time, so the signed payload is still
  EIP-155 correct.

This is the friction-reducing pattern `CLAUDE.md` highlights:
`FnSigner` removes the per-chain signer-struct boilerplate, and
captured-state-in-closure is how a stateful signer composes cleanly
with the otherwise-stateless `TransactionSigner` trait.

Each `BlockchainNetwork` in `config.networks` carries its own
`evm_chain_id: Option<u64>`. Networks without an EVM chain ID are
skipped by the builder.

## Adding a New EVM Chain

1. Create `src/chains/<name>.rs` with `pub fn <name>_plugin() -> EvmChainPlugin`
2. Add `pub mod <name>;` and re-export in `chains/mod.rs`
3. Re-export in `lib.rs`
4. Register in `dontyeet-app`

Each `BlockchainNetwork` in the config's `networks` vec must set `evm_chain_id: Some(<id>)` (e.g. `Some(1)` for mainnet, `Some(11_155_111)` for Sepolia). Missing chain IDs will cause `build_simple_transfer` to fail with `NotFound`.
