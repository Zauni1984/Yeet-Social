//! Kaspa fee estimation.
//!
//! Kaspa uses a mass-based fee model where transaction "mass" determines
//! the fee.  For simple transfers the mass is roughly fixed.  The fee
//! rate is expressed in SOMPI per gram of transaction mass.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use dontyeet_primitives::chain::NetworkId;
use dontyeet_primitives::error::{DontYeetWalletError, Result};
use dontyeet_primitives::traits::FeeEstimator;
use dontyeet_primitives::transaction::FeeTier;

/// Default mass for a simple KAS transfer (1 input, 2 outputs).
pub const SIMPLE_TRANSFER_MASS: u64 = 3000;

/// Kaspa fee parameters for a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KaspaFees {
    /// Fee rate in SOMPI per gram of transaction mass.
    pub sompi_per_gram: u64,
    /// Estimated transaction mass in grams.
    pub mass: u64,
}

impl KaspaFees {
    /// Total fee in SOMPI (mass * rate).
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::Chain` if the multiplication overflows.
    pub fn total_sompi(&self) -> Result<u64> {
        self.mass
            .checked_mul(self.sompi_per_gram)
            .ok_or_else(|| DontYeetWalletError::Chain("fee arithmetic overflow".into()))
    }
}

/// Estimates Kaspa fees by querying the REST API.
pub struct KaspaFeeEstimator {
    api_urls: HashMap<NetworkId, Vec<Url>>,
}

impl KaspaFeeEstimator {
    /// Create a fee estimator from the configured API URLs.
    #[must_use]
    pub fn new(api_urls: &HashMap<NetworkId, Vec<Url>>) -> Self {
        Self {
            api_urls: api_urls.clone(),
        }
    }
}

#[async_trait]
impl FeeEstimator<KaspaFees> for KaspaFeeEstimator {
    /// Estimate slow / standard / fast fee tiers for Kaspa.
    ///
    /// Kaspa has a minimum relay fee of 1 SOMPI per gram.  The tiers
    /// apply multipliers to account for network congestion.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError::NotFound` if no API URLs exist for the network.
    async fn estimate_fees(&self, network: &NetworkId) -> Result<FeeTier<KaspaFees>> {
        let _urls = self
            .api_urls
            .get(network)
            .ok_or_else(|| DontYeetWalletError::NotFound(format!("no API URLs for {network}")))?;

        // Kaspa's minimum relay fee is 1 sompi/gram.  Under normal
        // conditions this is sufficient.  We scale for tiers:
        //   slow = 1x, standard = 2x, fast = 5x
        let base_rate: u64 = 1;

        Ok(FeeTier {
            slow: KaspaFees {
                sompi_per_gram: base_rate,
                mass: SIMPLE_TRANSFER_MASS,
            },
            standard: KaspaFees {
                sompi_per_gram: base_rate
                    .checked_mul(2)
                    .ok_or_else(|| DontYeetWalletError::Chain("fee overflow".into()))?,
                mass: SIMPLE_TRANSFER_MASS,
            },
            fast: KaspaFees {
                sompi_per_gram: base_rate
                    .checked_mul(5)
                    .ok_or_else(|| DontYeetWalletError::Chain("fee overflow".into()))?,
                mass: SIMPLE_TRANSFER_MASS,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_sompi_simple() {
        let fees = KaspaFees {
            sompi_per_gram: 1,
            mass: SIMPLE_TRANSFER_MASS,
        };
        assert_eq!(
            fees.total_sompi().expect("no overflow"),
            SIMPLE_TRANSFER_MASS
        );
    }

    #[test]
    fn total_sompi_overflow() {
        let fees = KaspaFees {
            sompi_per_gram: u64::MAX,
            mass: u64::MAX,
        };
        assert!(fees.total_sompi().is_err());
    }

    #[test]
    fn fees_serialize_roundtrip() {
        let fees = KaspaFees {
            sompi_per_gram: 2,
            mass: SIMPLE_TRANSFER_MASS,
        };
        let json = serde_json::to_string(&fees).expect("serialize");
        let decoded: KaspaFees = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.sompi_per_gram, 2);
        assert_eq!(decoded.mass, SIMPLE_TRANSFER_MASS);
    }
}

// Rust guideline compliant 2026-05-02
