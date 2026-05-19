# Yeet Social — TODO / What's Missing

Status of the wallet integration work as it lands in this branch.
Use this as the handoff doc for tomorrow's wiring session.

---

## 1. Operator config (set before backend starts)

The backend silently disables features whose env vars are unset and
logs a warning at startup. Set these on the VPS before going live:

| Env var | What it does | Required for |
|---|---|---|
| `JWT_SECRET` | Already used; min 32 chars | Auth (existing) |
| `DATABASE_URL` | PostgreSQL DSN | Everything (existing) |
| `REDIS_URL` | Redis cache | Nonce + cache (existing) |
| `BSC_CHAIN_ID` | `56` for mainnet, `97` for testnet | Selects on-chain network |
| `BSC_RPC_URL` | Custom RPC override (optional) | Public defaults work |
| `YEET_TOKEN_ADDRESS` | YEET BEP-20 contract address | Indexer + balances |
| `YEET_DEPOSIT_ADDRESS` | Custodian deposit address (where users send YEET for in-app credit) | Deposits |
| `CUSTODIAN_PRIVKEY` | Private key controlling the deposit address | Cashout worker |
| `REWARDS_MINTER_PRIVKEY` | Private key for batch-reward minting | Hourly reward mints (existing) |

`CUSTODIAN_PRIVKEY` and `REWARDS_MINTER_PRIVKEY` should be different
keys so a leak is scoped. Both addresses need a small BNB balance for
gas; the cashout worker also needs a YEET balance to disburse.

---

## 2. Database migrations to apply on launch

In order (some build on earlier ones):

- `0001_init.sql` through `0025_ppv_unlocks.sql` — existing schema
- `0026_indexer_state.sql` — checkpoint table for the BSC Transfer-event indexer
- `0027_yeet_credit.sql` — renames `users.yeet_token_balance` → `yeet_credit_balance`, adds `credit_deposits` + `credit_withdrawals` tables, enforces `users.wallet_address NOT NULL`

`0027` is destructive on the column rename but safe pre-launch (no
real users). Apply before the backend boots once.

---

## 3. UI entry points that exist as functions but aren't wired to a visible button yet

The wasm shim + frontend exposes these on `window.*`. Wire them to
buttons / menu items where they make sense:

| Function | Suggested entry point |
|---|---|
| `window.openWalletCard()` | ✅ Wired — Account → Wallet sidebar link |
| `window.openMultichainReceive()` | ✅ Wired — "Other chains" button inside Wallet card |
| `window.openDepositModal(amount?)` | ✅ Wired — Deposit button inside Wallet card; also auto-fires from `showInsufficientYeetToast` |
| `window.openCashoutModal(maxCredit?)` | ✅ Wired — Cashout button inside Wallet card |
| `window.openChainSendModal({id, name, symbol, decimals, address})` | ✅ Wired — Send button per chain in Other chains modal |
| `window.openNftGallery(addressOrId)` | ❌ NOT WIRED — suggest: profile page button "View NFTs" |
| `window.openTransferNftModal(tokenId)` | ❌ NOT WIRED — suggest: per-NFT action menu on profile gallery |
| `window.showInsufficientYeetToast(amount?)` | ✅ Wired — fires from PPV unlock + DM tip on insufficient-YEET error |
| `window.pickWalletProvider({purpose})` | ✅ Wired — used by NFT mint; reusable for any future on-chain action |

---

## 4. Multichain — what works and what's missing

### Works today (every chain in the list)

The chains: BSC, Ethereum, Polygon, Avalanche, Sonic, Bitcoin, Solana,
Cardano, XRP, Algorand, Tron, Kaspa, Kadena.

- **Address derivation** from the user's seed phrase — `window.dontyeet.getChainAddress(chainId)`
- **Native balance read** via public RPC — `window.dontyeet.chainBalance(chainId, address)`
- **Native send** — sign + broadcast in-browser — `window.dontyeet.chainSend(chainId, to, amountRaw)`
- **Receive view** — the Other chains modal lists every chain with address, balance, Send + Copy buttons

