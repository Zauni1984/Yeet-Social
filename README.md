# YEET Social — Web3 Social Media Platform

> Live at **[justyeet.it](https://justyeet.it)** · Source-available under the [Elastic License 2.0](./LICENSE) (contracts: MIT)

YEET Social is a Web3-native social network: post, comment, like and tip each other; earn points that convert into the YEET utility token (BEP-20 on BNB Chain, contract pending). Rust backend, PostgreSQL, Redis, and a single-file frontend served by nginx.

---

## Current status (September 2026)

**Live on justyeet.it**

- Posts (text, image, video, permanent posts, pay-per-view), comments, likes, reposts, tips, trending
- **Audio Stories** — record in the browser, 24 h or permanent, optional 18+
- Email login (Argon2id, GDPR double opt-in) and MetaMask login; email users can link a wallet later
- Points economy: registration bonus (first 100k), posting reward (≥120 chars, daily cap), conversion queue with **manual admin approval**, hash-chained ledger, public explorer
- **Eighteen UI languages** (EN, DE, IT, FR, ES, PT, FI, SV, NB, IS, CS, DA, NL, PL, HR, SR, TR, LV) with browser auto-detect
- **Accessibility**: screen-reader mode, high contrast, large text, keyboard shortcuts, built-in audio reader
- **Post translation** (provider-neutral: Azure / Google / DeepL / LibreTranslate) and **feed filter** by language and country
- Moods (color themes), user-composable Webboards (RSS), direct messages (E2EE), paper wallets, 18+ gate with age verification
- Changelog bot **@yeet_updates** posts every shipped change as a permanent post
- CI (build, clippy, tests) → CD (Docker image + VPS deploy) on every merge to `main`

**Prepared, switched off until the token contract exists**

- NOTE → YEET swap page (`/swap.html`, 100 NOTE = 1 YEET, cap 500 M YEET) — see [`docs/swap-note-to-yeet.md`](./docs/swap-note-to-yeet.md)
- On-chain payout of approved conversions (batch minter)

**Parked**: live streaming (built end-to-end, see [`README_LIVEKIT.md`](./README_LIVEKIT.md)).

---

## Tech stack

| Layer | Technology |
|-------|-----------|
| Backend | Rust (Axum 0.7, sqlx) |
| Database | PostgreSQL 16 (migrations in `backend/migrations/`) |
| Cache / rate limits | Redis 7 |
| Frontend | Single `frontend/index.html` (vanilla JS), `admin.html`, `swap.html` |
| Web server | nginx (Docker) |
| Hosting | Hostinger VPS (Ubuntu 24.04), Docker Compose |
| Registry | `ghcr.io/zauni1984/yeet-social/backend:main` |

---

## Repository structure

```
backend/        Rust API (src/api = handlers, src/services = jobs & integrations, migrations/)
backend/changelog.json   release notes posted by the changelog bot
frontend/       index.html (app), admin.html (admin dashboard), swap.html (NOTE→YEET)
contracts/      Solidity (MIT)
docs/           feature docs: translation, feed filter, audio stories, changelog bot, swap, ledger/explorer
docs/mica/      MiCA dossier: readiness, whitepaper draft, chain assessment, compliance checklist, architecture
vps/            deploy scripts, .env template, DEPLOY.md
```

---

## Configuration

See [`.env.example`](./.env.example) (local) and [`vps/.env.example`](./vps/.env.example) / [`vps/DEPLOY.md`](./vps/DEPLOY.md) (production). Feature switches worth knowing: `TRANSLATE_PROVIDER` (post translation), `SWAP_ENABLED` (NOTE swap), `CHANGELOG_BOT_ENABLED`, `YEET_CONVERSION_POOL` / taper settings (points economy).

---

## Contributing conventions

- Every PR with a user-visible change adds an entry to `backend/changelog.json` (English, ≤ 420 chars incl. hashtags; `cargo test changelog` enforces it). The bot posts it after deploy.
- New UI strings go into every language block of `DICTS` in `frontend/index.html`; key parity is checked headlessly.
- Backend: `cargo clippy --all-targets -- -D warnings` and `cargo test` must pass (CI runs both against a live Postgres).

---

## Roadmap (external / decisions)

- [ ] YEET token smart contract on BNB Chain + audit → enables on-chain payouts and the NOTE swap
- [ ] Note full node for the swap (fork of note-llc/NoteBlockchain if needed)
- [ ] Translation provider key on the VPS (Azure AI Translator F0 recommended)
- [ ] MiCA legal review, trademark, KYC provider
- [ ] Mobile release

---

*Built by Stefan Zauni · BlockSocial UG (haftungsbeschränkt), Beilngries*
