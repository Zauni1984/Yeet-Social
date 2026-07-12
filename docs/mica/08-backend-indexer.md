# 08 — Backend-Indexer: Off-Chain-Buchung → On-Chain-Event-Indexing

**Status: ENTWURF.** Setzt die non-custodial Zahlungen (Doc 05–07) im Backend um:
Der Server **bucht nichts mehr selbst**, sondern **liest bestätigte Contract-Events**
und aktualisiert daraus die App-Zustände (PPV-Freischaltung, Zähler, Gutschein-Status).

Migration: `backend/migrations/0037_onchain_indexer.sql`.

---

## 1. Warum ein Indexer statt Off-Chain-Buchung

Heute schreibt `tips.rs`/PPV-`unlock`/`paper_wallets.rs` direkt in die DB und bewegt
`users.yeet_token_balance`. Das ist genau die custody-nahe Konstruktion (Doc 06, §4).
Im Zielmodell laufen Werttransfers **on-chain** über `YeetPayments`/`PaperWalletEscrow`.
Der Server ist nur noch **Beobachter**: Er indexiert Events und spiegelt sie in App-State.

## 2. Architektur

```
BSC RPC ──logs──> Indexer-Loop (pro Contract)
                    │  filter: address + topics, fromBlock..toBlock-CONF
                    │  je Log: dedup (tx_hash, log_index) → apply → cursor++
                    ▼
                 Postgres: onchain_events (dedup) + ppv_unlocks/onchain_tips + counters
```

- **Poll-basiert** (HTTP `eth_getLogs`), robuster als WS für einen Backend-Job.
  Intervall ~5–10 s.
- **Konfirmationen:** nur Blöcke bis `head - CONFIRMATIONS` (BSC: z. B. 15)
  verarbeiten → Reorg-Schutz.
- **Idempotenz:** `onchain_events(tx_hash, log_index)` PK; jede Anwendung ist ein
  `INSERT ... ON CONFLICT DO NOTHING` + Fachlogik nur bei „inserted".
- **Cursor:** `indexer_cursors.last_block` pro Contract; Neustart setzt dort auf.
- **Reorg:** Da wir nur finalisierte Blöcke anfassen, genügt der Confirmations-Puffer;
  optional zusätzlich die letzten N Blöcke bei jedem Lauf re-scannen (ON CONFLICT
  macht das gefahrlos).

## 3. Events → Wirkung

| Event | Quelle | Wirkung im Backend |
| --- | --- | --- |
| `Paid(kind=Tip, payer, recipient, ref, gross, fee, net)` | YeetPayments | `onchain_tips` insert; Tip-Zähler am Post `ref` erhöhen; Notification an `recipient` |
| `Paid(kind=PayPerView, payer, recipient, ref, …)` | YeetPayments | `ppv_unlocks` (post=`ref`, user=payer-Wallet→user_id, source='onchain', tx_hash) upsert → Inhalt freigeschaltet |
| `Paid(kind=Promotion, …)` | YeetPayments | Promotion/Boost für `ref` aktivieren |
| `VoucherCreated/Claimed/Refunded` | PaperWalletEscrow | Gutschein-Status spiegeln (ausgestellt/eingelöst/erstattet) |

**Wallet→user_id-Auflösung:** Über `users.wallet_address`. Zahlt eine Wallet, die zu
keinem Account gehört, wird das Event trotzdem für Analytics gespeichert, aber ohne
App-State-Änderung (bzw. PPV wird an der Empfänger-/Zahler-Wallet festgemacht — je nach
UX-Entscheidung). TODO(strategie).

## 4. Rust-Skeleton (nicht eingebunden — Contracts noch nicht deployed)

> Als Datei-Skelett gedacht: `backend/src/services/indexer.rs`. Erst aktivieren, wenn
> `YEET_PAYMENTS_ADDRESS`/`YEET_PAPER_ESCROW_ADDRESS` gesetzt sind, dann in `main.rs`
> `tokio::spawn(services::indexer::run(state.clone()))` ergänzen (analog zu den
> batch-Jobs).

