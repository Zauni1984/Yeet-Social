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
BOT_SEED="<your 12/24-word phrase>" BASE_URL=https://justyeet.it BOT_COUNT=5 npm start
```

`BOT_SEED` is required; without it the script exits. There is no built-in
default any more: the old one was the published Hardhat test phrase, so anyone
could sign in as the bots. Use a phrase generated only for the bots, never a
published test phrase and never a wallet that holds funds.

## Daily automation (GitHub Actions)

`.github/workflows/bots.yml` runs the bots every day at 09:13 UTC and can also
be triggered manually (Actions → "Test Bots — daily activity" → Run workflow).

### Configuration

| Setting | Where | Default |
| --- | --- | --- |
| `BOT_SEED` | repo **secret** | none (required; the run fails without it) |
| `BOT_BASE_URL` | repo **variable** | `https://justyeet.it` |
| `BOT_COUNT` | workflow input / env | `5` (max 25) |

Set the seed once:
**Settings → Secrets and variables → Actions → New repository secret** →
name `BOT_SEED`, value a 12/24-word BIP-39 mnemonic.

## Notes

- `BOT_COUNT` is capped at 25 (the number of built-in personas).
- The script is dependency-light: only `ethers` v6 + Node 18+ global `fetch`.
- It is safe to re-run; profile updates are idempotent and follows use
  `ON CONFLICT DO NOTHING` server-side.
