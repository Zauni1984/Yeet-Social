//! Checked-arithmetic amount types for financial calculations.
//!
//! A single overflow in a crypto wallet means lost funds.
//! `Amount` exposes **only** checked operations — there is no way to
//! silently overflow.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::error::{DontYeetWalletError, Result};

/// A non-negative amount in the smallest unit of a currency (wei, lamport,
/// satoshi, lovelace, etc.).
///
/// All arithmetic is checked.  Operations return `Err` on overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Amount {
    /// Raw value in smallest denomination.
    value: u128,
    /// Number of decimal places for display (e.g. 18 for ETH, 8 for BTC).
    decimals: u8,
}

impl Amount {
    /// Zero value with the given decimal precision.
    #[must_use]
    pub const fn zero(decimals: u8) -> Self {
        Self { value: 0, decimals }
    }

    /// Create an amount from a raw base-unit integer.
    #[must_use]
    pub const fn from_raw(value: u128, decimals: u8) -> Self {
        Self { value, decimals }
    }

    /// Raw value in the smallest unit.
    #[must_use]
    pub const fn raw(&self) -> u128 {
        self.value
    }

    /// Decimal precision.
    #[must_use]
    pub const fn decimals(&self) -> u8 {
        self.decimals
    }

    /// Whether the amount is zero.
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        self.value == 0
    }

    /// Checked addition.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the operation overflows or decimals mismatch.
    pub fn checked_add(self, rhs: Self) -> Result<Self> {
        Self::assert_same_decimals(self.decimals, rhs.decimals)?;
        self.value
            .checked_add(rhs.value)
            .map(|v| Self {
                value: v,
                decimals: self.decimals,
            })
            .ok_or_else(|| DontYeetWalletError::Validation("amount overflow on add".into()))
    }

    /// Checked subtraction.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the operation underflows or decimals mismatch.
    pub fn checked_sub(self, rhs: Self) -> Result<Self> {
        Self::assert_same_decimals(self.decimals, rhs.decimals)?;
        self.value
            .checked_sub(rhs.value)
            .map(|v| Self {
                value: v,
                decimals: self.decimals,
            })
            .ok_or_else(|| DontYeetWalletError::Validation("amount underflow on sub".into()))
    }

    /// Checked multiplication by a scalar.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the operation overflows.
    pub fn checked_mul(self, factor: u128) -> Result<Self> {
        self.value
            .checked_mul(factor)
            .map(|v| Self {
                value: v,
                decimals: self.decimals,
            })
            .ok_or_else(|| DontYeetWalletError::Validation("amount overflow on mul".into()))
    }

    /// Checked division by a scalar.
    ///
    /// # Errors
    /// Returns `DontYeetWalletError` if the divisor is zero.
    pub fn checked_div(self, divisor: u128) -> Result<Self> {
        if divisor == 0 {
            return Err(DontYeetWalletError::Validation("division by zero".into()));
        }
        self.value
            .checked_div(divisor)
            .map(|v| Self {
                value: v,
                decimals: self.decimals,
            })
            .ok_or_else(|| DontYeetWalletError::Validation("amount overflow on div".into()))
    }

    /// Format as a human-readable decimal string (e.g. `"1.5"` for 1.5 ETH).
    #[must_use]
    pub fn to_display_string(&self) -> String {
        if self.decimals == 0 {
            return self.value.to_string();
        }
        let divisor = 10u128.pow(u32::from(self.decimals));
        let whole = self.value / divisor;
        let frac = self.value % divisor;
        if frac == 0 {
            whole.to_string()
        } else {
            let frac_str = format!("{frac:0>width$}", width = self.decimals as usize);
            let trimmed = frac_str.trim_end_matches('0');
            format!("{whole}.{trimmed}")
        }
    }

    fn assert_same_decimals(a: u8, b: u8) -> Result<()> {
        if a != b {
            return Err(DontYeetWalletError::Validation(format!(
                "decimal mismatch: {a} vs {b}"
            )));
        }
        Ok(())
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_display_string())
    }
}

/// A fiat (or stablecoin) amount for display purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiatAmount {
    /// Currency code, e.g. `"USD"`, `"EUR"`, `"CAD"`.
    pub currency: String,
    /// Amount as a float (acceptable for display only — never for on-chain math).
    pub value: f64,
}

impl fmt::Display for FiatAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2} {}", self.value, self.currency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_eth_amount() {
        let amt = Amount::from_raw(1_500_000_000_000_000_000, 18);
        assert_eq!(amt.to_display_string(), "1.5");
    }

    #[test]
    fn display_btc_amount() {
        let amt = Amount::from_raw(100_000_000, 8);
        assert_eq!(amt.to_display_string(), "1");
    }

    #[test]
    fn checked_add_overflow() {
        let max = Amount::from_raw(u128::MAX, 18);
        let one = Amount::from_raw(1, 18);
        assert!(max.checked_add(one).is_err());
    }

    #[test]
    fn checked_sub_underflow() {
        let zero = Amount::zero(18);
        let one = Amount::from_raw(1, 18);
        assert!(zero.checked_sub(one).is_err());
    }

    #[test]
    fn decimal_mismatch_rejected() {
        let eth = Amount::from_raw(1, 18);
        let btc = Amount::from_raw(1, 8);
        assert!(eth.checked_add(btc).is_err());
    }
}

// Rust guideline compliant 2026-05-02
