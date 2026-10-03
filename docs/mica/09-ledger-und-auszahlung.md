# 09 — Off-Chain-Ledger ↔ On-Chain-Token: Anspruch, Umtausch, Auszahlung

**Status: ENTWURF (Dev-Fassung, aus dem Code abgeleitet; Anwaltsprüfung offen).**
Erfüllt die Checklisten-Zeile „Dokumentation Off-Chain-Ledger ↔ On-Chain-Token
(Anspruch, Umtausch, Auszahlung)" aus [04-compliance-checkliste.md](04-compliance-checkliste.md) §B.

> ⚠️ **Kein Rechtsrat.** Dieses Dokument beschreibt, *was das System tatsächlich tut*
> (Stand Migration 0051), damit die Kanzlei die rechtliche Einordnung an den echten
> Abläufen vornehmen kann. Rechtliche Aussagen sind als Arbeitshypothesen markiert
> und in §10 als Prüfpunkte für den Anwalt gesammelt.

Lesereihenfolge: Modell in [05](05-zielarchitektur-non-custodial.md), Leitplanken in
[06](06-leitplanken-validierung.md), dieses Dokument = konkrete Buchungs- und
Auszahlungsmechanik. Technische Details zum Ledger: `docs/transaction-ledger-and-explorer.md`;
Sanktions-Screening: `docs/sanktions-screening.md`.

---

## 1. Zwei getrennte Buchführungsebenen

| | **Off-Chain: Punkte** | **On-Chain: YEET-Token** |
| --- | --- | --- |
| Wo | Postgres, `users.yeet_token_balance` (Saldo) + `ledger_entries` (Journal) | BNB Smart Chain (Chain-ID 56), Contract `YeetToken` (BEP-20) |
| Was | Interne Plattform-Punkte, **kein Kryptowert**, keine DLT | Kryptowert i. S. v. MiCA Art. 3 Abs. 1 Nr. 5 (Einstufung: Doc 01) |
| Wer hält | Die Plattform führt den Saldo; der Nutzer hat ein **Nutzungs-/Leistungsrecht** gegen BlockSocial UG (siehe §3) | **Ausschließlich der Nutzer** in seiner Self-Custody-Wallet. Die Plattform hält zu keinem Zeitpunkt Nutzer-Token |
| Entsteht durch | Engagement-Rewards, Registrierungsbonus, empfangene Tips/PPV-Erlöse, Gutschein-Einlösung, Rückbuchungen (§4) | Ausschließlich durch **Mint** im Zuge einer vom Nutzer angestoßenen Umwandlung (§6–7) |
| Verwendung | Tips, Pay-per-View, Paper-Wallet-Gutscheine, Live-Promotions innerhalb der App (§5) | Freie Verfügung des Nutzers on-chain; künftig On-Chain-Tips Wallet↔Wallet (F1/F2, Doc 07) |
| Rückweg | — | **Keiner.** Es gibt keinen Endpoint, der YEET entgegennimmt und Punkte gutschreibt (Leitplanke L2) |
| Käuflich | **Nein** (L1). Kein Fiat-, kein Krypto-Kauf von Punkten | Kein Verkauf durch die Plattform (Status quo eingefroren, Checkliste §B) |
| Übertragbar | Nein, Konto und Punkteguthaben sind personengebunden (AGB §3) | Ja, frei (ERC-20-Semantik), außerhalb der Verantwortung der Plattform |

Die **einzige Brücke** zwischen den Ebenen ist die Einbahnstraße
`POST /api/v1/points/convert` → Admin-Freigabe → `batchMintRewards` (§6–7).

---

## 2. Begriffe

- **Punkte** — der Wert in `users.yeet_token_balance`. Historisch hieß die Spalte „Token-Balance"; seit Migration 0038 ist sie semantisch **Punkte** (Doc 05 §4.1). In der UI heißt es „Punkte", nicht „YEET".
- **YEET** — der On-Chain-Token (`contracts/src/YeetToken.sol`, 18 Dezimalstellen).
- **Umwandlung / Conversion** — die vom Nutzer ausgelöste Abbuchung von Punkten mit dem Ziel, YEET an seine verknüpfte externe Wallet zu erhalten. Verhältnis derzeit **1 Punkt = 1 YEET**; seit 3. Okt. 2026 **versioniert** in der Tabelle `conversion_rates` (gültig-ab-Datum, Ankündigungszeitpunkt, Notiz) statt als Code-Konstante — Details §6.3.
- **Auszahlung (Payout)** — die technische Erfüllung der Umwandlung: eine Zeile in `token_rewards` mit `kind='conversion'`, die der Batch-Minter on-chain mintet.
- **Conversion-Pool** — rechnerische Obergrenze aller Umwandlungen (`services/tokens.rs::pool_status`), siehe §6.3.
- **Ledger** — das append-only Journal `ledger_entries` (Migration 0039), in dem jede Punkte- und YEET-Bewegung hash-verkettet protokolliert wird (§8).

