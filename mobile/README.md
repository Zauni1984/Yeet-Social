# Yeet Social — Mobile App

## Stack
React Native 0.73 · TypeScript · React Navigation · ethers.js v6

API Base: `https://justyeet.it/api/v1`

## Features (v0.1)
- Global feed with real-time posts
- Post cards with 24h expiry countdown bar
- Compose screen
- Wallet screen (WalletConnect v2 in v0.2)
- Profile screen
- Brutalist dark crypto UI — DM Mono + Syne

## Setup
```bash
cd mobile && npm install
npx pod-install ios && npx react-native run-ios
# or: npx react-native run-android
```

## Roadmap
- [ ] WalletConnect deep-link sign-in on mobile
- [ ] YEET BEP-20 token tipping
- [ ] YEET Token deployment on BSC + swap from old chain
- [ ] BSC smart contract integration
- [ ] Push notifications
- [ ] NFT post creation
- [ ] 18+ content toggle
- [ ] Pay-per-view posts
