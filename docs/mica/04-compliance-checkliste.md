# 04 — MiCA-Compliance-Checkliste (Yeet / YEET)

**Status: ENTWURF** · Spalten „Owner"/„Status" bei Bearbeitung pflegen.
Legende: ☐ offen · ◐ in Arbeit · ☑ erledigt

## A. Strategische Vorentscheidungen (blockieren alles Weitere)

| ☐ | Aufgabe | Owner | Status |
| --- | --- | --- | --- |
| ☐ | Tokenisierungsstrategie festlegen: Verkauf? Listing? nur Rewards? | Gründer | offen |
| ☐ | Ledger-Modell entscheiden (non-custodial / Punkte-Modell / CASP) — Assessment §4 | Gründer + Anwalt | offen |
| ☐ | Rechtsträger + Sitzland bestätigen (→ zuständige NCA: BaFin/FMA/…) | Gründer | offen |
| ☐ | Kryptorechtliche Kanzlei mandatieren, Assessment validieren | Gründer | offen |

## B. Sofort umsetzbar (unabhängig von A)

Funktionsanpassungen F1–F8 sind in [06-leitplanken-validierung.md](06-leitplanken-validierung.md) §8 begründet.

| ☐ | Aufgabe | Owner | Status |
| --- | --- | --- | --- |
| ☐ | Status quo einfrieren: kein Token-Verkauf, keine projektseitigen Liquidity-Pools, kein Listing bis Freigabe | Alle | offen |
| ☐ | Token-Marketing-Regeln: fair, klar, nicht irreführend; keine Rendite-Versprechen; Kennzeichnung als Werbung | Marketing | offen |
| ☑ | **F4** — fiktiven YEET-Kurs (hartcodiert) aus dem Header entfernt; „Utility-Token · kein Marktpreis" statt Fake-Preis | Dev | erledigt (dieser PR) |
| ☑ | **F5** — irreführenden Fake-NFT-Mint (Junk-Tx + „NFT Minted!") deaktiviert; „NFT/verkaufen"-Wording entschärft | Dev | erledigt (dieser PR) |
| ☐ | Chain-ID-Inkonsistenz klären (Login/Mint 0x61 Testnet vs. Reward-Minting Chain 56) | Dev | offen |
| ☐ | **F8** — Token-Contract-Härtung: Supply-Cap, Mint nur Multisig, Owner-Funktionen minimiert; Audit | Dev | offen |
| ☐ | **F6** — Sanktions-Screening der Zieladressen vor jedem Batch-Mint | Dev | offen |
| ☐ | Dokumentation Off-Chain-Ledger ↔ On-Chain-Token (Anspruch, Umtausch, Auszahlung) | Dev + Anwalt | offen |

## B2. Umsetzung Non-Custodial-Modell (nach Strategie-/Rechtsfreigabe — F1–F3, F7)

| ☐ | Aufgabe | Owner | Status |
| --- | --- | --- | --- |
| ☑ | **Punkte-Modell** — `yeet_token_balance` = Punkte; Rewards gutgeschrieben statt auto-gemintet; Auto-Mint der Engagement-Rewards gestoppt (Migration 0038) | Dev | erledigt (dieser PR) |
| ☑ | **One-way Conversion** — `POST /api/v1/points/convert` (Punkte→YEET an verifizierte externe Wallet, kein Rückweg) | Dev | erledigt (dieser PR) |
| ☑ | **Eigene Wallets raus** — Frontend generiert keine Wallet/Seed mehr bei Registrierung; Backend ignoriert `wallet_address` beim Register; Wallet nur noch via Link-Flow | Dev | erledigt (dieser PR) |
| ☐ | **F1/F2** — On-Chain-YEET-Zahlungen strikt Wallet↔Wallet (nach Contract-Deploy) | Dev + Anwalt | offen |
| ☐ | **F3** — Paper Wallets als On-Chain-Escrow (kein Admin-Sweep, nicht upgradeable) + Betrags-/Rate-Limits; Alt-Ledger einfrieren | Dev | offen |
| ☐ | **F7** — PPV-Verbraucher-Consent (Widerrufsrecht) + AGB (Account-/Punkteübertragungsverbot) | Dev + Anwalt | offen |
| ◐ | `YeetPayments`- und `PaperWalletEscrow`-Contracts entwickeln + externes Audit | Dev | **Design + Contract-Sourcen + Tests erstellt** (`contracts/src`, Doc 07); Compile/Audit offen |
| ☐ | Contracts kompilieren (`forge build`) + Tests grün (`forge test`) — in dieser Umgebung nicht möglich (forge/OZ fehlen) | Dev | offen |
| ☑ | `Deploy.s.sol` um `YeetPayments` + `PaperWalletEscrow` erweitern | Dev | erledigt (dieser PR) |
| ☐ | Nach Deploy: Ownership → Multisig übertragen (Ownable2Step, transferOwnership + acceptOwnership) | Dev | offen |
| ☑ | Backend-Indexer geplant + Migration 0037 (idempotente Event-Verarbeitung); Rust-Skeleton in Doc 08 | Dev | Design erledigt; Wiring nach Contract-Deploy |
| ☐ | Chain-ID-Inkonsistenz behoben (Frontend `window.YEET_CHAIN` + Backend `YEET_CHAIN_ID`, Default Mainnet) | Dev | erledigt (dieser PR) |
| ☐ | WalletConnect v2 + injected Provider (MetaMask/Trust) für Auszahlung & On-Chain-Tips | Dev | offen |
| ☐ | Conversion-Flow Punkte→YEET über bestehende Batch-Mint-Infrastruktur | Dev | offen |

## C. Bei Angebot/Listing (auslösende Ereignisse: Sale, CEX/DEX-Listing, Pool)

| ☐ | Aufgabe | Owner | Status |
| --- | --- | --- | --- |
| ☐ | Whitepaper finalisieren (Dokument 02, alle ⟦Platzhalter⟧) | Anwalt + Gründer | offen |
| ☐ | Nachhaltigkeitsindikatoren erheben (CCRI/ESMA-Methodik, gewählte Chain) | Dev | offen |
| ☐ | iXBRL-Fassung erstellen (ESMA-RTS maschinenlesbares Format) | Anwalt/Dienstleister | offen |
| ☐ | NCA-Notifizierung ≥ 20 Arbeitstage vor Veröffentlichung (Art. 8) | Anwalt | offen |
| ☐ | Whitepaper auf Website veröffentlichen; ESMA-Register | Dev | offen |
| ☐ | Widerrufsprozess für Kleinanleger (14 Tage, Art. 13) implementieren | Dev | offen |
| ☐ | Haftungs-/Interessenkonflikt-Disclosures (10 %-Tip-Cut!) | Anwalt | offen |

## D. Falls CASP-Weg gewählt wird (Ledger bleibt custodial)

| ☐ | Aufgabe | Owner | Status |
| --- | --- | --- | --- |
| ☐ | CASP-Zulassungsantrag Titel V (oder White-Label-Partner) | Anwalt | offen |
| ☐ | Eigenmittel-/Governance-Anforderungen, Verwahrkonzept, Trennung Kundenvermögen | Gründer | offen |
| ☐ | Beschwerdemanagement, Interessenkonflikt-Policy, Auslagerungsregeln | Gründer | offen |
| ☐ | AML/KYC-Programm (Geldtransfer-VO/TFR-Anforderungen bei Transfers) | Anwalt | offen |

## E. Laufend

| ☐ | Aufgabe | Intervall |
| --- | --- | --- |
| ☐ | Whitepaper-Änderungen bei wesentlichen Neuerungen notifizieren (Art. 12) | anlassbezogen |
| ☐ | Nachhaltigkeitsdaten aktualisieren | jährlich |
| ☐ | Level-2/Level-3-Maßnahmen (ESMA/EBA-RTS & Guidelines) beobachten | quartalsweise |
| ☐ | Dieses Dokumenten-Set bei Strategieänderung fortschreiben | anlassbezogen |