---

## 3. Anspruch: Was ein Punkteguthaben rechtlich ist (Arbeitshypothese)

**Technische Tatsachen, an denen die Einordnung hängt:**

1. Punkte werden **nie gegen Zahlung ausgegeben** (kein Kaufendpoint, kein Fiat-Ramp, kein Krypto-Eingang). Sie entstehen nur durch die in §4 genannten Vorgänge.
2. Punkte haben **keinen Geldwert** und werden **nicht in Geld ausgezahlt**. Der einzige „Ausgang" ist die Umwandlung in YEET auf die eigene Wallet.
3. Die Umwandlung ist **kein garantierter Anspruch auf eine bestimmte Menge YEET zu einem bestimmten Zeitpunkt**:
   - sie steht unter **Pool-Vorbehalt** (`CONVERSION_POOL_EXHAUSTED`, §6.4),
   - unter **Admin-Freigabe** (`awaiting_approval`, §6.2),
   - unter **Sanktions-Screening** (`SANCTIONED_ADDRESS`, F6),
   - unter dem Vorbehalt eines **änderbaren Umwandlungsverhältnisses** (L7, nur prospektiv),
   - und sie setzt eine **verifizierte externe Wallet** voraus (`NO_WALLET_LINKED`).
   Wird eine Umwandlung abgelehnt oder scheitert sie endgültig, werden die **Punkte zurückgebucht** (`payout_refund`) — der Nutzer verliert nichts, erhält aber auch keinen Ersatz in Geld.
4. Punkte und Konto sind **nicht übertragbar** (AGB §3 „Non-transferable" / „Nicht übertragbar", DE/EN). Ein Sekundärmarkt für Punkte ist vertraglich ausgeschlossen.
5. Die Plattform hält **keine Token für Nutzer**: Vor dem Mint existieren die YEET nicht (es wird neu gemintet, nicht aus einem Bestand übertragen); nach dem Mint liegen sie in der Nutzer-Wallet.

**Arbeitshypothese (Doc 05 §2, Doc 06 §1–2):** Punkte sind ein geschlossenes, nicht
käufliches, nicht übertragbares Bonus-/Treuesystem → kein Kryptowert, kein E-Geld
(EMD2/ZAG), keine Verwahrung oder Transferdienstleistung i. S. v. MiCA Art. 3 Abs. 1 Nr. 16.
Der Nutzer hat gegen BlockSocial UG einen **vertraglichen Leistungsanspruch** auf die in
den AGB beschriebenen Nutzungen (Tips, PPV, Gutscheine) und auf die Umwandlung
*nach Maßgabe der AGB und der genannten Vorbehalte* — keine Geldforderung.

