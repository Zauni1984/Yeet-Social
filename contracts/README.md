# Yeet Social — Smart Contracts

## Network: BNB Smart Chain

| | Testnet | Mainnet |
|---|---|---|
| Chain ID | 97 | 56 |
| Native Token | tBNB | BNB |
| RPC | `https://bsc-testnet-dataseed.bnbchain.org` | `https://bsc-dataseed.bnbchain.org` |
| Explorer | `https://testnet.bscscan.com` | `https://bscscan.com` |
| Faucet | `https://testnet.bnbchain.org/faucet-smart` | — |

> Note: BNB Smart Chain was formerly known as Binance Smart Chain (BSC).
> The Chain IDs (56/97), EVM compatibility, and tooling are unchanged.

## Contracts

| Contract | Description |
|---|---|
| `YeetToken.sol` | BEP-20, 1B max supply, burnable, owner-mintable |
| `YeetTipping.sol` | Wallet-to-wallet YEET tips, 10% platform fee, pausable |
| `YeetNFT.sol` | ERC-721, posts as NFTs, 10% creator royalties |

## Deployed Addresses (Testnet)

> Run deployment and paste addresses here

| Contract | Address |
|---|---|
| YeetToken | TBD |
| YeetTipping | TBD |
| YeetNFT | TBD |

## Token Distribution

| Allocation | % | Amount |
|---|---|---|
| Community / Airdrop | 40% | 400M YEET |
| Reward Pool | 30% | 300M YEET |
| Team & Dev | 20% | 200M YEET |
| Liquidity | 10% | 100M YEET |

## Setup

```bash
# Install Foundry
curl -L https://foundry.paradigm.xyz | bash && foundryup

cd contracts
git clone --depth 1 https://github.com/OpenZeppelin/openzeppelin-contracts lib/openzeppelin-contracts
git clone --depth 1 https://github.com/foundry-rs/forge-std lib/forge-std
forge build
forge test -v
```

## Deploy to BNB Smart Chain Testnet

```bash
cp .env.example .env
# Set PRIVATE_KEY and BSCSCAN_API_KEY

forge script script/Deploy.s.sol \
  --rpc-url bsc_testnet \
  --broadcast \
  --verify \
  -vvvv
```

## Deploy to BNB Smart Chain Mainnet

```bash
forge script script/Deploy.s.sol \
  --rpc-url bsc_mainnet \
  --broadcast \
  --verify \
  -vvvv
```
