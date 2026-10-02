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

## Build & test

```bash
cd contracts
forge install foundry-rs/forge-std OpenZeppelin/openzeppelin-contracts@v5.1.0
forge build
forge test
```

Pinned: solc 0.8.24, OpenZeppelin **5.1.0** (≥ 5.2 uses `mcopy` and needs
`evm_version = "cancun"`), remappings in `remappings.txt`. Last full run:
45/45 tests green.

## Contracts

| Contract | Description |
|---|---|
| `YeetToken.sol` | BEP-20, fixed 21 B cap, burnable, Ownable2Step; 25 % minted at deploy, 75 % reward reserve mintable only via `batchMintRewards` (points→YEET) |
| `YeetTipping.sol` | Wallet-to-wallet YEET tips, 10% platform fee, pausable |
| `YeetNFT.sol` | ERC-721, posts as NFTs, 10% creator royalties |
| `YeetPayments.sol` | Non-custodial tips / PPV / promotion, atomic fee split, never holds funds (docs/mica/07) |
| `PaperWalletEscrow.sol` | Bearer vouchers as on-chain escrow, signature-bound claim (front-running safe), no admin sweep |

## Deployed Addresses (Testnet)

> Run deployment and paste addresses here

| Contract | Address |
|---|---|
| YeetToken | TBD |
| YeetTipping | TBD |
| YeetNFT | TBD |

## Token Distribution

Fixed maximum supply **21,000,000,000 YEET** (`MAX_SUPPLY`, docs/mica/02 Teil J).

| Allocation | % | Amount | Issued |
|---|---|---|---|
| Developer | 10% | 2.1B YEET | at deploy → `initialOwner` (vesting: separate contract/multisig policy) |
| Team | 10% | 2.1B YEET | at deploy → `initialOwner` (vesting: separate contract/multisig policy) |
| Reserve (liquidity, listings) | 5% | 1.05B YEET | at deploy → `initialOwner` |
| Rewards / community (`REWARD_RESERVE`) | 75% | 15.75B YEET | gradually, only via `batchMintRewards` — bounded by `rewardsMinted`, so burns never re-open it; there is no other mint function |

The backend's conversion pool (`YEET_CONVERSION_POOL`, default 15.75B) mirrors `REWARD_RESERVE`; `rewardsRemaining()` exposes the on-chain remainder.

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
