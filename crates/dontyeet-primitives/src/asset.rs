//! Asset metadata types.

use serde::{Deserialize, Serialize};

use crate::chain::ChainId;

/// Whether an asset is a native coin or a token on a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    /// Native coin of a chain (ETH, BTC, SOL, …).
    Coin,
    /// Token deployed on a chain (ERC-20, SPL, TRC-20, …).
    Token,
}

/// Metadata describing an asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetInfo {
    /// Full name, e.g. `"Ether"`, `"Bitcoin"`.
    pub name: String,
    /// Ticker symbol, e.g. `"ETH"`, `"BTC"`.
    pub symbol: String,
    /// Coin or token.
    pub kind: AssetKind,
    /// Which chain this asset lives on.
    pub chain_id: ChainId,
    /// Decimal places (e.g. 18 for ETH, 8 for BTC, 9 for SOL).
    pub decimals: u8,
}

/// Native asset definitions for all supported chains (April 2026).
impl AssetInfo {
    /// Ether — Ethereum native coin.
    #[must_use]
    pub fn eth() -> Self {
        Self {
            name: "Ether".into(),
            symbol: "ETH".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Ethereum,
            decimals: 18,
        }
    }

    /// POL — Polygon `PoS` native coin (formerly MATIC).
    #[must_use]
    pub fn pol() -> Self {
        Self {
            name: "POL".into(),
            symbol: "POL".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Polygon,
            decimals: 18,
        }
    }

    /// BNB — BNB Smart Chain native coin.
    #[must_use]
    pub fn bnb() -> Self {
        Self {
            name: "BNB".into(),
            symbol: "BNB".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Bnb,
            decimals: 18,
        }
    }

    /// AVAX — Avalanche C-Chain native coin.
    #[must_use]
    pub fn avax() -> Self {
        Self {
            name: "Avalanche".into(),
            symbol: "AVAX".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Avalanche,
            decimals: 18,
        }
    }

    /// S — Sonic native coin (formerly FTM / Fantom).
    #[must_use]
    pub fn sonic() -> Self {
        Self {
            name: "Sonic".into(),
            symbol: "S".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Sonic,
            decimals: 18,
        }
    }

    /// BTC — Bitcoin native coin.
    #[must_use]
    pub fn btc() -> Self {
        Self {
            name: "Bitcoin".into(),
            symbol: "BTC".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Bitcoin,
            decimals: 8,
        }
    }

    /// SOL — Solana native coin.
    #[must_use]
    pub fn sol() -> Self {
        Self {
            name: "Solana".into(),
            symbol: "SOL".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Solana,
            decimals: 9,
        }
    }

    /// ADA — Cardano native coin.
    #[must_use]
    pub fn ada() -> Self {
        Self {
            name: "Cardano".into(),
            symbol: "ADA".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Cardano,
            decimals: 6,
        }
    }

    /// XRP — XRP Ledger native coin.
    #[must_use]
    pub fn xrp() -> Self {
        Self {
            name: "XRP".into(),
            symbol: "XRP".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Xrp,
            decimals: 6,
        }
    }

    /// ALGO — Algorand native coin.
    #[must_use]
    pub fn algo() -> Self {
        Self {
            name: "Algorand".into(),
            symbol: "ALGO".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Algorand,
            decimals: 6,
        }
    }

    /// TRX — TRON native coin.
    #[must_use]
    pub fn trx() -> Self {
        Self {
            name: "TRON".into(),
            symbol: "TRX".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Tron,
            decimals: 6,
        }
    }

    /// KAS — Kaspa native coin (1 KAS = 100,000,000 SOMPI).
    #[must_use]
    pub fn kas() -> Self {
        Self {
            name: "Kaspa".into(),
            symbol: "KAS".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Kaspa,
            decimals: 8,
        }
    }

    /// KDA — Kadena native coin (Chainweb, 12 decimal precision).
    #[must_use]
    pub fn kda() -> Self {
        Self {
            name: "Kadena".into(),
            symbol: "KDA".into(),
            kind: AssetKind::Coin,
            chain_id: ChainId::Kadena,
            decimals: 12,
        }
    }
}

// Rust guideline compliant 2026-05-02
