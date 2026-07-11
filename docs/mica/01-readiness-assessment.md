# 01 — MiCA Readiness Assessment: Yeet Social & YEET Token

**Status: ENTWURF** · Rechtsgrundlage: Verordnung (EU) 2023/1114 (MiCA), vollständig
anwendbar seit 30.12.2024. **Kein Rechtsrat — anwaltliche Prüfung erforderlich.**

---

## 1. Ist-Zustand des Systems (aus dem Code abgeleitet)

| Merkmal | Befund (Code-Referenz) |
| --- | --- |
| Token | YEET, BEP-20 auf BNB Smart Chain (Mainnet, Chain-ID 56); Contract via `YEET_TOKEN_ADDRESS`, Minting über `batchMintRewards` (`backend/src/services/batch_rewards.rs`) |
| Token-Ausgabe | Plattform mintet Rewards für Nutzeraktionen (z. B. Post erstellt) gesammelt on-chain an die Wallet-Adressen der Nutzer |
| Interner Ledger | Off-Chain-Guthaben `users.yeet_token_balance`; Tips (mit 10 % Plattform-Cut, `backend/src/api/tips.rs`), Pay-per-View-Freischaltungen und Live-Tips werden **off-chain** gebucht |
| Paper Wallets | Gutschein-System: Server speichert nur den Hash eines Claim-Secrets; Einlösung schreibt internes Guthaben gut (`backend/src/api/paper_wallets.rs`) |
| Wallet-Login | Nutzer signieren mit eigener Wallet (EIP-191); private Schlüssel der Login-Wallets liegen **nicht** beim Server |
| NFT-Posts | Posts können als "NFT" mit YEET-Preis markiert werden |
| Kein Verkauf | TODO(verify): Es existiert kein Kauf-Flow (Fiat→YEET oder Krypto→YEET) im Code — bestätigen, dass auch außerhalb der App kein Verkauf durch das Projekt erfolgt |
| Kein Listing | TODO(verify): YEET ist auf keiner Handelsplattform (CEX oder DEX-Pool durch das Projekt) gelistet — bestätigen |

## 2. Token-Klassifizierung nach MiCA

- **Kein E-Geld-Token (EMT, Titel IV):** YEET ist an keine einzelne amtliche Währung gekoppelt.
- **Kein vermögenswertereferenzierter Token (ART, Titel III):** kein Stabilisierungs-
  mechanismus, keine Referenzwerte.
- **Ergebnis: "anderer Krypto-Wert" i. S. d. Art. 3 Abs. 1 Nr. 5 MiCA**, mit
  Utility-Token-Zügen (Art. 3 Abs. 1 Nr. 9: Zugang zu Diensten — Tips, PPV, Promotionen).
  → Maßgeblich ist **Titel II** (Art. 4–15).
- **NFT-Posts:** Einzigartige, nicht fungible Token sind nach Art. 2 Abs. 3 MiCA
  ausgenommen. Achtung: Bei Ausgabe in großen Serien oder bei Fraktionalisierung entfällt
  die Ausnahme (Erwägungsgrund 10 f.). TODO(strategie): Sobald die NFT-Funktion echte
  On-Chain-Mints bekommt, Serien-/Fungibilitätsdesign dokumentieren.

## 3. Whitepaper-Pflicht (Titel II) — Analyse

Die Pflicht zur Erstellung/Notifizierung eines Krypto-Whitepapers (Art. 4, 5, 6, 8) greift bei:
**(a)** einem öffentlichen Angebot in der EU oder **(b)** der Beantragung der Zulassung zum
Handel auf einer Handelsplattform.

### 3.1 Sind die Reward-Ausschüttungen ein "öffentliches Angebot"?

- Ein "öffentliches Angebot" setzt eine Mitteilung voraus, die Interessenten in die Lage
  versetzt, über den **Erwerb (Kauf)** zu entscheiden (Art. 3 Abs. 1 Nr. 12). Bei reinen
  Reward-Ausschüttungen wird nichts gekauft → gute Argumente, dass **kein Angebot** vorliegt.
