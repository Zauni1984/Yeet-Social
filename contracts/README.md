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
`evm_version = "cancun"`), forge-std v1.9.6, remappings in `remappings.txt`.
Last full run: **54/54 tests green** (2026-10-27).

## What gets deployed (points model)

Only **`YeetToken`** — see [docs/mica/10](../docs/mica/10-testnet-contract.md).
Points, tips, pay-per-view and paper wallets live in the backend ledger; the
chain is touched once per approved conversion, when the backend `minter`
mints YEET into the user's own wallet. `YeetPayments`, `PaperWalletEscrow`,
`YeetNFT` and `YeetTipping` stay in the repo (tested) but **undeployed**
until on-chain payments are switched on deliberately.

## Contracts

| Contract | Description |
|---|---|
| `YeetToken.sol` | BEP-20, fixed 21 B cap, burnable, permit, Ownable2Step; 25 % minted at deploy, 75 % reward reserve mintable only via `batchMintRewards` by the **`minter`** role (backend hot key), bounded by `dailyMintCap` per UTC day; owner can pause minting (`setMintingPaused`), rotate the minter (`setMinter`) and set the cap — the owner never mints itself |
| `YeetTipping.sol` | Wallet-to-wallet YEET tips, 10% platform fee, pausable |
| `YeetNFT.sol` | ERC-721, posts as NFTs, 10% creator royalties |
| `YeetPayments.sol` | Non-custodial tips / PPV / promotion, atomic fee split, never holds funds (docs/mica/07) |
| `PaperWalletEscrow.sol` | Bearer vouchers as on-chain escrow, signature-bound claim (front-running safe), no admin sweep |

## Deployed Addresses (Testnet, chain 97)

> Fill in after the first `scope: token` run of the Deploy Contracts workflow.

| Contract | Address | Owner | Minter | Daily cap |
|---|---|---|---|---|
| YeetToken | TBD | TBD (deployer → multisig later) | TBD (backend `REWARDS_MINTER_PRIVKEY`) | 1 000 000 YEET |

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
git clone --depth 1 --branch v5.1.0 https://github.com/OpenZeppelin/openzeppelin-contracts lib/openzeppelin-contracts
git clone --depth 1 --branch v1.9.6 https://github.com/foundry-rs/forge-std lib/forge-std
forge build
forge test -v
```

## Deploy the token (Testnet first)

Preferred: GitHub → Actions → **Deploy Contracts** → `network: bsc_testnet`,
`scope: token`, `minter_address: <backend minter wallet>`, `daily_mint_cap: 1000000`.
Secrets: `BNB_PRIVATE_KEY` (deployer = initial owner), `BSCSCAN_API_KEY`.

Locally:

```bash
cp .env.example .env            # PRIVATE_KEY, BSCSCAN_API_KEY
export MINTER_ADDRESS=0x...     # address of the backend's REWARDS_MINTER_PRIVKEY (default: deployer — testnet only)
export DAILY_MINT_CAP=1000000   # whole YEET per UTC day (0 = uncapped)
# export OWNER_ADDRESS=0x...    # optional multisig; it must call acceptOwnership()

forge script script/DeployToken.s.sol --rpc-url bsc_testnet --broadcast --verify -vvvv
```

The script prints the backend `.env` lines (`YEET_TOKEN_ADDRESS`, `YEET_CHAIN_ID`).
On **mainnet** (`--rpc-url bsc_mainnet`) it refuses to run unless
`MINTER_ADDRESS` differs from the deployer and `DAILY_MINT_CAP` is non-zero —
and it belongs after the external audit and the multisig hand-over
(docs/mica/10 §6). `script/Deploy.s.sol` (token + payments + escrow + NFT,
workflow `scope: all`) is only for the later on-chain-payments phase.
