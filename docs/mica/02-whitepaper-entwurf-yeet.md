# 02 — Krypto-Whitepaper (ENTWURF): YEET Token

**Status: ENTWURF / NICHT NOTIFIZIERT / NICHT VERÖFFENTLICHT.**
Gerüst nach MiCA Titel II (Art. 6 i. V. m. Anhang I) für "andere Kryptowerte".
Platzhalter `⟦…⟧` erst füllen, wenn die Tokenisierungsstrategie final ist.
Vor Veröffentlichung: NCA-Notifizierung (mind. 20 Arbeitstage vorher, Art. 8),
maschinenlesbares Format (iXBRL gemäß ESMA-RTS), Übermittlung ans ESMA-Register.

---

## Pflichthinweise (Art. 6 Abs. 3, 5, 6 MiCA — wörtlich aufzunehmen)

> Dieses Krypto-Whitepaper wurde von keiner zuständigen Behörde eines Mitgliedstaats der
> Europäischen Union gebilligt. Der Anbieter des Kryptowerts trägt die alleinige
> Verantwortung für den Inhalt dieses Krypto-Whitepapers.

> Dieser Kryptowert kann seinen Wert ganz oder teilweise verlieren, ist möglicherweise
> nicht immer übertragbar und möglicherweise nicht liquide.

> Der Kryptowert fällt nicht unter die Anlegerentschädigungssysteme nach der
> Richtlinie 97/9/EG und nicht unter die Einlagensicherungssysteme nach der
> Richtlinie 2014/49/EU.

**Erklärung des Leitungsorgans (Art. 6 Abs. 6):** ⟦Das Leitungsorgan von ⟦Rechtsträger⟧
bestätigt, dass dieses Whitepaper den Anforderungen des Titels II MiCA entspricht und
dass die darin enthaltenen Informationen nach bestem Wissen redlich, eindeutig und nicht
irreführend sind und keine wesentlichen Auslassungen enthalten.⟧

**Datum der Notifizierung:** ⟦TT.MM.JJJJ⟧ · **Version:** 0.1-ENTWURF

## Zusammenfassung (Art. 6 Abs. 7)

⟦Kurze, allgemein verständliche Zusammenfassung: Warnhinweis, dass die Zusammenfassung
als Einleitung zu lesen ist; Kaufentscheidung nur auf Basis des gesamten Whitepapers.⟧
- Token: **YEET** (Ticker `YEET`, Contract-Adresse ⟦0x… nach Deployment⟧), Utility-Token
  der Social-Plattform Yeet (justyeet.it), auf der **BNB Smart Chain (BEP-20)**.
