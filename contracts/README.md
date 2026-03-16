# Yeet Social — Smart Contracts

## Network
- **Testnet**: BSC Chapel (Chain ID: 97)
- **Mainnet**: BSC (Chain ID: 56) — after testnet validation

## Contracts

| Contract | Description |
|---|---|
| `YeetToken.sol` | BEP-20, 1B supply, burnable, mintable up to cap |
| `YeetTipping.sol` | Wallet-to-wallet YEET tips, 10% platform fee |
| `YeetNFT.sol` | ERC-721, posts as NFTs, creator royalties (10%) |

## Addresses (Chapel Testnet)
> Run deployment script and paste here

| Contract | Address |
|---|---|
| YeetToken | TBD |
| YeetTipping | TBD |
| YeetNFT | TBD |

## Setup

```bash
# Install Foundry
curl -L https://foundry.paradigm.xyz | bash
foundryup

cd contracts
forge install OpenZeppelin/openzeppelin-contracts
forge build
forge test
```

## Deploy to Chapel Testnet

```bash
# 1. Get free tBNB: https://testnet.bnb.org
# 2. Set up .env (copy .env.example)
cp .env.example .env
# edit PRIVATE_KEY and BSCSCAN_API_KEY

# 3. Deploy
forge script script/Deploy.s.sol \
  --rpc-url chapel \
  --broadcast \
  --verify \
  -vvvv
```

## Token Distribution

| Allocation | % | Amount |
|---|---|---|
| Community / Airdrop | 40% | 400M YEET |
| Reward Pool (engagement) | 30% | 300M YEET |
| Team & Development | 20% | 200M YEET |
| Liquidity | 10% | 100M YEET |

## Revenue Model
- **Tips**: 10% platform fee on all YEET tips
- **NFT minting**: Configurable mint fee (0 on testnet)
- **NFT royalties**: 5% platform cut on secondary sales
- **Creator royalties**: 10% on secondary sales
