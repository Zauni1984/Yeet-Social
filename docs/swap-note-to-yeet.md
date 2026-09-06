# NOTE → YEET Swap — Technisches Design

Status: **vorbereitet, Swaps gesperrt** bis der YEET-Smart-Contract steht.
Öffentliche Seite: `frontend/swap.html` → https://justyeet.it/swap.html (via CD deployt).
Kurs: **100 NOTE = 1 YEET**, fix. Pool-Deckel: **500.000.000 YEET**
(bei geschätzt bis zu 50 Mrd. NOTE im Umlauf; ~2,4 % der 21-Mrd.-Gesamtmenge).
Team-NOTE sind vom Swap ausgeschlossen. Netzwerkgebühren trägt der User.

## 1. Was die Note-Blockchain technisch ist (abgeleitet aus note-llc/NoteBlockchain)

Quelle: `src/chainparams.cpp` + Release v1.1.0.0 „Annapurna".

| Eigenschaft | Wert |
| --- | --- |
| Codebasis | Bitcoin-Core-Familie (UTXO, `chainparams.cpp`-Layout; Litecoin-typisches powLimit) |
| Konsens | PoW, **Digishield**-Difficulty je Block (seit v1.1.0.0, Mandatory Upgrade) |
| Blockzeit | **30 Sekunden** (`nPowTargetSpacing = 30`) |
| Halving | alle 1.051.200 Blöcke; Genesis-Reward 50 NOTE (20.11.2018) |
| Adressen | Base58 PUBKEY-Präfix 53 (→ beginnen mit „N"), P2SH-Präfix 5, Bech32-HRP `note` (Testnet `tnote`) |
| Ports | P2P 16158 (Mainnet), 15159 (Testnet) |
| Daemon | Bitcoin-Core-artig (Qt-Wallet + Daemon in den Releases) → Standard-RPCs: `getnewaddress`, `listtransactions`, `getblock`, `validateaddress`, `signmessage`/`verifymessage`, ZMQ optional |
| Smart Contracts / Memos | **keine** — reine UTXO-Zahlungs-Chain |

**Konsequenz:** Ein atomarer On-Chain-Swap ist nicht möglich (keine Contracts auf
der Note-Seite). Der Swap ist eine **Einweg-Deposit-Bridge** nach dem
Exchange-Deposit-Muster.

## 2. Architektur (Einweg-Deposit-Bridge)

```
User-NOTE-Wallet ──(NOTE-Tx, Gebühr zahlt User)──▶ persönliche Einzahladresse
                                                      │  (eigener Note-Fullnode,
                                                      │   1 Adresse ↔ 1 Yeet-User)
                                                      ▼
                                            Watcher/Indexer (Backend)
                                            N Bestätigungen abwarten
                                                      ▼
                                    swap_deposits-Row + Hash-Chain-Ledger
                                                      ▼
                          bestehender Batch-Minter: amount/100 YEET → BSC-Wallet
                          (token_rewards kind='conversion', wie Points→YEET;
                           Admin-Freigabe-Gate greift auch hier)
```

1. **Registrierung:** eingeloggter, KYC-verifizierter User mit verknüpfter
   BSC-Wallet fordert auf der Swap-Seite seine Einzahladresse an. Backend zieht
   sie per `getnewaddress` vom eigenen Note-Node und speichert die Zuordnung
   (`swap_addresses: user_id ↔ note_address`, unique beidseitig).
2. **Einzahlung:** User sendet NOTE aus der eigenen Wallet. Kein Memo nötig —
   die Adresse selbst identifiziert den User (UTXO-Standardmuster).
3. **Bestätigungen:** 30-Sekunden-Blöcke ⇒ konservativ **120 Confirmations
   (~1 h)** gegen Reorgs/51 %-Risiko einer kleinen PoW-Chain. Wert per Env
   konfigurierbar.
4. **Gutschrift:** Watcher schreibt `swap_deposits` (txid, vout, amount,
   confirmations, status) idempotent (unique txid:vout), bucht YEET-Auszahlung
   über die bestehende Conversion-Pipeline (inkl. Pool-Guard: Swap zählt gegen
   den 500-Mio.-Pool; inkl. manueller Admin-Freigabe in der Startphase).
5. **Verwahrung der NOTE:** Alle Einzahladressen gehören einer dedizierten
   Wallet; NOTE werden regelmäßig auf eine **öffentlich kommunizierte
   Sink-Adresse** konsolidiert und kommen nie zurück in Umlauf (Note kennt
   kein Burn; „provably out of circulation" ist das Äquivalent).

## 3. Betrieb: eigener Note-Node (ggf. Fork)

- Wir betreiben einen eigenen Fullnode aus `note-llc/NoteBlockchain` (Release
  ≥ v1.1.0.0, Digishield ist mandatory).
- **Fork des Repos** (github.com/Zauni1984/…) ist vorgesehen, falls nötig:
  Build-Fixes für aktuelle Toolchains, Docker-Packaging, ZMQ-Aktivierung für
  Push-Benachrichtigungen statt Polling. Konsens-Änderungen sind ausgeschlossen.
- Node läuft isoliert (eigener Container, nur RPC zum Backend, kein Wallet-Key
  auf dem Webserver-Host; Wallet verschlüsselt, Backup der wallet.dat).

## 4. Stand & Startcheckliste

**Bereits umgesetzt (im Repo):**

- ✅ Öffentliche Swap-Seite `frontend/swap.html` (18 Sprachen über `SWAP_DICTS`, Sprache aus `localStorage.yeet_lang` wie die App, Rechner, Sperr-Banner,
  liest `GET /api/v1/swap/status`; sobald `enabled`, erscheint der Ablauf
  „Einzahladresse anfordern → Adresse + eigene Einzahlungen").
- ✅ Migration `0044_note_swap.sql`: `swap_addresses` (1 Adresse ↔ 1 User),
  `swap_deposits` (Idempotenz über `txid,vout`; `seen|credited|failed`).
- ✅ `services/note_swap.rs`: Env-Konfiguration, Bitcoin-Core-JSON-RPC-Client,
  Adressvergabe (`getnewaddress`, Label = User-ID), 60-s-Watcher
  (`listtransactions` → Upsert → Gutschrift ab N Bestätigungen als
  `token_rewards` `kind='conversion', action='note_swap'` → Admin-Freigabe →
  Batch-Minter), Ledger-Eintrag `note_swap_in` (Asset `NOTE`), Guards
  (Mindestbetrag, 500-Mio.-Cap, verknüpfte Wallet → sonst `failed` + Grund).
- ✅ API: `GET /api/v1/swap/status` (öffentlich), `POST /api/v1/swap/address`
  (`SWAP_LOCKED` → `KYC_REQUIRED` → `NO_WALLET_LINKED`), `GET /api/v1/swap/deposits`.
- ✅ Payout-Fehlerpfad (`0043`): gescheiterte Mints werden nach
  `YEET_MINT_MAX_ATTEMPTS` als `failed` geparkt und sind im Admin ablehn-/erstattbar.

**Env-Variablen (Backend):**

| Variable | Default | Bedeutung |
| --- | --- | --- |
| `SWAP_ENABLED` | `false` | Hauptschalter; solange aus, ist alles inert |
| `NOTE_RPC_URL` | – | JSON-RPC des eigenen Note-Nodes, z. B. `http://note-node:16157` |
| `NOTE_RPC_USER` / `NOTE_RPC_PASS` | – | RPC-Zugang (Basic Auth) |
| `SWAP_CONFIRMATIONS` | `120` | Bestätigungen bis Gutschrift (~1 h bei 30-s-Blöcken) |
| `SWAP_POOL_CAP_YEET` | `500000000` | Deckel der Swap-Auszahlungen in YEET |
| `SWAP_MIN_NOTE` | `0` | Mindestbetrag je Einzahlung (0 = keiner) |
| `YEET_MINT_MAX_ATTEMPTS` | `5` | Mint-Versuche bis `failed` |

**Noch offen bis zum Start (Reihenfolge):**

1. **YEET-Smart-Contract** (BEP-20, 21-Mrd.-Cap, Minter/Vesting) — separat;
   der Swap zahlt über denselben Minter-Pfad aus.
2. **Note-Fullnode** aufsetzen (Release ≥ v1.1.0.0; ggf. Fork für Build/Docker/ZMQ),
   RPC nur intern erreichbar, Wallet verschlüsselt + Backup.
3. **Testnet-Durchstich:** `tnote`-Einzahlung → Watcher → Admin-Freigabe → Testnet-Mint.
4. Mindestbetrag + Bestätigungen final festlegen (`SWAP_MIN_NOTE`, `SWAP_CONFIRMATIONS`).
5. `SWAP_ENABLED=true` setzen → Seite entsperrt sich automatisch → **Startankündigung überall**.

## 5. Sicherheits-/Compliance-Notizen

- Kurs fix 100:1, keine Yeet-Gebühr auf den Swap (Ausnahme wie Registrierungs-
  bonus); Netzwerkgebühren (NOTE-Tx-Fee, BNB-Gas) trägt der User.
- KYC-Pflicht vor Adressvergabe; eine Einzahladresse pro Identität.
- Idempotenz über txid:vout; Auszahlungen laufen durch die Admin-Freigabe
  (Startphase) und den Pool-Guard.
- Warnhinweis auf der Seite: vor dem offiziellen Start keine NOTE an
  irgendwen senden („Fake-Swap"-Betrugsschutz).
