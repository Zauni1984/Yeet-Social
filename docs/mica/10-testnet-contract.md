# 10 — Ein Contract reicht: `YeetToken` auf dem BSC-Testnet, Punktesystem ohne Chain

**Status: ENTWURF (Dev)** · Stand 2026-10-27 · Quellen: `contracts/src/YeetToken.sol`,
`contracts/script/DeployToken.s.sol`, `contracts/test/YeetToken.t.sol` (**`forge test`: 54/54 grün**),
`backend/src/services/batch_rewards.rs`, `backend/src/api/points.rs`, `backend/src/api/tips.rs`.

Dieses Dokument beantwortet zwei Fragen:

1. **Läuft das Belohnungssystem vollständig ohne Blockchain?** — Ja, nachgewiesen (§1).
2. **Welcher Smart Contract ist dafür nötig?** — Genau einer: der Token selbst, mit einer
   Minter-Rolle, einem Tages-Limit und einem Not-Aus (§2). Alles andere bleibt bis auf
   Weiteres **undeployt** (§3). Zuerst Testnet (§5), Mainnet-Umstellung später (§6).

---

## 1. Das Punktesystem ist chain-unabhängig — Nachweis

### 1.1 Architektur in einem Satz

Alles, was Nutzer im Alltag tun — Punkte verdienen, Tipps geben, Pay-per-View freischalten,
Paper Wallets ausstellen/einlösen, Registrierungsbonus, Umtausch beantragen — ist eine
**Buchung im Backend-Ledger** (`users.yeet_token_balance` + append-only Journal
`ledger_entries` mit Hash-Kette, [09-ledger-und-auszahlung.md](09-ledger-und-auszahlung.md)).
Die Chain wird **an genau einer Stelle** berührt: wenn eine vom Admin freigegebene Umwandlung
Punkte → YEET per `batchMintRewards` in die **eigene Wallet des Nutzers** gemintet wird.
Die Plattform hält on-chain nichts (non-custodial, [05](05-zielarchitektur-non-custodial.md)).

### 1.2 Was der Code ohne Chain-Konfiguration tut

Ohne `REWARDS_MINTER_PRIVKEY` startet der Batch-Minter nicht (D3); ohne
`YEET_TOKEN_ADDRESS` antworten `/api/v1/config` und `/api/v1/explorer/token` mit „kein
Contract". Kein Endpoint des Punktesystems macht einen RPC-Aufruf. Freigegebene Umwandlungen
bleiben in `token_rewards.status='pending'`, bis ein Minter konfiguriert ist — nichts geht
verloren, nichts wird doppelt gebucht.

### 1.3 Prüflauf (lokal, 2026-10-27, ohne RPC / Token-Adresse / Minter-Key)

Skript: Backend mit `DATABASE_URL`, `REDIS_URL`, `JWT_SECRET`, `ADMIN_SECRET` gestartet,
alle Chain-Variablen **entfernt**. Ergebnis **25/25 Prüfungen grün**, 0 Chain-Fehler im Log:

| Bereich | Geprüft | Ergebnis |
| --- | --- | --- |
| Verdienen | Artikel ≥ 120 Zeichen = 10 P, Kommentar = 1 P, Reshare = 2 P, Kurzpost = 0 P | ✅ |
| Bonus | 1 000 P einmalig nach E-Mail- **und** Altersverifikation | ✅ |
| Ausgeben | Tipp 100 → 90 Empfänger / 10 `fee_ledger`; PPV 20 → 18 / 2 mit Consent-Protokoll; Paper Wallet 50 ausstellen (gesperrt) und einlösen | ✅ |
| Umtausch | 200 P abgebucht, `token_rewards` = `awaiting_approval` mit Wallet; Admin-Freigabe → `pending`; bleibt `pending` ohne Minter; Ablehnung einer `pending`-Zeile korrekt verweigert (422) | ✅ |
| Journal | Saldo == Journalsumme je Testnutzer; Hash-Kette intakt; alle neun Buchungsarten vorhanden (`reward_grant`, `registration_bonus`, `tip_sent/received`, `ppv_purchase/earning`, `paper_wallet_issue/claim`, `points_conversion`) | ✅ |
| Öffentlich | `/config` (kein Token, Chain 97), `/tokens/pool`, `/explorer/token` antworten 200 | ✅ |

