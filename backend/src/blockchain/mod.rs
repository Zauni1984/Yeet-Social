use anyhow::Result;
use ethers::{
    prelude::*,
    providers::{Http, Provider},
};
use std::sync::Arc;

pub type BscProvider = Provider<Http>;

/// Client for Binance Smart Chain interactions
pub struct BscClient {
    pub provider: Arc<BscProvider>,
    pub yeet_token_address: Address,
    pub nft_contract_address: Address,
}

impl BscClient {
    pub async fn new(rpc_url: &str) -> Result<Self> {
        let provider = Provider::<Http>::try_from(rpc_url)?;
        let provider = Arc::new(provider);

        let yeet_token_address = std::env::var("YEET_TOKEN_ADDRESS")
            .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".into())
            .parse::<Address>()?;

        let nft_contract_address = std::env::var("YEET_NFT_ADDRESS")
            .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".into())
            .parse::<Address>()?;

        Ok(Self { provider, yeet_token_address, nft_contract_address })
    }

    /// Get BNB balance for a wallet address
    pub async fn get_bnb_balance(&self, address: &str) -> Result<f64> {
        let addr: Address = address.parse()?;
        let balance = self.provider.get_balance(addr, None).await?;
        // Convert from wei to BNB (18 decimals)
        let bnb = ethers::utils::format_units(balance, 18)?
            .parse::<f64>()?;
        Ok(bnb)
    }

    /// Get YEET token balance (BEP-20)
    pub async fn get_yeet_balance(&self, wallet: &str) -> Result<f64> {
        // ABI for balanceOf
        abigen!(
            ERC20,
            r#"[{"inputs":[{"name":"account","type":"address"}],"name":"balanceOf","outputs":[{"name":"","type":"uint256"}],"stateMutability":"view","type":"function"}]"#
        );
        let addr: Address = wallet.parse()?;
        let token = ERC20::new(self.yeet_token_address, self.provider.clone());
        let balance = token.balance_of(addr).call().await?;
        let amount = ethers::utils::format_units(balance, 18)?.parse::<f64>()?;
        Ok(amount)
    }

    /// Verify a wallet signature (for login without password)
    pub fn verify_signature(
        &self,
        wallet: &str,
        message: &str,
        signature: &str,
    ) -> Result<bool> {
        let addr: Address = wallet.parse()?;
        let sig: Signature = signature.parse()?;
        let recovered = sig.recover(message)?;
        Ok(recovered == addr)
    }

    /// Get transaction status from BSC
    pub async fn get_tx_status(&self, tx_hash: &str) -> Result<bool> {
        let hash: TxHash = tx_hash.parse()?;
        match self.provider.get_transaction_receipt(hash).await? {
            Some(receipt) => Ok(receipt.status == Some(1u64.into())),
            None => Ok(false),
        }
    }
}
