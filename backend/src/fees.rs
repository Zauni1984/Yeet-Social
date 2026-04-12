// YEET Social — Fee Configuration
// 10% platform fee on all transactions
// Fees accumulate in central fee wallet
// When equivalent of $250 USD reached → auto-transfer to cold wallet

pub const PLATFORM_FEE_PERCENT: f64 = 10.0;

// Dummy addresses for testnet — replace before mainnet
pub const FEE_WALLET_ADDRESS: &str = "0xFEE_DUMMY_TESTNET_YEET_PLATFORM_WALLET_001";
pub const COLD_WALLET_ADDRESS: &str = "0xCOLD_DUMMY_TESTNET_YEET_COLD_WALLET_001";
pub const FEE_THRESHOLD_USD: f64 = 250.0;

/// Calculate platform fee (10%) and creator amount (90%)
pub fn split_amount(total: f64) -> (f64, f64) {
    let fee = (total * PLATFORM_FEE_PERCENT / 100.0 * 10000.0).round() / 10000.0;
    let creator = (total * 10000.0).round() / 10000.0 - fee;
    (fee, creator)
}

/// Record fee to central fee wallet (DB + future blockchain tx)
/// Returns (fee_amount, creator_amount)
pub fn apply_fee(amount: f64) -> (f64, f64) {
    split_amount(amount)
}