- Die ausdrückliche Ausnahme für **kostenlose Abgabe** (Art. 4 Abs. 3 lit. a) ist jedoch
  **eng**: Ein Krypto-Wert gilt *nicht* als kostenlos, wenn Erwerber im Gegenzug
  **personenbezogene Daten** bereitstellen oder der Anbieter sonstige Vorteile erhält.
  Yeet-Nutzer registrieren sich (E-Mail/Wallet, IP), liefern Content und Engagement —
  genau die Konstellation, die ESMA-nahe Kommentierung als "nicht kostenlos" einstuft.
- **Bewertung:** Solange YEET weder verkauft noch gelistet wird, ist das Risiko einer
  Whitepaper-Pflicht **niedrig bis mittel** — es fehlt am Kauf. Verlassen sollte man sich
  auf die Gratis-Ausnahme allein aber nicht. **Empfehlung:** Whitepaper proaktiv
  vorbereiten (→ Dokument 02), Notifizierung erst bei Verkauf/Listing.

### 3.2 Ausnahmen, falls doch ein Angebot vorliegt (Art. 4 Abs. 2)

| Ausnahme | Anwendbar? |
| --- | --- |
| < 150 Personen je Mitgliedstaat | Nein (offene Plattform) |
| Gesamtgegenwert ≤ 1 Mio. € über 12 Monate | Möglich, solange **kein Verkauf** stattfindet (Gegenwert = 0). TODO(strategie): entfällt bei einem Token-Sale |
| Nur qualifizierte Anleger | Nein |
| Reward für Validierung/Mining (Art. 4 Abs. 3 lit. b) | Nein — Posting-Rewards sind keine Konsens-Rewards |
| Utility-Token für existierende Dienstleistung (Art. 4 Abs. 3 lit. c) | Prüfenswert: YEET gewährt Zugang zu existierenden Diensten (Tips/PPV/Promotion). TODO(verify): anwaltlich prüfen, ob die Ausnahme trägt |

### 3.3 Admission to Trading

- Ein Listing auf einer **CASP-betriebenen** Handelsplattform löst die Whitepaper-Pflicht
  aus (Art. 5) — auch wenn ein Dritter das Listing betreibt, gelten Informationspflichten.
- **Vollständig dezentrale** Protokolle (reine DEX-Pools ohne Intermediär) liegen nach
  Erwägungsgrund 22 außerhalb von MiCA — aber die Grenze ist umstritten.
  TODO(strategie): Vor jedem Liquiditäts-Pool-Deployment (z. B. PancakeSwap) anwaltlich
  prüfen; faktisch würde das Projekt damit die Handelbarkeit selbst herstellen.

### 3.4 Sonstige Pflichten auch ohne Whitepaper

- **Marketing-Kommunikation** (Art. 7–8): fair, klar, nicht irreführend; kennzeichnungspflichtig.
  Gilt praktisch für jede Bewerbung des Tokens, sobald ein Angebot/Listing im Raum steht.
- **Redlichkeitspflichten** (Art. 14): Handeln im besten Interesse der Inhaber,
  Interessenkonflikte offenlegen, Systeme/Sicherheit angemessen.

## 4. ⚠️ Kernrisiko: der interne YEET-Ledger (CASP-Analyse)

Yeet verwaltet Nutzer-Guthaben **off-chain** (`users.yeet_token_balance`) und führt darauf
Tips, PPV-Käufe und Paper-Wallet-Gutschriften aus. On-chain-Auszahlung erfolgt über das
Batch-Minting. Damit liegt funktional Folgendes nahe:

| Mögliche Kryptowerte-Dienstleistung (Art. 3 Abs. 1 Nr. 16) | Bezug zu Yeet | Risiko |
| --- | --- | --- |
| **Verwahrung und Verwaltung für Kunden** (lit. a) | Plattform kontrolliert die dem Guthaben zugrunde liegenden Token bzw. das Anrecht darauf | **Hoch** |
| **Transferdienstleistungen** (lit. j) | Tips = Übertragung von Kryptowerten zwischen Nutzern durch die Plattform | **Hoch** |
| Tausch Krypto/Geld bzw. Krypto/Krypto (lit. c, d) | Nur falls künftig Kauf/Verkauf angeboten wird | Latent |
| Betrieb einer Handelsplattform (lit. b) | Nicht gegeben | Niedrig |

