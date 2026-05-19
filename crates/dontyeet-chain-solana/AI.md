# dontyeet-chain-solana

**Layer:** Integration | **Deps:** primitives, crypto, chain, network

## Purpose

Solana chain plugin. Ed25519 / Base58, BIP-44 coin type 501, JSON-RPC
backend over `eth_call`-equivalent endpoints. Mainnet, devnet, and
testnet network IDs.

## Factory

```rust
pub fn solana_plugin() -> SolChainPlugin;
```

No-arg factory; wire-level defaults baked into the default `SolConfig`.

## Key Types

| Type | Module | Description |
|------|--------|-------------|
| `SolChainPlugin` | `plugin` | The `ChainPlugin` impl |
| `SolConfig` | `config` | Per-network RPC endpoints + commitment defaults |
| `SolKeyDeriver` | `keys` | BIP-44 → Ed25519 keypair |
| `SolanaFees` | `fees` | Per-instruction lamport units + priority fee tiers |

## Module Layout

```
src/
  lib.rs          Re-exports
  error.rs        SolanaError
  config.rs       SolConfig + per-network RPC URLs
  keys.rs         Derivation + Base58 address encode/decode
  fees.rs         SolanaFees + priority-fee oracle
  balance.rs      JSON-RPC `getBalance`
  transfer.rs     System-program transfer instruction assembly
  signing.rs      Ed25519 signer
  broadcast.rs    JSON-RPC `sendTransaction` + `confirmTransaction`
  history.rs      `getSignaturesForAddress` + batched `getTransaction`
  token_balance.rs `getTokenAccountsByOwner` + decimal lookup
  plugin.rs       SolChainPlugin glue
  wasm.rs         wasm32 entry points
```

## Networks

- `solana-mainnet` — `https://api.mainnet-beta.solana.com`
- `solana-devnet`  — `https://api.devnet.solana.com`
- `solana-testnet` — `https://api.testnet.solana.com`

## Backend

JSON-RPC against the Solana cluster RPC. Adapters chunk batched
`getTransaction` into a single `Vec<JsonRpcRequest>` payload to keep
history fetches at one round-trip instead of N.

History adapter uses the holder's account index to drive pre/post-
balance deltas → direction + amount. The largest opposite-sign delta
across other accounts in the transaction supplies the primary
counterparty.

## SPL token balances

`fetch_token_balance` implementation in `plugin.rs` calls
`getTokenAccountsByOwner` with `mint` filter; decimals come from the
mint metadata cache. Tokens with zero balance are surfaced (the UI's
`token_balance` adapter filters them out for display).
