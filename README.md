# YEET Social  Web3 Social Media Platform

> Live at **[justyeet.it](https://justyeet.it)**

YEET Social is a Web3-native social media platform where users can post, comment, like, and tip each other with YEET tokens. Built with a Rust backend, PostgreSQL, Redis, and a single-file frontend served via nginx.

---

## Current Status (April 2026)

- **Live & working**  posts, comments, likes, tipping icon all functional
- **Email login/registration**  fully working
- **Wallet login**  MetaMask & WalletConnect UI present
- **Feed**  global feed with FOR YOU / FOLLOWING / NFT / 18+ tabs
- **Post composer**  text posts with character counter
- **Comments**  collapsible comment threads per post
- **Tipping**  tip button visible on posts (YEET token flow in development)
- **Mobile responsive**  hamburger menu, mobile auth button, responsive layout

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Backend | Rust (Axum) |
| Database | PostgreSQL 16 |
| Cache | Redis 7 |
| Frontend | Single `index.html` (vanilla JS) |
| Web server | Nginx (Docker) |
| Hosting | Hostinger VPS (Ubuntu 24.04) |
| Container registry | `ghcr.io/zauni1984/yeet-social/backend:main` |

---

## Infrastructure

All services run as Docker containers in the `yeet-social_yeet-net` network:

- `yeet-nginx`  serves frontend + proxies `/api/` to backend
- `yeet-backend`  Rust API on port 8080
- `yeet-postgres`  PostgreSQL database
- `yeet-redis`  Redis cache

Persistent config files:
- `/root/nginx.conf`  nginx config (survives reboots)
- `/root/nginx.conf`  nginx config with SSL, API proxy
- `/root/start_backend.sh`  backend start script with env vars
- `/root/yeet-html/index.html`  frontend (live)
- `/root/yeet-html/index_good.html`  last known good backup

---

## Environment Variables (Backend)

```
DATABASE_URL=postgres://yeet:<password>@yeet-postgres:5432/yeet
REDIS_URL=redis://yeet-redis:6379
JWT_SECRET=<min 32 chars>
```

---

## Roadmap

- [ ] Tipping flow  YEET token transfer on-chain
- [ ] Wallet login (MetaMask / WalletConnect) fully wired
- [ ] NFT post type
- [ ] 18+ content gate
- [ ] CD pipeline (auto-deploy on push)
- [ ] YEET token smart contract (BNB Chain)

---

## Repository Structure

```
/
 backend/          # Rust API (Axum)
    src/
        main.rs
        email_auth.rs
        posts.rs
        feed.rs
        ...
 frontend/
     index.html    # Entire frontend in one file
```

---

*Built by Stefan Zauni  Ostern 2026*