### 1.4 Bei der Prüfung gefundene und behobene Fehler

Der erste Lauf zeigte **HTTP 500 bei Tipps und PPV** auf einer frisch aus den Migrationen
aufgebauten Datenbank. Ursachen (alle in Migration `0054_tips_points_schema.sql` + `tips.rs`
behoben):

| # | Befund | Fix |
| --- | --- | --- |
| 1 | `tips.currency` war der Enum `tip_currency('yeet','bnb','fiat')`, der Code schreibt/liest den Text `'YEET'` | Spalte → `TEXT`, Werte groß, `CHECK (YEET/BNB/FIAT)` |
| 2 | `tips.post_id NOT NULL`, aber DM-Tipps und Live-Tipps haben keinen Post | `DROP NOT NULL` |
| 3 | Beträge als Text gebunden → `numeric` lehnt ab | f64-Binds |
| 4 | `ppv_unlocks` hatte die Form aus Migration 0001 (`amount_paid`, `created_at`); 0025 (`price_paid`, `tip_id`, `unlocked_at`) war ein `CREATE TABLE IF NOT EXISTS` → No-op | Spalten umbenennen/ergänzen, idempotent |
| 5 | **Log-Filter falsch**: `RUST_LOG=backend=info` — der Tracing-Target heißt aber `yeet_server` (Binary-Name). Folge: **keine** App-Logs, auch keine DB-Fehler, in Compose/VPS | Default `yeet_server=info,…` in Code, Compose, `.env.example` |
| 6 | Chain-Default im Backend-Code war 56 (Mainnet), Compose 97 | Code-Default 97; Standard-RPC folgt der Chain-ID |

> Hinweis für den Betrieb: Ob die Produktions-DB von Befund 1/4 betroffen war, hängt von ihrer
> Historie ab (Migrationen 0002–0011 fehlen im Repo). Migration 0054 ist auf beiden Formen
> idempotent; nach dem Deploy einmal `tips` und `ppv_unlocks` per Testnutzer durchspielen.

---

## 2. Vorschlag: genau ein Contract — `YeetToken`

### 2.1 Warum ein Contract genügt

Weil die Chain nur eine Aufgabe hat: **YEET existieren lassen und an Nutzer ausgeben**.
Tipps, PPV, Paper Wallets laufen als Punkte; sobald YEET in der Nutzer-Wallet sind, sind sie ein
gewöhnlicher BEP-20 — jede Wallet, jede DEX kann sie bewegen, ohne dass Yeet etwas betreibt.
Jeder weitere Contract (Payments, Escrow, NFT) wäre Angriffsfläche und Audit-Umfang ohne
Nutzen, solange Zahlungen nicht on-chain stattfinden.

### 2.2 Was der Contract kann (und was nicht)

| Eigenschaft | Umsetzung |
| --- | --- |
| Standard | BEP-20 (ERC-20) + `ERC20Burnable` + `ERC20Permit`, 18 Dezimalen |
| Angebot | `MAX_SUPPLY` 21 000 000 000 fest. 25 % (Dev 10 / Team 10 / Reserve 5) beim Deploy an `owner`; **75 % (`REWARD_RESERVE` 15,75 Mrd.)** nur über `batchMintRewards`, kumulativ gedeckelt (`rewardsMinted`), **Burns öffnen die Reserve nicht wieder** |
| Rollen | `owner` (später Multisig, `Ownable2Step`): setzt Minter, Tages-Cap, Pause — **mintet nie selbst**. `minter` (Backend-Hot-Key): kann **nur** `batchMintRewards` |
| Grenzen pro Mint | ≤ `MAX_BATCH` 200 Empfänger · Summe ≤ `REWARD_RESERVE` · Summe ≤ `dailyMintCap` pro UTC-Tag (`dailyRemaining()`) · kein Null-Empfänger · atomar (ein Fehler → ganzer Batch zurück) |
| Not-Aus | `setMintingPaused(true)` stoppt Mints, **nie Transfers** |
| Events | `RewardMinted(recipient, amount, action)`, `MinterUpdated`, `DailyMintCapUpdated`, `MintingPaused` |
| Bewusst **nicht** enthalten | kein Verkauf/Presale, keine Transfer-Gebühr, keine Blacklist, keine Upgradeability/Proxy, kein Admin-Zugriff auf Nutzerguthaben, keine Vesting-Logik (separater Vertrag/Multisig-Policy, [02](02-whitepaper-entwurf-yeet.md) Teil J) |

