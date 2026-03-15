# Yeet — Decentralized Social Media

[![CI](https://github.com/Zauni1984/Yeet/actions/workflows/ci.yml/badge.svg)](https://github.com/Zauni1984/Yeet/actions/workflows/ci.yml)
[![Security](https://github.com/Zauni1984/Yeet/actions/workflows/security.yml/badge.svg)](https://github.com/Zauni1984/Yeet/actions/workflows/security.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Decentralized social media platform on Binance Smart Chain. Own your content, earn YEET tokens, post as NFTs.

---

## Tech Stack

| Layer | Technology |
|---|---|
| Backend | Rust · Axum · sqlx · PostgreSQL · Redis |
| Frontend | Rust · Leptos · WebAssembly |
| Blockchain | BSC · Solidity · ethers-rs · Foundry |
| Infra | Docker · Nginx · GitHub Actions |

## Features (MVP)

- **Wallet Login** — MetaMask / WalletConnect on BSC, no password needed
- **Posts & Feed** — Global, Following, Subscriptions · 18+ filter
- **24h Auto-Delete** — Hybrid: DB timer, reset on reshare · NFT posts are permanent
- **YEET Token (BEP-20)** — Earned by: login, post, comment, reshare, downvote, NFT mint
- **Tipping** — YEET/BNB directly on posts · 10% platform fee
- **NFT Posts (BEP-721)** — Mint any post as NFT · 5 YEET burn fee · IPFS storage
- **Web Board API** — Connect any RSS/forum to your Yeet feed
- **Pay-per-view** — Gate content behind YEET tokens

## Quick Start

```bash
# 1. Clone
git clone https://github.com/Zauni1984/Yeet.git
cd Yeet

# 2. Environment
cp .env.example .env
# → Edit .env with your values

# 3. Start database + cache
docker compose up -d postgres redis

# 4. Run backend
cargo run --bin yeet-server

# 5. Run frontend (in separate terminal)
cd frontend && trunk serve
```

## Smart Contracts

```bash
cd contracts

# Install dependencies
forge install OpenZeppelin/openzeppelin-contracts

# Run tests
forge test -vvv

# Deploy to BSC Testnet
forge script script/Deploy.s.sol:DeployYeet \
  --rpc-url https://data-seed-prebsc-1-s1.binance.org:8545/ \
  --broadcast --verify
```

## CI/CD

| Branch | Trigger | Action |
|---|---|---|
| `develop` | Push / PR | CI Tests + Deploy → Staging + BSC Testnet |
| `main` | Push (from PR) | CI Tests + Deploy → Production + BSC Mainnet |

## Required GitHub Secrets

| Secret | Description |
|---|---|
| `JWT_SECRET` | `openssl rand -hex 64` |
| `POSTGRES_PASSWORD` | DB password |
| `DEPLOYER_PRIVATE_KEY` | Contract deployer wallet |
| `REWARDS_MINTER_ADDRESS` | Backend hot wallet address |
| `REWARDS_MINTER_PRIVKEY` | Backend hot wallet private key |
| `TEAM_WALLET` | Team token vesting wallet |
| `ECOSYSTEM_WALLET` | Ecosystem allocation wallet |
| `LIQUIDITY_WALLET` | Liquidity pool wallet |
| `BSCSCAN_API_KEY` | For contract verification |
| `SSH_HOST` | Server IP/hostname |
| `SSH_USER` | SSH username |
| `SSH_PRIVATE_KEY` | SSH private key for deploy |

## Token Economics

- **Total supply:** 1,000,000,000 YEET
- **Rewards pool:** 400M (minted on-demand for user actions)
- **Team:** 150M (2-year vesting)
- **Ecosystem:** 200M
- **Liquidity:** 150M
- **Reserve:** 100M

## License

MIT — see [LICENSE](LICENSE)
