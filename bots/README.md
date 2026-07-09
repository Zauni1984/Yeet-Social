# Yeet Social — Test Bots

Deterministic wallet bots that exercise the live app: each "day" they log in,
post a fresh message, and interact with each other (like / comment / follow).

## How it works

- Bots are HD-derived from a single BIP-39 seed phrase (`BOT_SEED`), path
  `m/44'/60'/0'/0/<i>`. The same seed → the same persistent accounts every run.
- Each bot authenticates with the **wallet** flow (nonce → `personal_sign` →
  verify). No email, no email verification needed — a wallet upsert creates the
  account on first login.
- After login each bot PATCHes its display name/bio (persona), posts one
  message (~15% are marked **permanent** so the Permanent Posts page gets
  exercised), then likes ~3, comments on ~1, and follows up to 2 *other* bots.

## Run locally

```bash
cd bots
npm install
BASE_URL=https://justyeet.it BOT_COUNT=5 npm start
```

Without `BOT_SEED` set, a built-in default test mnemonic is used (public,
well-known addresses) so it works out of the box. For a real/shared deployment,
set your own seed.

## Daily automation (GitHub Actions)

`.github/workflows/bots.yml` runs the bots every day at 09:13 UTC and can also
be triggered manually (Actions → "Test Bots — daily activity" → Run workflow).

### Configuration

| Setting | Where | Default |
| --- | --- | --- |
| `BOT_SEED` | repo **secret** | built-in test mnemonic |
| `BOT_BASE_URL` | repo **variable** | `https://justyeet.it` |
| `BOT_COUNT` | workflow input / env | `5` (max 25) |

To set a private, stable bot identity set:
**Settings → Secrets and variables → Actions → New repository secret** →
name `BOT_SEED`, value a 12/24-word BIP-39 mnemonic.

## Notes

- `BOT_COUNT` is capped at 25 (the number of built-in personas).
- The script is dependency-light: only `ethers` v6 + Node 18+ global `fetch`.
- It is safe to re-run; profile updates are idempotent and follows use
  `ON CONFLICT DO NOTHING` server-side.
