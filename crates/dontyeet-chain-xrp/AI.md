# dontyeet-chain-xrp

**Layer:** Integration | **Deps:** primitives, crypto, chain, network

## Purpose
Xrp chain plugin implementation. Implements `ChainPlugin` trait.

## Key Types
- `XrpChainPlugin` — implements ChainPlugin
- Chain-specific key derivation, tx building, signing, balance, fees, broadcasting

## Extensibility
Export a factory function: `pub fn xrp_plugin(config) -> impl ChainPlugin`