**⚠️ Inkonsistenz im AGB-Text (behoben am 2. Okt. 2026, Anwaltsprüfung offen):** AGB §6 (EN und DE) sagte
bis dahin: *„Off-chain balances reflect a custodial liability of BlockSocial UG"* /
„… können off-chain (über unseren internen Ledger) oder on-chain … durchgeführt werden".
Diese Formulierung stammt aus der Zeit vor dem Punkte-Modell und beschreibt genau die
**custody-nahe Konstruktion, die Doc 01 §4 als Kernrisiko identifiziert**. Sie widerspricht
§3 derselben AGB („Points have no cash value"). §1 und §6 sind inzwischen auf das Punkte-Modell
umgeschrieben (Punkte = Plattform-Punkte ohne Geldwert; YEET nur on-chain in der Nutzer-Wallet;
Umwandlung einseitig, unter Vorbehalt) — Details und Prüfbitte in `docs/rechtstexte.md`. Der
neue Wortlaut ist eine **Rechtstext-Änderung**; die Freigabe durch den Anwalt steht aus (A1).

---

## 4. Wie Punkte entstehen (alle Gutschriftspfade)

Jeder Pfad schreibt `users.yeet_token_balance` **und** in derselben DB-Transaktion einen
Ledger-Eintrag (Ausnahme: Live-Promotion-Refund, siehe Tabelle). Neue Gutschriftspfade
müssen laut L1+ (Doc 06) einen Compliance-Check durchlaufen, bevor sie live gehen.

| Pfad | Auslöser | Betrag | Ledger `tx_type` | Code |
| --- | --- | --- | --- | --- |
| Engagement-Reward | Artikel ≥ 120 Zeichen (10 P), Like (1), Reshare (2), Kommentar (1), Daily-Login (2), NFT-Mint (10) | Tageskappe **1 000 P/Nutzer** (`YEET_DAILY_POINTS_CAP`); bei niedrigem Pool (< 10 % Rest) **Taper** ×0,5 | `reward_grant` | `services/tokens.rs::grant_reward` |
| Registrierungsbonus | E-Mail-Double-Opt-in **und** Alters-/KYC-Verifizierung abgeschlossen | **1 000 P**, einmalig je Identität, nur für die ersten 100 000 Nutzer (`YEET_REGISTRATION_BONUS*`) | `registration_bonus` | `services/tokens.rs::maybe_grant_registration_bonus` |
| Tip empfangen | Anderer Nutzer tippt in Punkten | 90 % des Tips (10 % Plattformgebühr → `fee_ledger`) | `tip_received` (+ `platform_fee`) | `api/tips.rs` |
| PPV-Erlös | Käufer schaltet Post frei | 90 % des Preises | `ppv_earning` (Tip-Pfad mit `TipKind::PayPerView`; `ppv_unlocks.tip_id` verweist auf den Tip) | `api/posts.rs::unlock_post` → `api/tips.rs::send_tip_tx` |
| Gutschein eingelöst | Paper-Wallet-Claim-Code eingelöst | gesperrter Betrag | `paper_wallet_claim` | `api/paper_wallets.rs` |
| Gutschein storniert | Aussteller annulliert uneingelösten Gutschein | gesperrter Betrag zurück | `paper_wallet_refund` | `api/paper_wallets.rs` |
| Auszahlung abgelehnt | Admin lehnt Umwandlung ab (oder Mint endgültig gescheitert + Admin-Reject) | abgebuchte Punkte zurück | `payout_refund` | `api/payouts.rs::admin_reject` |
| Live-Promotion erstattet | Live abgesagt / nie gestartet | Promotion-Preis zurück (+ negative `fee_ledger`-Zeile) | `live_promotion_refund` | `api/lives.rs::refund_promotion_in_tx` (auch vom Sweep-Job genutzt) |

Zur Historie: Migration 0038 hat alle damals noch nicht gemintet Engagement-Rewards
einmalig in Punkte überführt (`status='folded'`) und das automatische On-Chain-Minting von
Rewards abgeschaltet. Seitdem wird **nur noch auf ausdrückliche Umwandlung** gemintet.

---

## 5. Wie Punkte verwendet werden (In-App)

| Verwendung | Abbuchung | Gegenbuchung | Gebühr | Ledger |
| --- | --- | --- | --- | --- |
| Tip an Creator | Sender −X | Creator +0,9 X | 0,1 X → `fee_ledger` (Quelle `tip`) | `tip_sent` / `tip_received` / `platform_fee` |
| Pay-per-View | Käufer −Preis (nur nach F7-Consent, `ppv_unlocks` mit `consent_version/at/lang`) | Autor +0,9 Preis | 0,1 Preis → `fee_ledger` (`ppv`) | `ppv_purchase` / `ppv_earning` / `platform_fee` (seit 3. Okt. 2026; ältere Einträge als `tip_*`) |
| Paper-Wallet-Gutschein | Aussteller −Betrag (gesperrt) | Einlöser +Betrag bei Claim | — | `paper_wallet_issue` / `_claim` / `_refund` |
| Live-Promotion | Nutzer −Preis | — (Plattformleistung) | 100 % → `fee_ledger` (`live_promo`), fließt dem Pool zu | `live_promotion` (Erstattung: `live_promotion_refund`) |

Die **Plattformgebühr** wird nicht als Punkte einem Konto gutgeschrieben, sondern in
`fee_ledger` erfasst und fließt rechnerisch dem Conversion-Pool zu (§6.3). Sie erhöht also
die Menge YEET, die insgesamt umgewandelt werden kann, verlässt aber nie das System als
Guthaben einer Person. (`TODO(verify)` Anwalt/Steuer: Behandlung der Gebühr, Doc 04 §C
„Interessenkonflikt-Disclosure 10 %".)

---

## 6. Umtausch (Conversion): Punkte → YEET

### 6.1 Voraussetzungen

| Bedingung | Prüfung | Fehlercode an den Client |
| --- | --- | --- |
| Mindestmenge | `points ≥ 100` (ganze Punkte) | 422 „Minimum conversion is 100 points" |
| Verknüpfte externe Wallet | `users.wallet_address` gesetzt — nur über den Signatur-Challenge-Flow (`email_auth::link_wallet_verify`, MetaMask/WalletConnect), nie plattformgeneriert | 403 `NO_WALLET_LINKED` |
| Sanktionsliste (F6) | `sanctions::check(wallet) == Sanctioned` | 403 `SANCTIONED_ADDRESS` |
| Deckung | Saldo ≥ Punkte | 422 „Insufficient points" |
| Pool | Punkte ≤ `pool.remaining` | 403 `CONVERSION_POOL_EXHAUSTED` |

Alle Prüfungen laufen in **einer** DB-Transaktion unter Advisory-Lock (`YEETCNV`), so dass
zwei gleichzeitige Anfragen den Pool nicht gemeinsam überziehen können, plus `FOR UPDATE`
auf der Nutzerzeile gegen Doppelabbuchung.

### 6.2 Zustandsautomat einer Umwandlung (`token_rewards`, `kind='conversion'`)

```
 Nutzer: POST /points/convert
         │  Punkte −N, Ledger points_conversion(−N)
         ▼
 awaiting_approval ──admin reject──► rejected   (Punkte +N, Ledger payout_refund)
         │ admin approve
         ▼
 pending ──────────────────────────► minted     (tx_hash gesetzt, Ledger onchain_payout(+N YEET, tx_hash))
         │ Mint schlägt fehl (mint_attempts++, last_error)
         │ ≥ YEET_MINT_MAX_ATTEMPTS (Default 5)   oder   Sanktionstreffer beim Mint
         ▼
 failed ───admin reject──────────► rejected    (Punkte +N, Ledger payout_refund)
```

- **`awaiting_approval`** — Punkte sind bereits abgebucht; der Batch-Minter ignoriert diesen Status. Menschliche Freigabe ist für den Launch bewusst vorgesehen (`api/points.rs`, Kommentar), automatisierte Regeln sollen folgen (`TODO(strategie)`).
- **`pending`** — vom Admin freigegeben (`POST /api/v1/admin/payouts/:id/approve`), wartet auf den nächsten Batch.
- **`minted`** — on-chain erfüllt; `tx_hash` ist der Nachweis.
- **`failed`** — endgültig gescheitert oder Sanktionstreffer (`last_error = 'SANCTIONED_ADDRESS: …'`); Punkte bleiben abgebucht, bis ein Admin per **Reject** zurückbucht. Ein `pending`-Eintrag kann **nicht** abgelehnt werden (der Minter könnte ihn gerade ausführen).
- **`rejected`** — geschlossen, Punkte wieder beim Nutzer. Nur dieser Status wird aus der Pool-Berechnung herausgerechnet.

Der Nutzer sieht in „Punkte umwandeln" die Bestätigung `convert.queued` („Eingereiht: N Punkte
→ YEET an 0x…"), unter `GET /api/v1/tokens/balance` den noch offenen Betrag
(`get_pending_payout`: Summe `awaiting_approval` + `pending`). Admins sehen die Queue unter
`GET /api/v1/admin/payouts?status=…` (admin.html).

### 6.3 Umwandlungsverhältnis (versioniert, nur prospektiv änderbar — L7)

- Tabelle `conversion_rates` (Migration 0052): `rate` = YEET je Punkt, `valid_from`, `announced_at`, `note`, `created_by`. Gültig ist die Zeile mit dem jüngsten `valid_from ≤ now()`; Zeilen mit künftigem `valid_from` sind **angekündigte** Änderungen.
- Jede Umwandlung speichert `points_debited` und `rate` auf der `token_rewards`-Zeile; `amount` ist der zu mintende YEET-Betrag (= Punkte × Kurs, 8 Nachkommastellen). Ablehnung/Erstattung gibt `points_debited` zurück. Der Journaleintrag `points_conversion` nennt Punkte, YEET und Kurs.
- **Vorlauf:** `POST /api/v1/admin/conversion-rate` lehnt jede Änderung ab, die früher als `YEET_RATE_NOTICE_DAYS` (Default 14) nach dem Eintragen gelten würde; rückwirkende Änderungen sind damit technisch ausgeschlossen (AGB §6: „nur für künftige Umwandlungen, vorab angekündigt“). Admin-Aktion wird in `admin_actions` protokolliert.
- **Transparenz:** `GET /api/v1/points/rate` (öffentlich) liefert aktuellen Kurs, angekündigte Änderungen und die Vorlauffrist. Der Umwandlungsdialog zeigt den Kurs dynamisch und blendet eine angekündigte Änderung mit Datum ein (38 Sprachen, `convert.rateChange`). Admin-Panel unter „Auszahlungen“.
- **Prozess bei Änderung (Anwalt/Marketing):** 1) Kurs mit Datum eintragen, 2) Changelog-Eintrag des Updates-Bots veröffentlichen (Nutzerinformation), 3) ggf. AGB-Hinweis prüfen. Punkte verfallen nicht; wer vor dem Stichtag umwandelt, erhält den alten Kurs.

