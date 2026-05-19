# dontyeet-chain

**Layer:** Services | **Deps:** primitives only (no crypto — trust boundary)

## Purpose
Chain-agnostic wallet orchestrator. Validates, preflights, delegates to `ChainPlugin`, formats results.

## Key Design: Wallet does NOT hold secrets
`Seed` is passed as a parameter per-operation. The wallet never owns key material.

## Send Pipeline
```
Stage 1: VALIDATE          → address_encoder().validate(destination)
Stage 2: PREFLIGHT         → derive keys, check balance >= amount
Stage 3+4: BUILD+SIGN+BCST → plugin.send_transfer(seed, to, amount, &network_id)
```

The orchestrator handles validation and preflight; build / sign /
broadcast is delegated to `ChainPlugin::send_transfer` so each chain
crate composes its own builder + signer + broadcaster against
chain-specific RPC shapes (e.g. EVM uses `FnSigner` with a closure
capturing the network's chain ID; Solana skips the broadcaster trait
entirely and uses its own JSON-RPC shape).

## Key Types
- `Wallet` — wraps `Arc<dyn ChainPlugin>`; 12 public methods
- `SendResult` — `{ tx_hash, explorer_url }`
- `WalletError` — 9 variants mapped to pipeline stages

## Public API
- `new(plugin)` — construct with default network
- `chain_id()`, `asset_info()`, `plugin_arc()` — metadata + access
- `address(seed)`, `balance(&address)`, `explorer_url(seed)` — note
  `balance` takes a previously-derived `&Address`, **not** a seed —
  the seed never leaves the signing crate, and balance is a public
  data read.
- `current_network()`, `networks()`, `change_network(id)` — network management
- `fee_estimator()` — type-erased fee estimation
- `send(seed, to, amount, fees_any)` — full pipeline (returns `SendResult`)

## Helpers
- `FnSigner` (in `fn_signer` module) — `TransactionSigner` adapter that
  wraps a function pointer or closure. Replaces the per-chain
  `XxxTransactionSigner` unit-struct boilerplate. Constructor takes
  `impl Fn(&[u8], &PrivateKey) -> Result<Vec<u8>> + Send + Sync + 'static`;
  internally stores `Box<dyn Fn ...>` so the field type stays uniform
  whether the chain passes a function pointer or a closure with
  captured state (e.g. EVM's chain id captured at plugin-construction
  time — the RLP `v` field is set from the closure, not decoded mid-sign).
- `chain_plugin_accessors!` macro (in `macros` module) — the friction-
  reducer called out in `CLAUDE.md`. Generates the boilerplate for
  the typed-getter traits a `ChainPlugin` impl needs (key deriver,
  balance fetcher, signer, broadcaster, fee estimator, address
  encoder, network provider, tx builder, etc.) so a new chain crate
  declares the concrete components in one struct and gets the trait
  glue for free. Use this when adding a chain.