**Wenn diese Dienste gewerblich in der EU erbracht werden, ist eine CASP-Zulassung
(Titel V, Art. 59 ff.) erforderlich** — mit Anforderungen an Eigenkapital, Governance,
Verwahrkonzept, Beschwerdemanagement etc. Das ist erheblich aufwendiger als ein Whitepaper.

**Strategische Optionen (TODO(strategie) — Entscheidung nötig):**
1. **Non-Custodial-Umbau:** Tips/PPV direkt on-chain aus der Nutzer-Wallet (Smart-Contract-
   Aufrufe, signiert vom Nutzer). Plattform hält nie Guthaben → CASP-Pflicht entfällt
   weitgehend. Technisch: Contract mit `tip()`/`unlock()`-Funktionen + 10 %-Fee-Split.
2. **Punkte-Modell:** Interner Ledger wird zu reinen, nicht übertragbaren und nicht
   auszahlbaren Plattform-Punkten (kein Kryptowert); On-Chain-YEET strikt getrennt.
3. **CASP-Zulassung anstreben** (bzw. Kooperation mit einem zugelassenen CASP/White-Label).

**→ Präferierte Richtung (Gründer-Skizze): Kombination aus 1 + 2 — E-Mail-Nutzer verdienen
Punkte, Umwandlung Punkte→YEET mit Direktauszahlung auf die Self-Custody-Wallet,
On-Chain-Tips aus der Nutzer-Wallet, Paper Wallets als On-Chain-Escrow.
Ausarbeitung + Bewertung + Leitplanken: [05-zielarchitektur-non-custodial.md](05-zielarchitektur-non-custodial.md).**

## 5. Emittenten-/Anbieterpflichten bei künftigem Angebot

Falls die Tokenisierungsstrategie einen Verkauf oder ein Listing vorsieht, gilt zusätzlich:
- **Rechtsträger erforderlich** (Art. 4 Abs. 1: Anbieter muss juristische Person sein).
  TODO(verify): Rechtsform/Sitz des Betreibers dokumentieren (bestimmt die zuständige NCA,
  z. B. BaFin bei DE-Sitz, FMA bei AT-Sitz).
- Whitepaper-Notifizierung an die NCA **mind. 20 Arbeitstage vor** Veröffentlichung (Art. 8).
- Veröffentlichung auf der Website, maschinenlesbar; Übermittlung ans ESMA-Register.
- **Widerrufsrecht** für Kleinanleger: 14 Tage bei Direkterwerb (Art. 13).
- Haftung für fehlerhafte Whitepaper-Angaben (Art. 15).

## 6. Ergebnis & Maßnahmenplan (priorisiert)

| # | Maßnahme | Priorität | Abhängigkeit |
| --- | --- | --- | --- |
| 1 | Entscheidung Ledger-Modell (Option 1/2/3 aus §4) | **Hoch** | Tokenisierungsstrategie |
| 2 | Bestätigen: kein Verkauf, kein Listing, keine Projekt-Liquidity-Pools (Status quo einfrieren, bis 1 entschieden) | **Hoch** | — |
| 3 | Whitepaper-Entwurf pflegen (Dokument 02), damit bei Strategiewechsel nur Platzhalter zu füllen sind | Mittel | — |
| 4 | Chain-Entscheidung inkl. Nachhaltigkeitsdaten (Dokument 03) | Mittel | 1 |
| 5 | Marketing-Guidelines für Token-Kommunikation (fair/klar/nicht irreführend, Kennzeichnung) | Mittel | — |
| 6 | Anwaltliche Validierung dieses Assessments (Kryptorecht, Sitzland) | **Hoch** | 1–2 |
| 7 | NFT-Design (Einzelstücke vs. Serie) dokumentieren, sobald On-Chain-NFTs kommen | Niedrig | Strategie |