**Schadensbegrenzung:** Ein geleakter Minter-Key kann höchstens *einen Tages-Cap* an Reward-YEET
minten (nicht die 25 % des Owners, keine fremden Guthaben), bevor der Owner pausiert oder
`setMinter` rotiert. Deshalb ist die Trennung Owner ≠ Minter auf Mainnet **Pflicht** (das
Deploy-Skript erzwingt sie bei Chain-ID 56).

### 2.3 Tests (`forge test`, 54/54)

`YeetToken.t.sol` (24): Tranchen summieren auf `MAX_SUPPLY`; Minter mintet; Owner und Fremde
werden mit `NotMinter` abgewiesen; Minter-Rotation; nur Owner verwaltet Rollen; Tages-Cap
greift, ist atomar und setzt sich am nächsten UTC-Tag zurück; Cap 0 = unbegrenzt; Pause stoppt
Mints, nicht Transfers; Reserve exakt bis `MAX_SUPPLY`; Burn öffnet Reserve nicht; Längen-,
Batch-, Null-Adress-Checks; Event; zweistufige Ownership-Übergabe.

---

## 3. Was **nicht** deployt wird (und wann doch)

| Contract | Status | Bedingung für Deploy |
| --- | --- | --- |
| `YeetPayments` (On-Chain-Tipps/PPV) | im Repo, getestet, **undeployt** | erst wenn Zahlungen bewusst on-chain gehen sollen (Strategie-/Rechtsentscheid, [06](06-leitplanken-validierung.md) F1/F2) |
| `PaperWalletEscrow` | im Repo, getestet, **undeployt** | wie oben (F3) |
| `YeetTipping`, `YeetNFT` | Legacy | nicht vorgesehen |

`script/Deploy.s.sol` (Gesamt-Suite) bleibt für diesen späteren Fall erhalten; der Workflow
`deploy-contracts.yml` deployt standardmäßig **nur den Token** (`scope: token`).

---

## 4. Backend-Integration

| Variable | Testnet-Wert |
| --- | --- |
| `YEET_CHAIN_ID` | `97` (Default im Code und in Compose) |
| `BSC_RPC_URL` | Testnet-RPC (Default folgt der Chain-ID) |
| `YEET_TOKEN_ADDRESS` | Adresse aus dem Deploy |
| `REWARDS_MINTER_PRIVKEY` | **Private Key der Minter-Wallet** (nicht des Deployers/Owners) |
| `YEET_CONVERSION_POOL` / `YEET_REWARD_RESERVE` | 15 750 000 000 (= `REWARD_RESERVE`) |

Der Batch-Minter prüft vor jedem Lauf **on-chain**: `minter()` == eigene Adresse (sonst
Fehler-Log, Queue bleibt `pending`), `mintingPaused()` (hält), `dailyRemaining()` (Zeilen, die
nicht mehr ins Tagesbudget passen, warten auf den nächsten Tag). Danach wie in
[09 §7](09-ledger-und-auszahlung.md): Sanktions-Screening (F6), `pending → minting`,
Gas-Schätzung (ein Revert wird **nicht** gesendet), Broadcast, Quittung → `minted`/`failed`,
Abgleich hängender Transaktionen beim nächsten Lauf.

---

## 5. Runbook Testnet (BSC Testnet, Chain-ID 97)

1. **Zwei Wallets anlegen** (z. B. MetaMask, getrennte Konten): *Deployer/Owner* und *Minter*.
   Beide mit tBNB aus dem Faucet (`https://testnet.bnbchain.org/faucet-smart`) befüllen;
   der Minter braucht laufend Gas (≈ 0,01 tBNB je Batch reicht lange).
