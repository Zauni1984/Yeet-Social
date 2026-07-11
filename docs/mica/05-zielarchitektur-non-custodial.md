# 05 — Zielarchitektur: Non-Custodial-Modell (Punkte + Direktauszahlung)

**Status: ENTWURF — vom Gründer skizzierte Zielarchitektur, MiCA-Bewertung dazu.**
**Kein Rechtsrat — anwaltliche Validierung erforderlich (Checkliste A).**

## 1. Das Modell

1. **E-Mail-Only-Nutzer** verdienen ausschließlich **Punkte** (interner DB-Ledger,
   kein Kryptowert). Tips/PPV innerhalb der Plattform laufen in Punkten.
2. **Umwandlung Punkte → YEET** ist jederzeit möglich, aber nur mit verifizierter
   Self-Custody-Wallet: Auszahlung wird **direkt on-chain an die Nutzer-Wallet** gemintet
   (bestehende `batchMintRewards`-Infrastruktur). Die Plattform hält nie Nutzer-Token.
3. **Wallet-Nutzer** (MetaMask, Trust Wallet, Atomic, WalletConnect) tippen/zahlen
   **direkt on-chain aus der eigenen Wallet** (Smart Contract mit 90/10-Split).
4. **Paper Wallets** werden zu **On-Chain-Escrow-Gutscheinen**: Ausstellung nur aus einer
   verbundenen Nutzer-Wallet, Einlösung direkt auf die Wallet des Empfängers.

## 2. MiCA-Bewertung

| Baustein | Einstufung | Begründung |
| --- | --- | --- |
| Punkte (DB-Ledger, nur verdienbar) | **Außerhalb MiCA** | Kein Kryptowert i. S. v. Art. 3 Abs. 1 Nr. 5 — zentrale DB ist keine DLT/„ähnliche Technologie". Da nur verdient (nie gegen Geld ausgegeben), auch kein E-Geld |
| Tips/PPV in Punkten | **Außerhalb MiCA** | Folge aus Zeile 1 — keine Verwahrung, kein Transferdienst |
| Umwandlung Punkte → YEET (Mint auf Nutzer-Wallet) | **Kein CASP-Dienst** | Emittent schüttet eigene Token aus; keine Verwahrung (Plattform hält nie), kein Transfer „für Kunden", kein Tausch (Punkte sind weder Kryptowert noch Geld). Titel-II-Analyse wie bei Rewards (Assessment §3) bleibt: kein Kauf → vermutlich kein öffentliches Angebot; Whitepaper-Bereitschaft empfohlen |
| On-Chain-Tips aus Nutzer-Wallet (Contract-Split 90/10) | **Kein CASP-Dienst** | Nutzer signiert selbst; Plattform = Software-Anbieter ohne Kontrolle über Fremdgelder; Fee-Empfang ist unschädlich |
| PPV-Zahlung in YEET an die Plattform | **Kein CASP-Dienst** | Annahme von Krypto als Zahlungsmittel für eigene Dienste ist nicht MiCA-reguliert |
| Paper Wallet als Escrow-Contract | **Kein CASP-Dienst**, wenn Leitplanke L5 gilt | Contract verwahrt, nicht die Plattform; kein Admin-Zugriff auf escrowte Mittel |

**Ergebnis: Das CASP-Kernrisiko aus Assessment §4 ist in diesem Modell beseitigt.**
Übrig bleibt die (unveränderte, beherrschbare) Titel-II-Frage der Token-Ausschüttung —
abgedeckt durch Whitepaper-Bereitschaft (Dokument 02) und die Auslöser-Checkliste (04 C).

## 3. Leitplanken — Bedingungen, an denen die Bewertung hängt

