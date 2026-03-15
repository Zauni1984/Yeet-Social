# Yeet — Decentralized Social Media

[![CI](https://github.com/Zauni1984/Yeet-Social/actions/workflows/ci.yml/badge.svg)](https://github.com/Zauni1984/Yeet-Social/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Decentralized social media platform on Binance Smart Chain.
Own your content, earn YEET tokens, post as NFTs.

## Tech Stack

| Layer | Technology |
|---|---|
| Backend | Rust · Axum · sqlx · PostgreSQL · Redis |
| Frontend | Rust · Leptos · WebAssembly |
| Blockchain | BSC · Solidity · ethers-rs · Foundry |
| Infra | Docker · Nginx · GitHub Actions |

## Quick Start

```bash
cp .env.example .env
# Fill in .env values
docker compose up -d postgres redis
cargo run --bin yeet-server
# In separate terminal:
cd frontend && trunk serve
```

## Smart Contracts (BSC)

```bash
cd contracts
forge install OpenZeppelin/openzeppelin-contracts
forge test -vvv
forge script script/Deploy.s.sol:DeployYeet --rpc-url bsc_testnet --broadcast --verify
```

## Features

- Wallet Login (MetaMask / WalletConnect on BSC)
- Posts & Feed (Global, Following, Subscriptions, 18+ filter)
- 24h Auto-Delete (DB timer, reset on reshare — NFT posts permanent)
- YEET Token BEP-20 (earned by: login, post, comment, reshare, NFT mint)
- Tipping (YEET/BNB, 10% platform fee)
- NFT Posts BEP-721 (5 YEET burn fee, IPFS storage)
- Pay-per-view content
- Web Board RSS connector

## License

MIT
 