- **Feste Höchstmenge: 21.000.000.000 YEET (21 Mrd.)** — kein weiteres Minting nach
  Initial-Supply (fixe Obergrenze, kein „Uncapped").
- Funktionen: Trinkgelder (Tips), Pay-per-View-Freischaltungen, Promotionen, Rewards,
  Einweg-Umtausch von Plattform-Punkten in YEET.
- Angebotstyp: **keine öffentliche Emission/Sale** — YEET wird ausschließlich als
  Aktivitäts-Reward zugeteilt bzw. gegen Plattform-Punkte ausgezahlt; der Note→YEET-Swap
  läuft über eine **eigene Swap-Seite auf justyeet.it** (siehe Eckdaten). Marktpreisbildung nur an externen Handelsplätzen.
- Vollständige Kennzahlen: siehe **Teil J — Tokenomics-Eckdaten**.

## Teil A — Angaben zum Anbieter (bzw. zur Person, die die Zulassung beantragt)

| Feld | Angabe |
| --- | --- |
| Name / Rechtsform | ⟦TODO(verify): Rechtsträger, z. B. GmbH⟧ |
| Eingetragene Anschrift / Sitz | ⟦…⟧ |
| Registernummer / LEI | ⟦…⟧ |
| Kontakt (E-Mail, Website) | ⟦…⟧ / https://justyeet.it |
| Leitungsorgan | ⟦Namen, Funktionen⟧ |
| Finanzlage der letzten 3 Jahre | ⟦bzw. seit Gründung⟧ |

## Teil B — Angaben zum Emittenten (falls vom Anbieter verschieden)

⟦Entfällt, falls identisch — sonst analog Teil A. Hinweis: Minting erfolgt derzeit durch
die Plattform via `batchMintRewards`; Emittent = Betreiber-Rechtsträger.⟧

## Teil C — Angaben zum Betreiber der Handelsplattform (nur bei Admission to Trading)

⟦TODO(strategie): Nur ausfüllen, falls ein Listing angestrebt wird.⟧

## Teil D — Das Kryptowert-Projekt

- **Projektname:** Yeet Social — Web3-Social-Media-Plattform (ephemere 24h-Posts,
  permanente Posts, Live-Streams, verschlüsselte DMs).
- **Zweck des Tokens:** In-App-Ökonomie: Tips an Creator (90/10-Split zugunsten des
  Creators), Pay-per-View-Inhalte, Promotion/Boosts, Aktivitäts-Rewards.
- **Beteiligte Personen:** ⟦Team/Advisors⟧
- **Meilensteine (vergangen/geplant):** ⟦Roadmap⟧
- **Mittelverwendung** (bei Sale): TODO(strategie)

## Teil E — Öffentliches Angebot / Zulassung zum Handel

| Feld | Angabe |
| --- | --- |
| Art | Kein öffentliches Angebot / kein Sale — Zuteilung nur als Reward bzw. Punkte-Auszahlung (Whitepaper freiwillig; Einstufung als Utility-Token angestrebt) |
| Emissionsvolumen / Höchstmenge | **21.000.000.000 YEET, fix** (kein weiteres Minting). Aufteilung: 10 % Developer (2,1 Mrd.), 10 % Team (2,1 Mrd.), 5 % Reserve (1,05 Mrd.), Rest 75 % (15,75 Mrd.) Rewards-/Community-Pool inkl. Registrierungs-Bonus, Posting-Rewards und Note-Swap-Pool. Details: Teil J. |
| Preis / Preisermittlung | Keine Zeichnung/kein Ausgabepreis; unentgeltliche Zuteilung als Reward. Marktpreis ergibt sich ausschließlich aus Angebot/Nachfrage an externen Handelsplätzen — Yeet hat keinen Einfluss. |
| Zeichnungsfrist, Zielgruppe, Mitgliedstaaten | ⟦…⟧ |
| Widerrufsrecht (Art. 13) | 14 Tage für Kleinanleger bei Direkterwerb ⟦anpassen⟧ |
| Verwahrung der eingenommenen Mittel | ⟦…⟧ |

## Teil F — Der Kryptowert: Rechte und Pflichten

- **Rechte:** Nutzung innerhalb der Plattform (Tips, PPV, Promotion). **Keine** Dividenden-,
  Stimm-, Rückzahlungs- oder sonstigen Ansprüche gegen den Rechtsträger. ⟦prüfen/ergänzen⟧
- **Übertragbarkeit:** ⟦TODO(strategie): frei übertragbar on-chain? Einschränkungen?⟧
- **Bedingungen für Funktionsänderungen:** ⟦Governance/Upgrade-Prozess des Contracts⟧
- **Off-Chain-Guthaben (Punkte-Modell):** Aktivität auf der Plattform wird in internen
  **Punkten** geführt und angezeigt (Yeet betreibt **keine** eigene Wallet). Punkte lassen
  sich **1:1 und ausschließlich in eine Richtung** in On-Chain-YEET umtauschen
  (Points → YEET), Auszahlung an eine vom Nutzer **selbst verwahrte, signaturverifizierte
  externe Wallet**. Ein Rücktausch YEET → Punkte ist ausgeschlossen. Yeet nimmt zu keinem
  Zeitpunkt Kundengelder oder -Token in Verwahrung (non-custodial). Voraussetzung für die
  Auszahlung: abgeschlossene KYC. Alle Transaktionen sind öffentlich im Blockchain-Explorer
  einsehbar. **Auszahlungen werden in der Startphase manuell von einem Admin
  freigegeben** (Betrugs-/Missbrauchsschutz); ohne Freigabe erfolgt keine
  On-Chain-Zahlung, eine Ablehnung erstattet die Punkte. Automatisierte Regeln
  ersetzen diese manuelle Freigabe später.

## Teil G — Zugrunde liegende Technologie

- **Netzwerk:** **BNB Smart Chain (BEP-20)**; Konsens: Proof of Staked Authority
  (~45 aktive Validatoren). Nutzer benötigen für On-Chain-Aktionen (u. a. Swap) etwas
  **BNB zur Deckung der Netzwerkgebühren** (Beispielgröße ~0,01 BNB); spätere Anpassungen
  des Fee-Managements sind möglich.
- **Smart Contract:** Adresse ⟦0x…⟧, Standard BEP-20/ERC-20, Minting-Funktion
  `batchMintRewards(address[], uint256[], string[])`, Mint-Berechtigung: ⟦Rollen/Multisig⟧
- **Audits:** ⟦TODO(verify): Contract-Audit beauftragen/verlinken⟧

## Teil H — Risiken

⟦Projektspezifisch ausformulieren; Gerüst:⟧
1. Angebots-/Emittentenrisiken (Abhängigkeit vom Fortbestand der Plattform)
2. Marktrisiken (Volatilität, fehlende Liquidität, kein Marktpreis solange kein Listing)
3. Technische Risiken (Smart-Contract-Fehler, Chain-Ausfälle, Schlüsselverlust)
4. Verwahrrisiken (internes Guthaben vs. Self-Custody; Plattform-Insolvenz)
5. Regulatorische Risiken (MiCA-Einstufung, künftige Level-2/Level-3-Maßnahmen)

## Teil I — Nachhaltigkeitsangaben (Art. 6 Abs. 1 lit. j, ESMA-RTS)

Pflichtindikator: **jährlicher Gesamtenergieverbrauch des Konsensmechanismus (kWh)**;
liegt dieser über 500.000 kWh/Jahr, zusätzlich u. a. Anteil erneuerbarer Energien,
Energieintensität je Transaktion, THG-Emissionen.

| Indikator | Wert | Quelle |
| --- | --- | --- |
| Energieverbrauch p. a. (kWh) | ⟦TODO(verify): CCRI/ESMA-Methodik für gewählte Chain⟧ | ⟦…⟧ |
| Erneuerbaren-Anteil | ⟦falls > 500 MWh⟧ | ⟦…⟧ |
| Energieintensität / Tx | ⟦…⟧ | ⟦…⟧ |
| THG-Emissionen (Scope-Angabe) | ⟦…⟧ | ⟦…⟧ |

→ Vorbereitete Daten je Chain-Kandidat: siehe [03-chain-assessment.md](03-chain-assessment.md).

## Teil J — Tokenomics-Eckdaten

> Stand: Eckdaten festgelegt; Smart Contract wird erst nach Finalisierung geschrieben.
> Der Note→YEET-Swap läuft über eine **eigene Seite auf justyeet.it** (`/swap.html`);
> Swaps sind bis zur Fertigstellung des Smart Contracts gesperrt (Start wird angekündigt).
> Technisches Design: `docs/swap-note-to-yeet.md`.

### Token

| Feld | Wert |
| --- | --- |
| Name / Ticker | Yeet Token / `YEET` |
| Chain / Standard | BNB Smart Chain, BEP-20 |
| Dezimalstellen | 18 (BEP-20-Standard) |
| **Höchstmenge** | **21.000.000.000 YEET — FIX**, kein weiteres Minting nach Initial-Supply |
| Verwahrung | non-custodial — Yeet hält niemals Wallets/Token für Kunden |

### Verteilung des Initial-Supply (21 Mrd.)

| Empfänger | Anteil | Menge | Hinweis |
| --- | --- | --- | --- |
| Developer | 10 % | 2.100.000.000 | **Vesting empfohlen** (z. B. 12 Mon. Cliff + linear 24–48 Mon., on-chain) |
| Team | 10 % | 2.100.000.000 | **Vesting empfohlen**; Team verzichtet auf alte Note-Coins → kein Team-Swap |
| Reserve | 5 % | 1.050.000.000 | Liquidität, Listings, Notfälle |
| Rewards-/Community-Pool | 75 % | 15.750.000.000 | speist Registrierungs-Bonus, Posting-Rewards, Note-Swap-Pool u. a. |

### Punkte & Rewards (on-platform)

| Regel | Wert |
| --- | --- |
| Anzeige | Alle Guthaben werden als **Punkte** dargestellt; keine Wallet auf der Plattform |
| Punkte → YEET | **1 Punkt = 1 YEET**, Einweg, Auszahlung nur an selbst verwahrte, verifizierte Wallet; KYC erforderlich |
| YEET → Punkte | **nicht möglich** |
| Registrierungs-Bonus | **1.000 YEET** für die **ersten 100.000** Registrierungen nach Double-Opt-in **und** KYC — **gebührenfrei** (max. 100.000.000 YEET) |
| Posting-Reward | **10 YEET** pro Artikel ≥ 120 Zeichen; **Tages-Cap 1.000 Punkte/User** (später anpassbar) |
| Tips | implementiert; **90/10-Split** zugunsten des Creators |

### Gebühren (Plattform)

| Quelle | Gebühr |
| --- | --- |
| Tips, NFT-Verkäufe, sonstige Transaktionen | **10 % Plattform-Anteil** |
| Registrierungs-Bonus | **0 % (Ausnahme, gebührenfrei)** |
| Netzwerk | BNB-Gasgebühren trägt der Nutzer; ~0,01 BNB (Beispiel) für Swap; spätere Fee-Anpassungen möglich |

### Note → YEET Swap (eigene Seite auf justyeet.it)

| Feld | Wert |
| --- | --- |
| Kurs | **100 Note-Coins : 1 YEET** (100 Note → 1 YEET) |
| Umsetzung | **eigene öffentliche Swap-Seite** auf justyeet.it (`/swap.html`), Einweg-Deposit-Bridge über eigenen Note-Fullnode (Design: `docs/swap-note-to-yeet.md`); Swaps gesperrt bis zum Contract-Start, Start wird überall angekündigt. Netzwerkgebühren (NOTE-Tx, BNB-Gas) trägt der User. |
| Team-Coins | Team verzichtet auf zuvor geminte Note-Coins → **kein Swap alter Team-Coins** |
| Note-Menge (Schätzung) | bis zu **50.000.000.000 Note** (⟦TODO(verify): exakte swap-berechtigte Menge⟧) |
| Swap-Pool | max. **500.000.000 YEET** (50 Mrd. Note ÷ 100) — ca. **2,4 % der 21 Mrd.**, aus dem Rewards-/Community-Pool. Reicht selbst bei vollständigem Swap aller Note-Coins. |

### Reserve-/Drain-Schutz & Auszahlungsfreigabe

Punkte können nicht „leerlaufen" (sie werden je Reward neu erzeugt) — endlich ist
der **On-Chain-Auszahlungs-Pool** (Rewards-/Community-Anteil). Damit dieser nicht
einseitig entleert wird:

| Mechanismus | Wirkung |
| --- | --- |
| **Manuelle Auszahlungsfreigabe** | Jede Points→YEET-Auszahlung wird zunächst als „wartet auf Freigabe" eingereiht; erst nach Admin-Freigabe zahlt der Minter aus. Ablehnung erstattet die Punkte. (Startphase; später regelbasiert.) |
| **Fee-Recycling** | Die 10 % Plattform-Gebühren fließen als Reserve zurück in den effektiven Pool — ein aktives Ökosystem füllt nach, was Auszahlungen abziehen. |
| **Drain-Guard** | Auszahlungen, die den verbleibenden Pool übersteigen würden, werden abgelehnt (`CONVERSION_POOL_EXHAUSTED`). |
| **Reward-Taper** | Sinkt der Pool unter einen Schwellwert (Standard 10 %), wird die Reward-Ausgabe automatisch reduziert (Standard-Faktor 0,5). |
| **Transparenz** | Öffentlicher Pool-Status via `GET /api/v1/tokens/pool` (Basis, recycelte Fees, ausgezahlt, Rest). |

Alle Parameter sind per Env konfigurierbar (`YEET_CONVERSION_POOL`,
`YEET_TAPER_THRESHOLD_PCT`, `YEET_TAPER_FACTOR`).

### Offene Punkte (nicht Teil dieses Whitepapers)

- Smart Contract (BEP-20, feste 21-Mrd.-Obergrenze, Vesting-Verträge) — **wird später geschrieben, wenn alles final ist**.
- Rechtliche MiCA-Prüfung + NCA-Notifizierung; Einstufung als Utility-Token ist **angestrebt, nicht bestätigt** — kein „fully compliant"-Claim vor Rechtsgutachten.
- Etwaige Auszahlungs-/Fee-Anpassungen (Points → YEET) noch nicht festgelegt.

## Anhang: Interessenkonflikte, anwendbares Recht, Beschwerdeweg

⟦Interessenkonflikte (Plattform-Cut bei Tips!), zuständige NCA, Beschwerdeverfahren,
anwendbares Recht/Gerichtsstand.⟧