```rust
// backend/src/services/indexer.rs  (SKELETON — needs contract addrs + wiring)
use std::time::Duration;
use tokio::time::interval;
use ethers::prelude::*;
use crate::AppState;

const CONFIRMATIONS: u64 = 15;      // BSC reorg buffer
const MAX_RANGE: u64 = 2_000;       // max blocks per eth_getLogs page

pub async fn run(state: AppState) {
    // Skip cleanly if not configured yet.
    let payments = match std::env::var("YEET_PAYMENTS_ADDRESS").ok()
        .and_then(|s| s.parse::<Address>().ok()) {
        Some(a) if a != Address::zero() => a,
        _ => { tracing::info!("indexer: YEET_PAYMENTS_ADDRESS unset — indexer idle"); return; }
    };
    let rpc = std::env::var("BSC_RPC_URL")
        .unwrap_or_else(|_| "https://bsc-dataseed.binance.org/".into());
    let provider = Provider::<Http>::try_from(rpc).expect("rpc");
    let chain_id: i64 = std::env::var("YEET_CHAIN_ID").ok()
        .and_then(|s| s.parse().ok()).unwrap_or(56);

    // event signature topic0 for Paid(uint8,address,address,bytes32,uint256,uint256,uint256)
    let paid_topic = H256::from(ethers::utils::keccak256(
        b"Paid(uint8,address,address,bytes32,uint256,uint256,uint256)"));

    let mut ticker = interval(Duration::from_secs(8));
    loop {
        ticker.tick().await;
        if let Err(e) = tick(&state, &provider, payments, paid_topic, chain_id).await {
            tracing::warn!("indexer tick: {e}");
        }
    }
}

async fn tick(
    state: &AppState, provider: &Provider<Http>,
    payments: Address, paid_topic: H256, chain_id: i64,
) -> anyhow::Result<()> {
    let head = provider.get_block_number().await?.as_u64();
    let safe_head = head.saturating_sub(CONFIRMATIONS);

    // Load cursor (or start near head on first run to avoid a huge backfill).
    let from: i64 = sqlx::query_scalar(
        "SELECT last_block FROM indexer_cursors WHERE contract = 'payments'")
        .fetch_optional(state.db.pool()).await?
        .unwrap_or_else(|| safe_head as i64);
    let mut from = from as u64 + 1;
    if from > safe_head { return Ok(()); }

    let to = (from + MAX_RANGE).min(safe_head);
    let filter = Filter::new()
        .address(payments).topic0(paid_topic)
        .from_block(from).to_block(to);
    let logs = provider.get_logs(&filter).await?;

    for log in logs {
        let tx = format!("{:?}", log.transaction_hash.unwrap_or_default());
        let li = log.log_index.unwrap_or_default().as_u64() as i32;
        // Dedup: only act if this (tx, log_index) is new.
        let inserted = sqlx::query(
            "INSERT INTO onchain_events (tx_hash, log_index, block_number, contract, event, payload)
             VALUES ($1,$2,$3,'payments','Paid','{}'::jsonb)
             ON CONFLICT DO NOTHING")
            .bind(&tx).bind(li)
            .bind(log.block_number.unwrap_or_default().as_u64() as i64)
            .execute(state.db.pool()).await?;
        if inserted.rows_affected() == 0 { continue; }

        // decode: topics[1]=kind, topics[2]=payer, topics[3]=recipient; data = ref,gross,fee,net
        // apply_paid(state, kind, payer, recipient, ref, net, &tx).await?;   // TODO
    }

    sqlx::query(
        "INSERT INTO indexer_cursors (contract, chain_id, last_block)
         VALUES ('payments', $1, $2)
         ON CONFLICT (contract) DO UPDATE SET last_block = EXCLUDED.last_block, updated_at = NOW()")
        .bind(chain_id).bind(to as i64)
        .execute(state.db.pool()).await?;
    Ok(())
}
```

## 5. Umbau der bestehenden Endpoints

| Alt (custodial) | Neu (non-custodial) |
| --- | --- |
| `POST /api/v1/tips` bucht `yeet_token_balance` | Client sendet On-Chain-Tx via `YeetPayments.pay`; Endpoint entfällt oder wird auf „Tx-Hash-Quittung" reduziert (nur Anzeige, keine Buchung) |
| PPV-`unlock` schreibt DB-Guthaben ab | Freischaltung kommt aus `Paid(PayPerView)`-Event |
| `paper_wallets.rs` interner Ledger | `PaperWalletEscrow` + Event-Spiegelung |
| `users.yeet_token_balance` | → **Punkte** umwidmen (Doc 05); Auszahlung Punkte→YEET über bestehenden Batch-Mint |

**Wichtig:** Der Umbau erst NACH Deploy + Audit der Contracts. Bis dahin bleibt der
Ist-Zustand eingefroren (kein Verkauf/Listing — Checkliste B).

## 6. Betrieb

- Ein zusätzlicher `tokio::spawn` neben den bestehenden Jobs (batch-rewards, cleanup).
- Metriken/Logs: last_block-Lag, Events/Minute, Fehlerrate.
- Bei RPC-Ausfall: Backoff, kein Cursor-Fortschritt → sicheres Re-Scan.
- Kein privater Schlüssel im Indexer (read-only).