### 6.4 Conversion-Pool (Drain-Schutz)

```
effective_pool = YEET_CONVERSION_POOL (Default 15 750 000 000) + Σ fee_ledger.fee_amount
converted      = Σ token_rewards.amount  WHERE kind='conversion' AND status <> 'rejected'
remaining      = max(effective_pool − converted, 0)
```

Öffentlich einsehbar unter `GET /api/v1/tokens/pool`. Sinkt `remaining` unter 10 % des
Basispools, werden neue Engagement-Rewards halbiert (Taper), damit nicht dauerhaft mehr
Punkte entstehen, als je ausgezahlt werden können. Der Registrierungsbonus ist davon ausgenommen.

Der Basispool entspricht `YeetToken.REWARD_RESERVE` (75 % von 21 Mrd. = 15,75 Mrd.), der
einzigen nach dem Deploy mintbaren Menge. Der **effektive** Pool (Basis + recycelte Gebühren)
ist zusätzlich auf diese Reserve gedeckelt (`YEET_REWARD_RESERVE`), damit Gebühren-Recycling
nie mehr zusagt, als der Contract minten kann. Nach einem Redeploy mit anderer Tranche beide
Werte per Env nachziehen. (Bis 2. Okt. 2026 stand der Contract auf 1 Mrd. und mintete beim
Deploy 100 % — `batchMintRewards` hätte nie etwas auszahlen können; behoben, D2.)