2. **GitHub-Secrets** (Repo → Settings → Secrets): `BNB_PRIVATE_KEY` = Deployer-Key,
   `BSCSCAN_API_KEY`. Der Minter-Key kommt **nie** zu GitHub, nur ins Server-`.env`.
3. **Workflow „Deploy Contracts"** manuell starten: `network = bsc_testnet`, `scope = token`,
   `minter_address = <Minter-Adresse>`, `daily_mint_cap = 1000000` (1 Mio. YEET/Tag; für Tests
   beliebig). Die Ausgabe enthält `YeetToken: 0x…` und die `.env`-Zeilen.
4. **Server-`.env`** (`/root/yeet-social/.env`): `YEET_CHAIN_ID=97`, `BSC_RPC_URL=<Testnet-RPC>`,
   `YEET_TOKEN_ADDRESS=0x…`, `REWARDS_MINTER_PRIVKEY=<Minter-Key>`; Backend neu starten.
5. **Prüfen:** `curl https://justyeet.it/api/v1/config` → `chain_id: 97`, Token-Adresse gesetzt;
   `/api/v1/explorer/token` zeigt Supply/Reserve; Wallet-Login zeigt „BNB Smart Chain Testnet".
6. **Ende-zu-Ende:** Testnutzer verdient Punkte → Umtausch beantragen → Admin-Freigabe
   (`admin.html`, Payouts) → nächster Minter-Lauf → `token_rewards.status='minted'`,
   `tx_hash` auf `https://testnet.bscscan.com` sichtbar, YEET in der Nutzer-Wallet.
7. **Not-Aus üben:** Owner ruft `setMintingPaused(true)` (bscscan „Write Contract") → Minter-Log
   meldet „paused", Queue bleibt stehen → `false` → Lauf geht weiter.
8. Adresse in `contracts/README.md` (Tabelle „Deployed Addresses") eintragen.

Testnet-YEET haben **keinen Wert** und werden **nicht** auf Mainnet migriert; das ist in der
In-App-Kommunikation (Umtausch-Dialog) so zu kennzeichnen, solange `chain_id == 97`.

---

## 6. Umstellung auf Mainnet (später)

Voraussetzungen (Checkliste [04](04-compliance-checkliste.md) B1/F8, keine davon optional):

1. Rechts-/Strategiefreigabe (Whitepaper-Status, kein Verkauf, Marketing-Regeln).
2. **Externes Audit** des Contracts in seiner finalen Fassung (Commit-Hash festhalten).
3. **Multisig** (z. B. Safe) als Owner: Deploy mit `OWNER_ADDRESS=<Safe>`, dann
   `acceptOwnership()` aus dem Safe. `MINTER_ADDRESS` = dedizierte Hot-Wallet ≠ Deployer
   (das Skript bricht auf Chain 56 sonst ab); `DAILY_MINT_CAP` aus dem erwarteten
   Umtauschvolumen ableiten (eher klein starten; Owner kann erhöhen).
4. Umtausch während der Umstellung pausieren (Admin: keine Freigaben), alle Testnet-Zeilen in
   `token_rewards` sind damit abgeschlossen und nicht übertragbar.
5. Server-`.env`: `YEET_CHAIN_ID=56`, Mainnet-RPC, **neue** `YEET_TOKEN_ADDRESS`, neuer
   Minter-Key; Restart; `/api/v1/config` → 56. Frontend folgt automatisch (Preset 0x38).
6. Erster Mint klein, manuell beobachtet; danach Tages-Cap wie geplant setzen.
7. Deployer-Key nach Ownership-Übergabe aus allen Systemen entfernen; `BNB_PRIVATE_KEY`-Secret
   löschen oder rotieren.

---

## 7. Offene Punkte

- Externes Audit und Multisig (F8) — organisatorisch, nicht im Code lösbar.
- Vesting Dev/Team (Teil J) als separater Vertrag oder Safe-Policy vor Mainnet.
- Tages-Cap und Freigabe-Regeln (automatische Freigabe kleiner Beträge?) sind Strategie-Fragen
  (`TODO(strategie)` in `api/points.rs`).
- UI-Hinweis „Testnet — kein Wert" im Umtausch-Dialog, solange `chain_id == 97`.
