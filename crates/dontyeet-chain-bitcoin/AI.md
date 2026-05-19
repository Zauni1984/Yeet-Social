# dontyeet-chain-bitcoin

**Layer:** Integration | **Deps:** primitives, crypto, chain, network

## Purpose

Bitcoin chain plugin. Implements `ChainPlugin` for Bitcoin mainnet,
testnet4, and signet via the Mempool.space REST API. Native segwit
(bech32 P2WPKH) addresses; BIP-44 coin type 0 derivation.

## Factory

```rust
pub fn bitcoin_plugin() -> BtcChainPlugin;
```

No-arg factory. Wire-level RPC defaults are baked into the default
`BtcConfig`.

## Key Types

| Type | Module | Description |
|------|--------|-------------|
| `BtcChainPlugin` | `plugin` | The `ChainPlugin` impl |
| `BtcConfig` | `config` | Per-network base URLs, derivation prefixes, fee feeds |
| `BtcKeyDeriver` | `keys` | BIP-44 → secp256k1 → P2WPKH |
| `BtcAddressEncoder` | `keys` | bech32 encode + structural validate (also `AddressFormat::Mainnet` / `Testnet`) |
| `BtcFees` | `fees` | sat/vB tier breakdown (`fast` / `standard` / `slow`) |

## Module Layout

```
src/
  lib.rs          Re-exports
  error.rs        BtcError
  config.rs       BtcConfig + AddressFormat
  keys.rs         Derivation + address encode/decode
  fees.rs         BtcFees + fee feed parsing
  balance.rs      Mempool.space balance query
  transfer.rs     UTXO selection + raw-tx assembly
  signing.rs      ECDSA SegWit signer
  broadcast.rs    Mempool.space tx-submit + tx-id parsing
  history.rs      Mempool.space address-tx walking
  plugin.rs       BtcChainPlugin glue
  wasm.rs         wasm32 entry points (derive_address, balance,
                  build_signed_payload, broadcast_signed, history,
                  fee_estimate, health)
```

## Networks

- `bitcoin-mainnet` — `https://mempool.space/api`
- `bitcoin-testnet4` — `https://mempool.space/testnet4/api`
- `bitcoin-signet` — `https://mempool.space/signet/api`

`config::default_btc_config()` builds the mainnet-only profile used
by `bitcoin_plugin()`; full testnet wiring lives in the builder if
needed.

## Backend

Mempool.space REST (no auth, no rate-limiting in normal use). Calls:
- `/address/{addr}` — balance + UTXO funding/spending sums
- `/tx` POST — broadcast raw hex
- `/address/{addr}/txs` — paginated transaction history (used in
  `wallet::history::mempool` on the UI side)

Token balances (no UTXO-token model on native Bitcoin) are unsupported
and surface as `DontYeetWalletError::Unsupported`.

## Adding a Bitcoin variant (e.g. Litecoin)

Use `dontyeet-chain-generic` for fork chains rather than re-using
this crate — it parameterizes the bech32 prefix, coin type, and base
URL without touching this code.
