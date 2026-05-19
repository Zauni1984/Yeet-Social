# dontyeet-primitives

**Layer:** Foundation (zero internal deps)

## Purpose
All shared types, error enums, and trait definitions. Every other crate depends on this.

## Key Types
- `ChainId` — chain identifier enum
- `NetworkId` — chain-qualified network identifier
- `NetworkCategory` — Mainnet / Testnet / Devnet
- `Address` — validated address newtype
- `Amount` — u128 wrapper with checked arithmetic only
- `AssetInfo` — name, symbol, kind, chain, decimals
- `BlockchainNetwork` — id + label + category
- `FeeTier<F>` — cheap / normal / fast fee suggestions
- `SimpleTxParams<F>` — destination + amount + fees
- `Seed`, `PrivateKey`, `Mnemonic`, `KeyPair` — secret types (Zeroize)

## Key Traits
- `KeyDeriver` — derive keypair from seed
- `AddressEncoder` — public key to address
- `TransactionSigner` — sign raw transaction bytes
- `TransactionBuilder<F>` — build unsigned transaction
- `BalanceFetcher` — fetch balance for address
- `FeeEstimator<F>` — estimate fee tiers
- `TransactionBroadcaster` — broadcast signed tx
- `NetworkProvider` — list networks, explorer URLs
- `RpcEndpointProvider` — RPC URLs per network
- `IdentityResolver` — resolve identifier to address
- `ChainPlugin` — bundles all per-chain traits
- `IdentityPlugin` — bundles identity traits

## Error
- `DontYeetWalletError` — top-level thiserror enum
