# dontyeet-chain-tron

**Layer:** Integration | **Deps:** primitives, crypto, chain, network

## Purpose
Tron chain plugin implementation. Implements `ChainPlugin` trait.

## Key Types
- `TronChainPlugin` — implements ChainPlugin
- Chain-specific key derivation, tx building, signing, balance, fees, broadcasting

## Extensibility
Export a factory function: `pub fn tron_plugin(config) -> impl ChainPlugin`