| # | Leitplanke | Warum |
| --- | --- | --- |
| L1 | **Punkte sind nie käuflich** (weder Fiat noch Krypto), nur verdienbar | Käufliche, einlösbare Punkte → E-Geld-/Angebots-Risiko |
| L2 | **Einbahnstraße:** nur Punkte → YEET. Kein Rückweg (YEET einzahlen → Punkte/Guthaben) | Annahme von Kundenkrypto = Verwahrung → CASP wieder da |
| L3 | **Kein Fiat-On-/Off-Ramp durch die Plattform** (kein Kauf/Verkauf YEET↔EUR) | Tausch Krypto↔Geld = CASP-Dienst (Art. 3 Abs. 1 Nr. 16 lit. c) |
| L4 | **Auszahlung nur an signaturverifizierte Adressen** (EIP-191-Nachweis, wie Wallet-Login) | Stützt Non-Custodial-Charakter, verhindert Fehl-/Fremdauszahlungen |
| L5 | **Escrow-Contract ohne Plattform-Admin-Zugriff** auf gelockte Mittel (Claim nur mit Secret, Refund nur an Aussteller, ggf. Timelock) | Admin-Sweep-Rechte = faktische Verwahrung |
| L6 | Punkte-Preislogik neutral kommunizieren (kein „1 Punkt = X €", keine Renditeversprechen) | Marketing-Regeln Art. 7 ff.; Vermeidung E-Geld-/Wertpapier-Optik |
| L7 | Umwandlungsverhältnis Punkte→YEET transparent + änderbar vorbehalten dokumentieren | Erwartungsmanagement, Haftungsminimierung |

## 4. Technische Umsetzung (Skizze)

### 4.1 Punkte statt Off-Chain-YEET
- `users.yeet_token_balance` → semantisch zu **Punkten** umwidmen (Anzeige „Punkte",
  nicht „YEET"); Tips/PPV-Flows bleiben technisch identisch, buchen aber Punkte.
- Conversion-Endpoint `POST /api/v1/points/convert`: bucht Punkte ab und legt einen
  Eintrag in `token_rewards` an → bestehender Batch-Mint-Job zahlt on-chain aus.
  Voraussetzung: verifizierte Wallet-Adresse am Konto (L4).

### 4.2 Wallet-Verknüpfung für E-Mail-Nutzer
- Flow „Wallet verbinden zum Auszahlen": injected Provider (MetaMask) + **WalletConnect v2**
  (deckt Trust Wallet u. v. m. ab) → Adresse per EIP-191-Signatur nachweisen
  (Infrastruktur existiert: `link-wallet/nonce` + `link-wallet/verify`).
- **Atomic Wallet:** dApp-Connect eingeschränkt — für reine Auszahlung genügt
  Adresse + Signaturnachweis über die Wallet-eigene Sign-Funktion; Connect-Integration
  ist nur für On-Chain-Tips nötig. TODO(verify): Atomic-Sign-Message-Support prüfen.

### 4.3 On-Chain-Tips & PPV
- Contract `YeetPayments`: `tip(address creator, uint256 amount)` und
  `unlock(bytes32 postId, uint256 amount)` via `approve/transferFrom`,
  Split 90/10 on-chain, Events (`Tipped`, `Unlocked`).
- Backend-Indexer liest Events (Confirmations abwarten) und schaltet PPV frei /
  aktualisiert Zähler. Kein Server-Key mit Zugriff auf Nutzer-Mittel.

### 4.4 Paper Wallets (On-Chain-Escrow)
- Contract `PaperWalletEscrow`:
  - `create(bytes32 secretHash, uint256 amount, uint64 expiry)` — lockt YEET des Ausstellers
  - `claim(bytes secret, address recipient)` — prüft `keccak256(secret) == secretHash`,
    zahlt an `recipient`
  - `refund(id)` — nach `expiry`, **nur an den Aussteller**
  - **kein** `adminWithdraw` (L5)
- Ausstellung nur mit verbundener Wallet; QR enthält Secret wie bisher.
- E-Mail-Only-Nutzer können Paper Wallets **einlösen**, sobald sie eine Wallet verknüpft
  haben (Claim zahlt direkt an ihre Adresse) — Ausstellung erfordert eigene Wallet.
- Hinweis (Checkliste): Inhaberpapier-Charakter → AML-Optik; Betragsobergrenze je
  Gutschein erwägen. TODO(verify): anwaltlich mitprüfen.

### 4.5 Migration
- Bestehende Off-Chain-Guthaben: einmalige Wahl je Nutzer — als Punkte weiterführen
  oder (mit Wallet) sofort auszahlen. Stichtag + Kommunikation.
- Danach: kein YEET-Guthaben mehr in der DB, nur Punkte + On-Chain.

## 5. Auswirkungen auf die übrigen Dokumente

- **Assessment §4:** Option (a) ist hiermit konkretisiert; Optionen (b)/(c) entfallen,
  wenn dieses Modell beschlossen wird. → Bei Beschluss Assessment aktualisieren.
- **Whitepaper Teil F:** „Off-Chain-Guthaben"-Passus ersetzen durch Punkte-/Conversion-
  Beschreibung (Punkte ausdrücklich als Nicht-Kryptowert abgrenzen).
- **Checkliste B:** Neue Punkte — Contracts (`YeetPayments`, `PaperWalletEscrow`)
  entwickeln + auditieren; Conversion-Flow; WalletConnect-Integration; Migrationsplan.
