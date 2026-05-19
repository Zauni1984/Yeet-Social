# dontyeet-chain-algorand

**Layer:** Integration | **Deps:** primitives, crypto, chain, network

## Purpose
Algorand chain plugin implementation. Implements `ChainPlugin` trait.

## Key Types
- `AlgorandChainPlugin` — implements ChainPlugin
- Chain-specific key derivation, tx building, signing, balance, fees, broadcasting

## Extensibility
Export a factory function: `pub fn algorand_plugin(config) -> impl ChainPlugin`