### Not implemented yet

#### Send by `@username` / URL
- Backend: add `users.chain_addresses JSONB` column (map `chain_id → address`)
- On user login: derive every chain address browser-side, POST to a new `/api/v1/users/me/chain-addresses` endpoint
- Add `/api/v1/users/:username/chain-addresses` (auth-required, returns the map)
- In `openChainSendModal`: when "to" is `@username`, resolve via that endpoint
- Effort: ~2 hours

#### Token balances per chain (not just native asset)
- Each chain crate has a `wasm::fetch_balance` for the native coin; token-balance support is not in every chain crate's wasm path
- For EVM tokens specifically, an `ethers::Interface(['function balanceOf(address) view returns (uint256)'])` call works — already used in `_fetchOnChainYeetBalance`
- For non-EVM tokens (SPL on Solana, BEP-20 tokens on BSC, Cardano native assets), each needs its own RPC pattern
- Effort: 1-2 days for complete coverage

#### Transaction history per chain
- HyperHub UI has per-chain history modules; not ported to Yeet's wasm shim
- For now, frontend can link out to the explorer (block-explorer URL helper would help — also missing)
- Effort: ~30 min for explorer-URL helper; ~1 day for in-app history view

#### QR codes on receive addresses
- The Other chains modal shows addresses as text + Copy button
- Add a tiny dep like `qrcode-svg` and render QR per row
- Effort: ~30 min

#### Cardano operator config
- The Cardano plugin needs a config (genesis-related) on the server side
- The wasm path in `hyperhub-chain-cardano::wasm` is self-contained but
  uses public Koios endpoints — verify the operator is OK with that
- Effort: validate / document the default

---

## 5. NFT — stubbed but functional

### Works today

- `mintNFT()` — properly wired with ethers ABI encoding (was previously a placeholder selector)
- Wallet chooser modal: YeetWallet (in-browser) / MetaMask / Trust Wallet / WalletConnect (stubbed)
- YeetWallet path signs and broadcasts directly; MetaMask/Trust use injected provider

### Not implemented yet

- **NFT gallery view per user** — `window.openNftGallery(addressOrId)` is a placeholder modal. Will eventually fetch ERC-721 ownership from BSC and render each post-NFT with embedded media. Needs:
  - Backend or browser-side enumeration of NFTs owned by an address (alchemy/moralis-style API, or direct contract scan via `Transfer` event indexer)
  - Frontend gallery layout
- **NFT transfer** — `window.openTransferNftModal(tokenId)` is a stub. Needs:
  - `safeTransferFrom(from, to, tokenId)` ABI encoding
  - Wallet chooser flow (same as mint)
- **WalletConnect** — modal lists it but immediate toast says "coming soon". Add the WalletConnect SDK + provider integration.
- **NFT marketplace** — buy/sell NFTs at listed prices. Pre-existing `posts.nft_price_yeet` column suggests this was planned. Not implemented.

---

## 6. PPV / DM tips

### Works today

- PPV unlock and DM-attached tips both use `credit_ops::debit_credit_pair` — atomic off-chain credit transfer
- On "Insufficient YEET" response, frontend auto-fires `showInsufficientYeetToast(price/amount)` with a Deposit shortcut

### Not implemented yet

- **PPV refund flow** — if a post is deleted after being unlocked, no refund mechanism exists
- **DM tip reversal** — if recipient blocks sender after a tip, no refund
- **Tip thank-you UX** — recipient sees a notification but no native "thank" action

---

## 7. Authentication flows

### Works today

- Email signup / login with auto-generated YeetWallet
- MetaMask signup — derives deterministic password from MetaMask signature, no extra step for user
- Biometric unlock via WebAuthn PRF on mobile (Touch ID / Face ID / Windows Hello)

### Not implemented yet