---

## 7. Auszahlung On-Chain (Batch-Mint)

| Aspekt | Ist-Zustand |
| --- | --- |
| Job | `services/batch_rewards.rs::start_reward_batch_job`, **stündlich**, nur aktiv wenn `REWARDS_MINTER_PRIVKEY` gesetzt |
| Auswahl | `kind='conversion' AND status='pending' AND tx_hash IS NULL`, Wallet aus `users.wallet_address` |
| Screening | F6 vor jedem Batch: Treffer → `failed` (+Grund); Liste nicht geladen → **gesamter Batch wird zurückgehalten** (fail-closed) |
| Chain / Contract | `YEET_CHAIN_ID` (Default 56), `BSC_RPC_URL`, `YEET_TOKEN_ADDRESS`; Aufruf `YeetToken.batchMintRewards(recipients, amounts, actions)` |
| Berechtigung | `batchMintRewards` ist `onlyOwner`, gekappt durch `REWARD_RESERVE` (75 % = 15,75 Mrd., kumulativ über `rewardsMinted`, Burns öffnen nichts). Keine andere Mint-Funktion. Der Minter-Key ist derzeit der Contract-Owner (Hot Key auf dem Server) → **F8 / Checkliste: Ownership → Multisig** (Ownable2Step) noch offen |
| Betrag | 1 Punkt = 1 YEET (× 10¹⁸ wei) |
| Nachweis | `token_rewards.tx_hash`, Ledger `onchain_payout` je Umwandlung (Asset `YEET`, `onchain_tx_hash`), Event `RewardMinted(recipient, amount, action)` on-chain |
| Fehlerfall | Tx-Fehler: `mint_attempts++`, `last_error`; nach 5 Versuchen `failed`; Punkte bleiben abgebucht bis Admin-Reject |
| Was die Plattform **nicht** tut | Keine Verwahrung, kein Transfer aus einem Plattformbestand, kein Rücknahme-/Rückkaufpfad, keine Fiat-Auszahlung |

