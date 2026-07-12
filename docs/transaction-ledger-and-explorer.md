# Transaction Ledger + YEET Token Explorer

Two subsystems: (1) an internal, tamper-evident **transaction ledger** (evidence
+ tax export, admin-only) and (2) a public **on-chain token explorer** with a
richlist and an API for third parties.

---

## 1. Transaction ledger

### What it records
Every value movement is appended to `ledger_entries` (migration 0039). Wired in
at each money-flow site:

| Event | tx_type | asset |
| --- | --- | --- |
| Engagement points earned | `reward_grant` | POINTS |
| Points tip (sender / creator / platform) | `tip_sent` / `tip_received` / `platform_fee` | POINTS |
| PPV purchase (flows through the tip path) | `tip_sent`/`tip_received`/`platform_fee` | POINTS |
| Paper wallet issue / claim / void | `paper_wallet_issue` / `_claim` / `_refund` | POINTS |
| Points → YEET conversion (debit) | `points_conversion` | POINTS |
| On-chain payout of a conversion | `onchain_payout` | YEET |
| On-chain tips / PPV (via indexer, once live) | `onchain_tip` / `onchain_ppv` | YEET |

`amount` is signed from the subject's perspective (credit +, debit −).

### Why it's evidence (Nachweis) — GoBD-friendly
- **Append-only**: DB triggers block `UPDATE`/`DELETE` on `ledger_entries`.
  Corrections are made by appending a reversing entry, never by editing history.
- **Gapless `entry_no`**: assigned under a transaction advisory lock, so the
  sequence is 1,2,3,… with no holes (a rollback doesn't burn a number).
- **Hash chain**: `entry_hash = sha256(canonical_content || prev_hash)`. Changing
  any historical row changes its hash and breaks every subsequent hash — so
  tampering is detectable. `GET /admin/ledger/verify` recomputes the whole chain
  and reports the first broken entry (or "intact").
- **Atomicity**: ledger writes join the same DB transaction as the balance move
  (`record_in_tx`), so a movement can never exist without its ledger entry.
- **Tax valuation columns**: `fiat_currency`, `fiat_value`, `fx_rate`, `fx_source`
  are present for the value-at-time-of-transaction that a Finanzamt export needs
  (nullable until a market price exists).

### Admin API (backend-only, `?secret=<ADMIN_SECRET>`)
- `GET /api/v1/admin/ledger` — filtered, paginated JSON
  (`from`,`to`,`tx_type`,`asset`,`user_id`,`page`,`per_page`).
- `GET /api/v1/admin/ledger/export` — **CSV** (semicolon-delimited + UTF-8 BOM →
  opens cleanly in German Excel / DATEV). Same filters. Exact decimal strings.
- `GET /api/v1/admin/ledger/summary` — aggregates per `tx_type`×`asset`
  (entries, total_credit, total_debit, net, total_fee) for the period.
- `GET /api/v1/admin/ledger/verify` — hash-chain integrity check.

Access is only through the backend and gated on `ADMIN_SECRET` (same mechanism
as the moderation admin API). **Set a strong `ADMIN_SECRET` in production** — the
default is a placeholder.

### Notes
- PPV is captured via the tip path (it reuses `send_tip_tx`), so it's in the
  ledger even though its `tx_type` reads as a tip with a post reference.
- On-chain payout entries are best-effort *after* the mint settles (they carry
  the `onchain_tx_hash`); a ledger hiccup never rolls back a settled on-chain tx.

---

## 2. YEET token explorer (public)

### Data (migration 0040)
- `token_holders (address, balance, tx_count, …)` — richlist source of truth.
- `token_transfers (tx_hash, log_index, from, to, value, block_number, …)`.
- `token_stats` — singleton cache (holder/transfer counts, circulating supply,
  last indexed block).

### Public API (read-only, for third-party providers)
- `GET /api/v1/explorer/token` — name/symbol/decimals/chain/contract + stats.
- `GET /api/v1/explorer/richlist?limit=&offset=` — top holders by balance.
- `GET /api/v1/explorer/holders/:address` — balance, tx_count, rank.
- `GET /api/v1/explorer/transfers?address=&limit=&before_block=` — transfer feed.
- `GET /api/v1/explorer/tx/:hash` — transfers in one transaction.

All values are wei strings (exact). No auth (public). Consider adding an API key
+ rate limit if you expose it widely.

### Enabling it (after the token is deployed)
The tables are empty until an indexer fills them. The indexer scans the YEET
ERC-20 `Transfer(address,address,uint256)` events and, per event:
- upsert `token_transfers` (dedup on `tx_hash+log_index`);
- apply the balance delta to `from`/`to` in `token_holders` (mint = from 0x0,
  burn = to 0x0); bump `tx_count`, `last_active`;
- refresh `token_stats`.

This mirrors the payments indexer (docs/mica/08): poll `eth_getLogs` with a
confirmations buffer, a per-contract cursor, and reorg-safe dedup. Wire it in
`main.rs` (`tokio::spawn(services::token_indexer::run(state))`) once
`YEET_TOKEN_ADDRESS` is set. Skeleton:

```rust
// backend/src/services/token_indexer.rs (SKELETON — enable after deploy)
// filter: address=YEET_TOKEN_ADDRESS, topic0 = keccak("Transfer(address,address,uint256)")
// for each log in confirmed range:
//   from = topic1, to = topic2, value = data
//   INSERT token_transfers ... ON CONFLICT DO NOTHING  (only act if inserted)
//   UPDATE token_holders SET balance = balance - value WHERE address = from
//   INSERT token_holders(address,balance) VALUES(to,value)
//     ON CONFLICT(address) DO UPDATE SET balance = token_holders.balance + value
//   (skip the zero address for mint/burn on the respective side)
// then bump token_stats + cursor.
```

Until then, the explorer endpoints return empty lists / zero stats — safe to
expose immediately; they light up when indexing starts.
