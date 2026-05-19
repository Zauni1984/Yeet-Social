//! Factory functions for each supported EVM chain.
//!
//! Each submodule exports a single function that returns a fully configured
//! [`EvmChainPlugin`](crate::plugin::EvmChainPlugin).

pub mod avalanche;
pub mod bnb;
pub mod ethereum;
pub mod polygon;
pub mod sonic;

pub use avalanche::avalanche_plugin;
pub use bnb::bnb_plugin;
pub use ethereum::ethereum_plugin;
pub use polygon::polygon_plugin;
pub use sonic::sonic_plugin;

#[cfg(test)]
mod tests {
    use dontyeet_primitives::chain::ChainId;
    use dontyeet_primitives::traits::ChainPlugin;

    use super::*;

    #[test]
    fn ethereum_plugin_has_correct_config() {
        let p = ethereum_plugin();
        assert_eq!(*p.chain_id(), ChainId::Ethereum);
        assert_eq!(p.native_asset().symbol, "ETH");
        assert_eq!(p.network_provider().networks().len(), 2);
        assert_eq!(p.config().evm_chain_id_mainnet, 1);
    }

    #[test]
    fn polygon_plugin_has_correct_config() {
        let p = polygon_plugin();
        assert_eq!(*p.chain_id(), ChainId::Polygon);
        assert_eq!(p.native_asset().symbol, "POL");
        assert_eq!(p.network_provider().networks().len(), 2);
        assert_eq!(p.config().evm_chain_id_mainnet, 137);
    }

    #[test]
    fn bnb_plugin_has_correct_config() {
        let p = bnb_plugin();
        assert_eq!(*p.chain_id(), ChainId::Bnb);
        assert_eq!(p.native_asset().symbol, "BNB");
        assert_eq!(p.network_provider().networks().len(), 2);
        assert_eq!(p.config().evm_chain_id_mainnet, 56);
    }

    #[test]
    fn avalanche_plugin_has_correct_config() {
        let p = avalanche_plugin();
        assert_eq!(*p.chain_id(), ChainId::Avalanche);
        assert_eq!(p.native_asset().symbol, "AVAX");
        assert_eq!(p.network_provider().networks().len(), 2);
        assert_eq!(p.config().evm_chain_id_mainnet, 43_114);
    }

    #[test]
    fn sonic_plugin_has_correct_config() {
        let p = sonic_plugin();
        assert_eq!(*p.chain_id(), ChainId::Sonic);
        assert_eq!(p.native_asset().symbol, "S");
        assert_eq!(p.network_provider().networks().len(), 2);
        assert_eq!(p.config().evm_chain_id_mainnet, 146);
    }
}

// Rust guideline compliant 2026-05-02