Nach dem Mint endet die Verantwortung der Plattform für die Token (AGB §6 „Irreversibility").
Der Nutzer kann sie frei halten, übertragen oder (künftig, F1/F2) für On-Chain-Tips einsetzen.

**Sonderfall NOTE-Swap** (`docs/swap-note-to-yeet.md`, inert bis `SWAP_ENABLED=true`): Dort
entsteht die Umwandlungs-Zeile nicht aus Punkten, sondern aus einer bestätigten NOTE-Einzahlung
(Ledger `note_swap_in`, 100 NOTE = 1 YEET) und durchläuft dann **dieselbe** Pipeline
(Admin-Freigabe, Pool, F6, Batch). Rechtlich ist das ein Krypto-zu-Krypto-Vorgang und muss
**separat** bewertet werden (`TODO(strategie)`, Doc 06 §7 Nachbarregime).

---

## 8. Nachweis: das Transaktionsjournal

`ledger_entries` (Migration 0039, `services/ledger.rs`) ist das Beweis- und Steuerjournal:

- **append-only** — DB-Trigger blockieren `UPDATE`/`DELETE`; Korrekturen nur durch Gegenbuchung;
- **lückenlos** — `entry_no` 1, 2, 3 … unter Advisory-Lock vergeben;
- **manipulationserkennend** — `entry_hash = sha256(kanonischer Inhalt ‖ prev_hash)`; Prüfung über `GET /api/v1/admin/ledger/verify` (liefert die erste gebrochene `entry_no` oder „intakt");
- **atomar** — Saldoänderung und Journal-Eintrag liegen in derselben DB-Transaktion (`record_in_tx`), Ausnahme der On-Chain-Payout-Eintrag (best effort *nach* bestätigter Tx, damit ein Ledger-Fehler einen bereits gesettelten Mint nicht „rückgängig" erscheinen lässt);
- **Exporte** — `GET /api/v1/admin/ledger/export` (CSV, DATEV-freundlich), `/summary` (Aggregat je `tx_type`×`asset`), Nutzer-Selbstauskunft `GET /api/v1/users/me/export`;
- **Bewertungsspalten** — `fiat_value`, `fx_rate`, `fx_source` sind vorhanden, aber **leer**, solange kein Marktpreis existiert (F4: kein fiktiver Kurs).

Vollständige `tx_type`-Liste: `reward_grant`, `registration_bonus`, `tip_sent`, `tip_received`,
`ppv_purchase`, `ppv_earning`, `platform_fee`, `paper_wallet_issue/claim/refund`,
`points_conversion`, `payout_refund`, `onchain_payout`, `note_swap_in`, `live_promotion`,
`live_promotion_refund`, `opening_balance`, `onchain_tip`, `onchain_ppv` (die letzten beiden
erst mit dem Indexer, Doc 08).

### 8.1 Abstimmung (Reconciliation)

Diese Gleichungen müssen jederzeit gelten; Abweichungen sind ein Incident:

```sql
-- (1) Punktesalden = Summe aller Punkte-Buchungen je Nutzer
SELECT u.id, u.yeet_token_balance, COALESCE(SUM(l.amount),0) AS ledger_sum
  FROM users u LEFT JOIN ledger_entries l ON l.user_id = u.id AND l.asset = 'POINTS'
 GROUP BY u.id, u.yeet_token_balance
HAVING ABS(COALESCE(u.yeet_token_balance,0) - COALESCE(SUM(l.amount),0)) > 1e-6;

-- (2) Jede gemintete Umwandlung hat genau einen onchain_payout-Eintrag mit tx_hash
SELECT r.id FROM token_rewards r
 WHERE r.kind='conversion' AND r.status='minted'
   AND NOT EXISTS (SELECT 1 FROM ledger_entries l
                    WHERE l.tx_type='onchain_payout' AND l.reference_id = r.id::text
                      AND l.onchain_tx_hash = r.tx_hash);

-- (3) Offene Umwandlungen (Punkte abgebucht, noch nicht erfüllt/zurückgebucht)
SELECT status, COUNT(*), SUM(amount) FROM token_rewards
 WHERE kind='conversion' AND status IN ('awaiting_approval','pending','failed')
 GROUP BY status;
```

Alle drei Gleichungen plus die Kettenprüfung liefert `GET /api/v1/admin/ledger/reconcile`
in einem Aufruf; ein täglicher Job (2 min nach Start, dann alle 24 h) schreibt bei jeder
Abweichung eine Warnung ins Log. Altbestände aus der Zeit vor Migration 0039 erscheinen in
Gleichung (1) als Differenz; dafür schreibt `POST /api/v1/admin/ledger/baseline` **einmalig**
je Nutzer einen `opening_balance`-Eintrag über die Differenz (Eröffnungsbilanz) — vorher die
Reconcile-Liste prüfen, danach muss (1) dauerhaft aufgehen.

---

## 9. Bekannte Lücken (Dev)

| # | Lücke | Wirkung | Maßnahme |
| --- | --- | --- | --- |
| D1 | ~~Live-Promotion ohne Journaleintrag~~ **behoben (3. Okt. 2026):** `live_promotion` bei Buchung, `live_promotion_refund` bei Erstattung; Sweep-Job nutzt dieselbe Funktion wie `cancel_live` (die `fee_ledger`-Gegenbuchung gab es bereits) | — | — |
| D2 | ~~Pool-Default vs. `MAX_SUPPLY`~~ **behoben (2. Okt. 2026):** Contract auf 21 Mrd./Tranchen des Whitepapers umgestellt, 75 % nur via `batchMintRewards` mintbar (`rewardsMinted`-Deckel, kein generisches `mint()`), effektiver Pool im Backend auf `REWARD_RESERVE` gedeckelt | — | Nach Deploy: `rewardsRemaining()` gegen Pool-Status abgleichen (D6) |
| D3 | Minter-Key = Contract-Owner (Hot Key) | Single Point of Failure; F8 | Ownership → Multisig (Ownable2Step); Minter nur mit begrenzter Minter-Rolle |
| D4 | ~~Verhältnis als Code-Konstante~~ **behoben (3. Okt. 2026):** `conversion_rates` mit Gültig-ab, Vorlauffrist ≥ 14 Tage serverseitig erzwungen, Kurs/Punkte auf jeder Umwandlung und im Journal, öffentlicher Endpoint + dynamischer Dialog (§6.3) | — | Bei Kursänderung zusätzlich Changelog-Eintrag veröffentlichen |
| D5 | `pending` kann vom Admin nicht zurückgezogen werden | Einmal freigegeben, nur über Mint-Fehler → `failed` wieder stornierbar | Bewusst so (Race mit Minter); ggf. „Approve zurücknehmen" nur zwischen Batches mit Lock |
| D6 | ~~Abstimmung manuell~~ **behoben (3. Okt. 2026):** `GET /api/v1/admin/ledger/reconcile` (Gleichungen 1–3 + Kettenprüfung), täglicher Job mit Warnung im Log; `POST /api/v1/admin/ledger/baseline` schreibt einmalig `opening_balance`-Einträge für Guthaben aus der Zeit vor dem Journal | — | Baseline **einmal** nach Review der Reconcile-Liste ausführen |
| D7 | ~~PPV als Tip journaliert~~ **behoben (3. Okt. 2026):** `send_tip_tx(…, TipKind)`; PPV schreibt `ppv_purchase`/`ppv_earning` und `fee_ledger.source_type = 'ppv'`. Ältere Einträge bleiben `tip_*` (append-only) | — | — |

---

## 10. Prüfpunkte für den Anwalt

| # | Frage | Bezug |
| --- | --- | --- |
| A1 | **AGB §6 „custodial liability"** — **Dev erledigt (Stand 2. Okt. 2026):** §1 und §6 (EN/DE) auf das Punkte-Modell umgeschrieben: Punkte = Plattform-Punkte (kein Kryptowert, kein E-Geld, keine Einlage, nicht käuflich, kein Geldwert, nicht übertragbar); YEET nie in Plattformhand; Umwandlung einseitig, min. 100 P, 1:1 mit prospektivem Änderungsvorbehalt, Prüfung/Batch/Sanktions-/Pool-Vorbehalt, Rückbuchung bei Ablehnung, kein Geldanspruch. **Anwalt:** Wortlaut prüfen (AGB-Kontrolle § 307 BGB, Transparenz, Änderungsvorbehalt) | §3 dieses Dokuments, Doc 01 §4, `docs/rechtstexte.md` |
| A2 | Rechtsnatur des Punkteguthabens: Leistungsanspruch ohne Geldwert tragfähig? Verjährung/Verfall von Punkten regeln? (derzeit **kein** Verfall implementiert) | §3 |
| A3 | Umwandlung als „Ausschüttung eigener Token ohne Gegenleistung" vs. Tausch: hält die Einordnung „kein CASP-Dienst" (Doc 05 §2) angesichts Admin-Freigabe und Pool-Vorbehalt? | §6–7 |
| A4 | Zulässigkeit der Vorbehalte (Pool, Freigabe, Verhältnisänderung, Sanktion) als AGB-Klauseln gegenüber Verbrauchern (§ 307 ff. BGB), Transparenzgebot | §3 Nr. 3, L7 |
| A5 | Plattformgebühr 10 % auf Punkte-Tips/PPV: Umsatzsteuer, Hinweispflicht, Interessenkonflikt-Disclosure | §5, Doc 04 §C |
| A6 | Registrierungsbonus (1 000 P nach KYC) als „Gegenleistung für Daten" → Whitepaper-Gratis-Ausnahme (Doc 01 §3.2) | §4 |
| A7 | NOTE-Swap (Krypto→YEET) — separate Einordnung, bevor `SWAP_ENABLED` | §7 |
| A8 | Aufbewahrung des Ledgers (GoBD/AO: 10 Jahre) vs. DSGVO-Löschung — `user_id` wird bei Account-Löschung auf `NULL` gesetzt (`ON DELETE SET NULL`), Einträge bleiben | §8 |
| A9 | Wording der Nutzer-Bestätigung `convert.queued` („Auszahlung im nächsten Batch") — Erwartungsmanagement angesichts Admin-Freigabe | §6.2 |

---

## Anhang A — Endpoints

| Methode | Pfad | Zweck |
| --- | --- | --- |
| POST | `/api/v1/points/convert` | Umwandlung anstoßen (`{points}`) |
| GET | `/api/v1/points/rate` | Aktueller Kurs, angekündigte Änderungen, Vorlauffrist (öffentlich) |
| GET / POST | `/api/v1/admin/conversion-rate` | Kurs-Historie · neuen Kurs mit Gültig-ab ankündigen (≥ Vorlauffrist) |
| GET | `/api/v1/tokens/balance` | Punktesaldo + offene Auszahlung |
| GET | `/api/v1/tokens/rewards` | Reward-Historie des Nutzers |
| GET | `/api/v1/tokens/pool` | Pool-Status (öffentlich) |
| GET | `/api/v1/admin/payouts?status=` | Freigabe-Queue |
| POST | `/api/v1/admin/payouts/:id/approve` · `/reject` | Freigabe / Ablehnung (+Rückbuchung) |
| GET | `/api/v1/admin/ledger` · `/export` · `/summary` · `/verify` | Journal, CSV, Aggregat, Kettenprüfung |
| GET / POST | `/api/v1/admin/ledger/reconcile` · `/baseline` | Abstimmung §8.1 · einmalige Eröffnungsbilanz |
| GET | `/api/v1/users/me/export` | Selbstauskunft des Nutzers |

## Anhang B — Konfiguration

| Variable | Default | Bedeutung |
| --- | --- | --- |
| `YEET_CONVERSION_POOL` | 15 750 000 000 | Basispool in YEET = `REWARD_RESERVE` (§6.4) |
| `YEET_REWARD_RESERVE` | 15 750 000 000 | Harter Deckel des effektiven Pools (Basis + Gebühren) = On-Chain-Mint-Reserve |
| `YEET_TAPER_THRESHOLD_PCT` / `YEET_TAPER_FACTOR` | 10 / 0,5 | Reward-Taper |
| `YEET_DAILY_POINTS_CAP`, `YEET_POST_REWARD`, `YEET_POST_MIN_CHARS` | 1 000 / 10 / 120 | Reward-Regeln |
| `YEET_REGISTRATION_BONUS`, `YEET_REGISTRATION_BONUS_MAX` | 1 000 / 100 000 | Bonus |
| `REWARDS_MINTER_PRIVKEY` | — | Minter aktiv nur wenn gesetzt (**D3**) |
| `YEET_CHAIN_ID`, `BSC_RPC_URL`, `YEET_TOKEN_ADDRESS` | 56 / BSC-Dataseed / — | Chain |
| `YEET_MINT_MAX_ATTEMPTS` | 5 | Versuche bis `failed` |
| `YEET_RATE_NOTICE_DAYS` | 14 | Mindestvorlauf für eine Kursänderung (§6.4) |
| `SANCTIONS_*` | s. `docs/sanktions-screening.md` | F6 |