- **Trust Wallet / WalletConnect login** — wallet chooser only exists on the NFT mint path, not on the auth modal
- **Recovery via cloud backup** — earlier convo proposed Google Drive `appDataFolder` for encrypted mnemonic backup. Not yet implemented.
- **Account deletion / wallet reset** — exposed via `delete_account` in account manager but no UI to call it
- **Password change** — exposed on `window.dontyeet.changePassword(old, new)` but no UI

---

## 8. Backend services — runtime config

### Services that auto-spawn from `main.rs`

- `batch_rewards` — hourly mint of pending rewards (existing)
- `cleanup` — periodic data cleanup (existing)
- `message_cleanup` — DM retention enforcement (existing)
- `transfer_indexer` — watches BSC for incoming YEET transfers; populates feed notifications + auto-credits deposits to custodian
- `credit_payout` — hourly drain of pending cashout requests

### Caveats

- `transfer_indexer` runs even without `YEET_DEPOSIT_ADDRESS` set; it just skips the deposit branch
- `credit_payout` logs "disabled" and exits if `CUSTODIAN_PRIVKEY` is unset — no cashouts will process
- `batch_rewards` similarly disables if `REWARDS_MINTER_PRIVKEY` unset

---

## 9. Frontend dynamic chain config

- `_BSC_TESTNET_RPC` / `_BSC_TESTNET_CHAIN_ID` / `_YEET_TOKEN_ADDR` constants — all removed
- Frontend reads `bsc_rpc_url`, `chain_id`, `chain_name`, `explorer_url`, `yeet_token_address` from `GET /api/v1/credit/info`
- `_ensureBscChain` (renamed from `_ensureBscTestnet`) prompts MetaMask/Trust to add the configured BSC network with the right RPC and explorer
- **One env-var flip** on the VPS swaps the whole frontend from testnet → mainnet without a redeploy

---

## 10. Known build / dev quirks

- **`wasm-opt` disabled** in `wallet/Cargo.toml` — the version bundled with wasm-pack can't validate the wallet's wasm-bindgen output once all chain crates are linked in. Rust's own `opt-level = 3 + lto = "thin"` still optimises; bundle is ~10-15% bigger than it would be with `wasm-opt`. Re-enable when wasm-pack ships a newer wasm-opt.
- **Path dependencies** in `wallet/Cargo.toml` — point at `../../HyperHUb/crates/...` on the local dev machine. For CI / fresh clones, two options:
  - Vendor the crates into `wallet/vendor/` (full self-contained, larger repo)
  - Add the upstream crates as git submodules
  - For now, contributors need a local clone at the expected sibling path
- **Backend `[profile.release]`** is at the workspace root (was duplicated in `backend/Cargo.toml`; Cargo ignored it there and warned on every build). Single source of truth now.
- **`SQLX_OFFLINE=true`** required for `cargo check -p backend` on machines without a live PostgreSQL — `backend/.sqlx/` holds the cached query metadata.

---

## 11. Build commands cheat-sheet

```bash
# Backend (offline sqlx)
SQLX_OFFLINE=true cargo check -p backend
SQLX_OFFLINE=true cargo build -p backend --release

# Wallet wasm bundle
cd wallet && cargo clippy --target wasm32-unknown-unknown -- -D warnings
wasm-pack build wallet --release --target web --out-dir frontend/wallet/pkg --out-name wallet

# Run everything (docker compose)
docker compose up --build
```

---

## 12. Test coverage

Currently zero automated tests for:

- The new YEET Credit flows (`credit.rs`, `credit_ops.rs`, `credit_payout.rs`) — only the unit tests embedded in each module
- The wasm shim (`wallet/src/lib.rs`) — has a couple of unit tests for helpers (`format_amount`, `yeet_to_wei`)
- The frontend — no JS test infrastructure

Recommend `cargo test -p backend` first to confirm existing test coverage didn't regress, then layer on integration tests for the credit flows.

---

*Generated alongside the `feat/dontyeet-wallet` branch — last touch before the dev wiring session.*
