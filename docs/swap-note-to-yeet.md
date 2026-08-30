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

## 4. Was noch fehlt (Reihenfolge bis zum Start)

1. **YEET-Smart-Contract** (BEP-20, 21-Mrd.-Cap, Minter/Vesting) — wird separat
   geschrieben; der Swap zahlt über denselben Minter-Pfad aus.
2. Note-Fullnode aufsetzen (+ ggf. Fork/Docker), Testnet-Durchstich:
   tnote-Einzahlung → YEET-Testnet-Mint.
3. Backend: Migration (`swap_addresses`, `swap_deposits`), Watcher-Job,
   API (`POST /api/v1/swap/address`, `GET /api/v1/swap/status`),
   Ledger-tx_types `NOTE_SWAP_IN` / `NOTE_SWAP_PAYOUT`.
4. Swap-Seite entsperren (Formular aktivieren), Mindestbetrag + Confirmations
   final festlegen, **Startankündigung überall**.

## 5. Sicherheits-/Compliance-Notizen

- Kurs fix 100:1, keine Yeet-Gebühr auf den Swap (Ausnahme wie Registrierungs-
  bonus); Netzwerkgebühren (NOTE-Tx-Fee, BNB-Gas) trägt der User.
- KYC-Pflicht vor Adressvergabe; eine Einzahladresse pro Identität.
- Idempotenz über txid:vout; Auszahlungen laufen durch die Admin-Freigabe
  (Startphase) und den Pool-Guard.
- Warnhinweis auf der Seite: vor dem offiziellen Start keine NOTE an
  irgendwen senden („Fake-Swap"-Betrugsschutz).
