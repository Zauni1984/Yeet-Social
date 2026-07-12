# 03 — Chain-Assessment: BNB Chain unter MiCA & Alternativen

**Status: ENTWURF** · **Kein Rechtsrat.**

## 1. Vorab: „Ist BNB Chain MiCA-konform?" — die Frage ist anders zu stellen

MiCA reguliert **Personen und Tätigkeiten** (Anbieter, Emittenten, Personen, die die
Zulassung zum Handel beantragen, und Kryptowerte-Dienstleister), **nicht Blockchains**.
Es gibt keine Zulassung, Zertifizierung oder Sperrliste für Netzwerke. Eine Blockchain
kann daher weder „MiCA-konform" noch „nicht MiCA-konform" sein.

**Für die Chain-Wahl MiCA-relevant sind nur:**
1. **Nachhaltigkeits-Offenlegung** (Art. 6 Abs. 1 lit. j; ESMA-RTS): Für den
   Konsensmechanismus der genutzten Chain müssen Energie-/Klimaindikatoren offengelegt
   werden. Praktisch heißt das: Für die Chain müssen **belastbare Daten verfügbar** sein
   (z. B. CCRI-Methodik). Für alle gängigen Chains inkl. BNB Smart Chain existieren solche
   Daten → **kein Ausschlusskriterium**.
2. **Beschreibbarkeit der Technologie** im Whitepaper (Teil G): Konsens, Validatoren,
   Governance, Risiken müssen redlich beschreibbar sein — bei BSC also insbesondere die
   vergleichsweise **hohe Zentralisierung** (Proof of Staked Authority, kleine
   Validatorenmenge) als Risiko-/Governancefaktor.
3. **Faktische Marktakzeptanz**: EU-CASPs entscheiden selbst, welche Assets sie listen.

**Fazit: BNB Smart Chain ist aus MiCA-Sicht zulässig.** Ein Wechsel wäre eine
strategische, keine zwingend regulatorische Entscheidung.

## 2. Bewertungsmatrix (Kandidaten)

Kriterien: EVM-Kompatibilität (bestehender Code: BEP-20-Contract, ethers-rs,
MetaMask-Login mit `wallet_switchEthereumChain`), Gebühren, Dezentralisierung,
Verfügbarkeit von Nachhaltigkeitsdaten, EU-Wahrnehmung.

| Chain | EVM | Gebühren | Dezentralisierung | Nachhaltigkeitsdaten | Anmerkungen |
| --- | --- | --- | --- | --- | --- |
| **BNB Smart Chain** (Status quo) | ✅ nativ | sehr niedrig | ⚠️ niedrig (PoSA, wenige Validatoren) | ✅ (CCRI) | Kein Migrationsaufwand; Reputationsnähe zu Binance im Whitepaper als Risiko adressierbar |
| **Ethereum Mainnet** | ✅ | hoch | ✅ sehr hoch | ✅ (PoS, sehr niedriger Verbrauch) | Für Micro-Tips zu teuer |
| **Polygon PoS** | ✅ | sehr niedrig | mittel | ✅ | Etabliert, günstig, gute EU-Akzeptanz |
| **Base** (OP-Stack L2) | ✅ | niedrig | mittel (zentraler Sequencer) | ✅ | Betreiber Coinbase (EU-MiCA-lizenzierter Konzern) — reputativ günstig; Sequencer-Zentralität offenlegen |
| **Arbitrum One** | ✅ | niedrig | mittel | ✅ | Große DeFi-Liquidität |
| Solana | ❌ (Rust/SPL) | sehr niedrig | mittel | ✅ | Kompletter Rewrite von Contract + Wallet-Flows → nicht empfohlen |

TODO(verify): Konkrete kWh-/THG-Werte je Kandidat nach CCRI/ESMA-Methodik einholen und in
Whitepaper Teil I übernehmen (Momentaufnahme + Aktualisierungsprozess definieren).

## 3. Empfehlung (Entwurf)

1. **Kurzfristig: auf BNB Smart Chain bleiben.** Kein MiCA-Hindernis; Aufwand und Risiko
   einer Migration sind derzeit nicht gerechtfertigt, solange kein Sale/Listing ansteht.
2. **Bei finaler Tokenisierungsstrategie neu bewerten:** Falls ein öffentliches Angebot
   oder EU-Listing geplant wird, sprechen EU-Wahrnehmung und Dezentralisierung eher für
   **Polygon PoS oder Base** (EVM-kompatibel → Migration = Contract-Redeploy +
   Chain-ID/RPC-Anpassung in `frontend/index.html` und `batch_rewards.rs`, Bridge- oder
   Snapshot-Migration der Bestände).
3. **Unabhängig von der Chain:** Die CASP-Frage aus Assessment §4 (interner Ledger) ist
   der eigentliche Blocker — sie ändert sich durch einen Chain-Wechsel **nicht**.

## 4. Migrationsaufwand (falls entschieden)

| Schritt | Aufwand |
| --- | --- |
| Contract-Redeploy (BEP-20 → ERC-20 auf Ziel-Chain) | niedrig (Standard) |
| Backend: RPC-URL, Chain-ID 56 → Ziel, `YEET_TOKEN_ADDRESS` | niedrig (`batch_rewards.rs`, env) |
| Frontend: `wallet_switchEthereumChain` (aktuell 0x61 Testnet-Hinweis!), Anzeige | niedrig |
| Bestandsmigration (Snapshot der On-Chain-Holder + interner Ledger) | mittel |
| Kommunikation/Anleitung für Nutzer-Wallets | mittel |

TODO(verify): Frontend nutzt beim MetaMask-Login Chain-ID `0x61` (BSC **Testnet**),
das Reward-Minting läuft gegen Chain-ID 56 (**Mainnet**) — Inkonsistenz klären.
