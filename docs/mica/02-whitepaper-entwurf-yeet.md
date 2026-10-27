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

**Datum der Notifizierung:** ⟦TT.MM.JJJJ⟧ · **Version:** 0.2-ENTWURF (Stand 2026-10-27:
Contract-Rollen, Off-Chain-Belohnungssystem und Testnet-Phase eingetragen)

## Zusammenfassung (Art. 6 Abs. 7)

⟦Kurze, allgemein verständliche Zusammenfassung: Warnhinweis, dass die Zusammenfassung
als Einleitung zu lesen ist; Kaufentscheidung nur auf Basis des gesamten Whitepapers.⟧
- Token: **YEET** (Ticker `YEET`, Contract-Adresse ⟦0x… nach Mainnet-Deployment⟧), Utility-Token
  der Social-Plattform Yeet (justyeet.it), auf der **BNB Smart Chain (BEP-20)**.
  **Derzeit Testphase auf dem BNB-Smart-Chain-Testnet (Chain-ID 97):** dort ausgegebene
  Test-YEET haben keinen Wert und werden nicht auf das Mainnet übertragen (Teil J).
- **Feste Höchstmenge: 21.000.000.000 YEET (21 Mrd.)** — im Contract als `MAX_SUPPLY`
  fixiert (kein „Uncapped"). 25 % (Developer/Team/Reserve) werden beim Deploy ausgegeben;
  die 75 %-Rewards-Tranche wird ausschließlich bei Punkte→YEET-Umwandlungen bis zur
  Obergrenze gemintet; eine andere Mint-Funktion existiert nicht. Minten darf nur eine
  einzelne **Minter-Rolle** (Backend der Plattform), begrenzt durch ein **Tageslimit**; der
  Eigentümer des Contracts kann das Minting jederzeit anhalten, selbst aber nicht minten.
- **Ein Contract, sonst nichts on-chain:** Die gesamte Plattform-Ökonomie (Rewards, Tips,
  Pay-per-View, Gutscheine) läuft als **Punkte** in einem geprüften Off-Chain-Journal der
  Plattform. Die Blockchain wird nur berührt, wenn ein Nutzer Punkte in YEET umwandelt;
  die YEET gehen dann direkt in seine eigene Wallet. Die Plattform verwahrt nichts.
- Funktionen: Trinkgelder (Tips), Pay-per-View-Freischaltungen, Promotionen, Rewards,
  Einweg-Umtausch von Plattform-Punkten in YEET.
- Angebotstyp: **keine öffentliche Emission/Sale** — YEET wird ausschließlich als
  Aktivitäts-Reward zugeteilt bzw. gegen Plattform-Punkte ausgezahlt; der Note→YEET-Swap
  läuft über eine **eigene Swap-Seite auf justyeet.it** (siehe Eckdaten). Marktpreisbildung nur an externen Handelsplätzen.
- Vollständige Kennzahlen: siehe **Teil J — Tokenomics-Eckdaten**.

## Teil A — Angaben zum Anbieter (bzw. zur Person, die die Zulassung beantragt)

| Feld | Angabe |
| --- | --- |
| Name / Rechtsform | BlockSocial UG (haftungsbeschränkt) |
| Eingetragene Anschrift / Sitz | Kratzmühlstr. 14, 92339 Beilngries, Deutschland |
| Registernummer / LEI | ⟦…⟧ |
| Kontakt (E-Mail, Website) | ⟦…⟧ / https://justyeet.it |
| Leitungsorgan | ⟦Namen, Funktionen⟧ |
| Finanzlage der letzten 3 Jahre | ⟦bzw. seit Gründung⟧ |

## Teil B — Angaben zum Emittenten (falls vom Anbieter verschieden)

⟦Entfällt, falls identisch — sonst analog Teil A.⟧ Hinweis: Das Minting erfolgt durch das
Backend der Plattform über die Contract-Rolle `minter` (Funktion `batchMintRewards`),
ausschließlich zur Erfüllung von Punkte→YEET-Umwandlungen; Emittent = Betreiber-Rechtsträger
(BlockSocial UG). Contract-Eigentümer (`owner`) ist ⟦bis zur Multisig-Übergabe der Deployer-Schlüssel
des Betreibers / danach die Multisig ⟦Adresse⟧⟧.

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
| Emissionsvolumen / Höchstmenge | **21.000.000.000 YEET, fix** (Contract-Obergrenze; Rewards-Tranche wird schrittweise bis zur Obergrenze gemintet, darüber hinaus kein Minting). Aufteilung: 10 % Developer (2,1 Mrd.), 10 % Team (2,1 Mrd.), 5 % Reserve (1,05 Mrd.), Rest 75 % (15,75 Mrd.) Rewards-/Community-Pool inkl. Registrierungs-Bonus, Posting-Rewards und Note-Swap-Pool. Details: Teil J. |
| Preis / Preisermittlung | Keine Zeichnung/kein Ausgabepreis; unentgeltliche Zuteilung als Reward. Marktpreis ergibt sich ausschließlich aus Angebot/Nachfrage an externen Handelsplätzen — Yeet hat keinen Einfluss. |
| Zeichnungsfrist, Zielgruppe, Mitgliedstaaten | ⟦…⟧ |
| Widerrufsrecht (Art. 13) | 14 Tage für Kleinanleger bei Direkterwerb ⟦anpassen⟧ |
| Verwahrung der eingenommenen Mittel | ⟦…⟧ |

## Teil F — Der Kryptowert: Rechte und Pflichten

- **Rechte:** Nutzung innerhalb der Plattform (Tips, PPV, Promotion). **Keine** Dividenden-,
  Stimm-, Rückzahlungs- oder sonstigen Ansprüche gegen den Rechtsträger. ⟦prüfen/ergänzen⟧
- **Übertragbarkeit:** On-Chain-YEET sind ein gewöhnlicher BEP-20-Token und **frei
  übertragbar**; der Contract enthält keine Transfersperren, Blacklists oder Transfergebühren,
  und der Eigentümer kann Transfers nicht anhalten (nur das Minting). ⟦Anwalt: Wording
  prüfen.⟧ **Punkte** dagegen sind nicht übertragbar und nicht handelbar; sie können innerhalb
  der Plattform verwendet (Tips, Pay-per-View, Gutscheine) oder einmalig in YEET umgewandelt
  werden.
- **Bedingungen für Funktionsänderungen:** Der Token-Contract ist **nicht upgradefähig**
  (kein Proxy); seine Regeln (Höchstmenge, Tranchen, Mint-Beschränkung) sind unveränderlich.
  Der Eigentümer hat genau drei Stellschrauben: Minter-Adresse wechseln, Tageslimit setzen,
  Minting anhalten/fortsetzen; jede davon löst ein öffentliches Event aus. Änderungen des
  **Umwandlungsverhältnisses** Punkte→YEET gelten nur für künftige Umwandlungen und müssen
  mindestens **14 Tage vorher** angekündigt werden (technisch erzwungen, Teil J; AGB §6).
  Änderungen der Reward-Regeln (Punkte je Aktivität, Tageskappe) werden im öffentlichen
  Changelog der Plattform angekündigt. ⟦Anwalt: Übergangsregel für bereits angesammelte Punkte.⟧
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
  ersetzen diese manuelle Freigabe später. Weitere Bedingungen: Mindestmenge **100 Punkte**
  je Umwandlung; die Zielwallet wird gegen öffentliche **Sanktionslisten** geprüft
  (Treffer → keine Auszahlung); Umwandlungen sind auf den verbleibenden Rewards-Pool
  begrenzt (Teil J, Drain-Schutz). Punkte **verfallen derzeit nicht**. ⟦Anwalt: Rechtsnatur
  des Punkteguthabens (Leistungsanspruch ohne Geldwert), Verjährung/Verfall, Behandlung
  bei Account-Löschung und Plattform-Einstellung — vgl. 09-ledger-und-auszahlung.md §10.⟧

## Teil G — Zugrunde liegende Technologie

- **Netzwerk:** **BNB Smart Chain (BEP-20)**; Konsens: Proof of Staked Authority
  (~45 aktive Validatoren). Nutzer benötigen für On-Chain-Aktionen (u. a. Swap) etwas
  **BNB zur Deckung der Netzwerkgebühren** (Beispielgröße ~0,01 BNB); spätere Anpassungen
  des Fee-Managements sind möglich.
- **Smart Contract `YeetToken`** (Quelle: `contracts/src/YeetToken.sol`, Solidity 0.8.24,
  OpenZeppelin 5.1.0, 54 automatisierte Tests): Standard BEP-20/ERC-20 mit `ERC20Burnable`
  und `ERC20Permit`, 18 Dezimalstellen, **nicht upgradefähig**.
  - Adresse Testnet (Chain-ID 97): ⟦0x… nach Testnet-Deployment⟧ · Adresse Mainnet
    (Chain-ID 56): ⟦0x… nach Mainnet-Deployment⟧.
  - **Höchstmenge** `MAX_SUPPLY` = 21 Mrd.; 25 % beim Deploy an den Eigentümer; die
    **75 %-Rewards-Reserve** (`REWARD_RESERVE`, 15,75 Mrd.) ist die einzige nach dem
    Deploy mintbare Menge, kumulativ gezählt (`rewardsMinted`) — Burns öffnen sie nicht
    wieder. Eine andere Mint-Funktion existiert nicht.
  - **Rollen:** `owner` (Ownable2Step; Ziel: Multisig) setzt Minter, Tageslimit und Pause,
    mintet aber **nie selbst**. `minter` (ein Backend-Schlüssel der Plattform) kann
    ausschließlich `batchMintRewards(address[], uint256[], string[])` aufrufen: max. 200
    Empfänger je Aufruf, Summe ≤ Rewards-Reserve, Summe ≤ **Tageslimit** `dailyMintCap`
    pro UTC-Tag, kein Null-Empfänger, atomar (ein Fehler verwirft den ganzen Aufruf).
  - **Not-Aus:** `setMintingPaused(true)` stoppt das Minting; Transfers der Nutzer sind
    davon **nie** betroffen.
  - **Ereignisse** (öffentlich nachvollziehbar): `RewardMinted`, `MinterUpdated`,
    `DailyMintCapUpdated`, `MintingPaused`.
  - Bewusst **nicht** enthalten: Verkaufs-/Presale-Logik, Transfergebühren, Blacklist,
    Proxy/Upgrade, Zugriff des Betreibers auf Nutzerguthaben, Vesting (separat, Teil J).
- **Weitere Contracts:** Keine. Zahlungs-, Escrow- und NFT-Contracts existieren als
  geprüfte Entwürfe im Quellcode, werden aber **nicht deployt**, solange Zahlungen als
  Punkte abgewickelt werden (siehe 10-testnet-contract.md).
- **Off-Chain-Komponente:** Punkte und deren Bewegungen werden in einem append-only
  **Transaktionsjournal mit Hash-Kette** geführt (jede Buchung referenziert den Hash der
  vorherigen); Saldo und Journal werden regelmäßig automatisch abgeglichen; Nutzer können
  ihr eigenes Journal einsehen und exportieren. Details: Teil J und 09-ledger-und-auszahlung.md.
- **Betriebsphase:** Zuerst **BNB Smart Chain Testnet (Chain-ID 97)**; die Umstellung auf das
  Mainnet (Chain-ID 56) erfolgt nach externem Audit und Multisig-Übergabe mit einem neuen
  Deployment (10-testnet-contract.md §6).
- **Audits:** ⟦TODO(verify): externes Contract-Audit beauftragen/verlinken (Commit-Hash der
  geprüften Fassung angeben)⟧; interner Code-Audit des Backends vom 2026-10-03
  (`docs/audit-2026-10-03.md`).

## Teil H — Risiken

⟦Projektspezifisch ausformulieren; Gerüst mit den bereits bekannten Punkten:⟧
1. Angebots-/Emittentenrisiken (Abhängigkeit vom Fortbestand der Plattform; Punkte sind ein
   Anspruch gegen die Plattform, kein Kryptowert — bei Einstellung der Plattform ⟦Regelung⟧)
2. Marktrisiken (Volatilität, fehlende Liquidität, kein Marktpreis solange kein Listing;
   Yeet betreibt keine Liquidity-Pools)
3. Technische Risiken: Smart-Contract-Fehler (Audit ausstehend), Chain-Ausfälle, Verlust des
   Nutzer-Wallet-Schlüssels (Auszahlungen sind endgültig, kein Rückruf möglich),
   **Kompromittierung des Minter-Schlüssels** (Schaden auf höchstens ein Tageslimit der
   Rewards-Reserve begrenzt; Eigentümer kann pausieren und den Schlüssel wechseln),
   Fehlkonfiguration (falsche Zielwallet durch den Nutzer)
4. Verwahrrisiken (internes Punkteguthaben vs. Self-Custody der YEET; Plattform-Insolvenz;
   Verlust der Journaldaten — tägliche Backups, 10-jährige Aufbewahrung)
5. Operative Risiken des Belohnungssystems: Reward-Taper bei niedrigem Pool, Ablehnung
   oder Verzögerung einer Umwandlung (manuelle Freigabe, Sanktionsprüfung, Tageslimit),
   Änderung des Umwandlungsverhältnisses mit 14 Tagen Vorlauf
6. **Testnet-Phase:** Test-YEET auf der Testkette haben keinen Wert und keine Beziehung zum
   späteren Mainnet-Token; Testdaten können zurückgesetzt werden
7. Regulatorische Risiken (MiCA-Einstufung, künftige Level-2/Level-3-Maßnahmen)

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

> Stand 2026-10-27: Eckdaten festgelegt und im Smart Contract `YeetToken` umgesetzt
> (Teil G); Deploy **zuerst auf dem BNB-Smart-Chain-Testnet**, Mainnet nach Audit und
> Multisig-Übergabe. Der Note→YEET-Swap läuft über eine **eigene Seite auf justyeet.it**
> (`/swap.html`); Swaps sind bis zum Mainnet-Start gesperrt (Start wird angekündigt).
> Technisches Design: `docs/swap-note-to-yeet.md`.

### Token

| Feld | Wert |
| --- | --- |
| Name / Ticker | Yeet Token / `YEET` |
| Chain / Standard | BNB Smart Chain, BEP-20 |
| Dezimalstellen | 18 (BEP-20-Standard) |
| **Höchstmenge** | **21.000.000.000 YEET — FIX** (`MAX_SUPPLY`); 25 % beim Deploy, 75 % Rewards-Tranche nur per `batchMintRewards` bis zur Obergrenze (`REWARD_RESERVE`), keine weitere Mint-Funktion |
| Mint-Berechtigung | nur die Rolle `minter` (Backend-Schlüssel), begrenzt durch Tageslimit `dailyMintCap` (Startwert ⟦1.000.000 YEET/Tag⟧, vom Eigentümer anpassbar); Eigentümer kann Minting pausieren und den Minter wechseln, mintet nie selbst |
| Eigentümer (`owner`) | Testnet: Deployer-Schlüssel des Betreibers; Mainnet: **Multisig** ⟦Adresse, Signer, Schwelle⟧ (zweistufige Übergabe) |
| Upgradefähigkeit | keine (kein Proxy); Regeln sind nach Deploy unveränderlich |
| Verwahrung | non-custodial — Yeet hält niemals Wallets/Token für Kunden |
| Netzwerk-Phase | **Testnet (Chain-ID 97)** bis ⟦Datum/Meilenstein⟧; danach Mainnet (Chain-ID 56) mit neuer Contract-Adresse; Test-YEET werden nicht migriert |

### Verteilung des Initial-Supply (21 Mrd.)

| Empfänger | Anteil | Menge | Hinweis |
| --- | --- | --- | --- |
| Developer | 10 % | 2.100.000.000 | **Vesting empfohlen** (z. B. 12 Mon. Cliff + linear 24–48 Mon., on-chain) |
| Team | 10 % | 2.100.000.000 | **Vesting empfohlen**; Team verzichtet auf alte Note-Coins → kein Team-Swap |
| Reserve | 5 % | 1.050.000.000 | Liquidität, Listings, Notfälle |
| Rewards-/Community-Pool | 75 % | 15.750.000.000 | speist Registrierungs-Bonus, Posting-Rewards, Note-Swap-Pool u. a. |

### Off-Chain-Belohnungssystem (Punkte) — so läuft es ohne Blockchain

Alle Aktivität wird in **Punkten** geführt. Punkte sind ein internes Guthaben der Plattform
(kein Kryptowert, keine Wallet, nicht übertragbar). Jede Bewegung wird in derselben
Datenbanktransaktion in ein **append-only Journal mit Hash-Kette** geschrieben; Saldo und
Journal werden automatisch abgeglichen. Das System benötigt keine Blockchain-Verbindung —
die Chain wird ausschließlich bei der Auszahlung einer Umwandlung berührt (nachgewiesen
in 10-testnet-contract.md §1).

**Punkte entstehen** (Stand 2026-10-27; Werte per Konfiguration anpassbar, Änderungen
werden im Changelog angekündigt):

| Pfad | Punkte | Bedingung |
| --- | --- | --- |
| Artikel veröffentlicht | **10** | Text ≥ 120 Zeichen; Entwürfe/Kurzposts erhalten nichts |
| Kommentar | **1** | sichtbarer Post |
| Reshare | **2** | einmal je Post, nur Live-/öffentliche Posts |
| Tageskappe | **max. 1.000 P/Tag und Nutzer** | für alle Aktivitäts-Rewards zusammen |
| Reward-Taper | ×0,5 | sobald der Auszahlungs-Pool unter 10 % seines Basiswerts fällt |
| Registrierungs-Bonus | **1.000 P**, einmalig | E-Mail-Double-Opt-in **und** Alters-/KYC-Verifizierung; nur die ersten 100.000 Nutzer (max. 100.000.000 P); vom Taper ausgenommen |
| Tip erhalten / Pay-per-View-Erlös | 90 % des Betrags | 10 % Plattformgebühr |
| Gutschein (Paper Wallet) eingelöst | gesperrter Betrag | Einlösung per Claim-Code |

(Im Code vorgesehene, derzeit **nicht aktive** Regeln: Like 1 P, täglicher Login 2 P; der
frühere NFT-Mint-Reward ist deaktiviert.)

**Punkte werden verwendet** (in-app, ohne Chain):

| Verwendung | Abbuchung | Gegenbuchung | Gebühr |
| --- | --- | --- | --- |
| Tip an Creator (Post, Profil, DM, Live) | −X | Creator +0,9 X | 0,1 X |
| Pay-per-View-Freischaltung | −Preis, erst nach ausdrücklicher Zustimmung (Consent-Protokoll) | Autor +0,9 Preis | 0,1 Preis |
| Paper-Wallet-Gutschein | Aussteller −Betrag (gesperrt, stornierbar bis zur Einlösung) | Einlöser +Betrag | — |
| Live-Promotion | −Preis | — (Plattformleistung; Erstattung bei Absage) | 100 % |

**Umwandlung Punkte → YEET** (der einzige Weg aus dem Punktesystem):

| Regel | Wert |
| --- | --- |
| Verhältnis | **1 Punkt = 1 YEET** (versioniert; Änderungen nur für künftige Umwandlungen, **≥ 14 Tage** vorab angekündigt, öffentlich unter `GET /api/v1/points/rate`) |
| Richtung | **Einweg** — YEET → Punkte ist nicht möglich |
| Mindestmenge | **100 Punkte** je Umwandlung |
| Zielwallet | ausschließlich eine vom Nutzer **selbst verwahrte**, per Signatur verknüpfte externe Wallet; Sanktionslisten-Prüfung |
| Voraussetzung | abgeschlossene KYC/Altersverifikation |
| Ablauf | Punkte sofort abgebucht → „wartet auf Freigabe" → Admin-Freigabe → Batch-Mint durch die Minter-Rolle (≤ 200 Empfänger je Transaktion, Tageslimit) → Transaktions-Hash im Journal; Ablehnung erstattet die Punkte |
| Transparenz | eigenes Journal (Ansicht/Export), öffentlicher Pool-Status, Explorer-Link je Auszahlung |

### Punkte & Rewards — Kurzfassung

| Regel | Wert |
| --- | --- |
| Anzeige | Alle Guthaben werden als **Punkte** dargestellt; keine Wallet auf der Plattform |
| Punkte → YEET | **1 Punkt = 1 YEET**, Einweg, min. 100 Punkte, Auszahlung nur an selbst verwahrte, verifizierte Wallet; KYC erforderlich |
| YEET → Punkte | **nicht möglich** |
| Registrierungs-Bonus | **1.000 Punkte** für die **ersten 100.000** Registrierungen nach Double-Opt-in **und** KYC — **gebührenfrei** (max. 100.000.000 Punkte) |
| Posting-Reward | **10 Punkte** pro Artikel ≥ 120 Zeichen (Kommentar 1, Reshare 2); **Tages-Cap 1.000 Punkte/User** (anpassbar) |
| Tips | implementiert (Punkte); **90/10-Split** zugunsten des Creators |

### Gebühren (Plattform)

| Quelle | Gebühr |
| --- | --- |
| Tips, Pay-per-View | **10 % Plattform-Anteil** (in Punkten erfasst, fließt dem Auszahlungs-Pool zu — Fee-Recycling) |
| Live-Promotion | 100 % Plattformleistung |
| Registrierungs-Bonus, Punkte→YEET-Umwandlung | **0 % (gebührenfrei)**; Netzwerkgebühr des Mints trägt die Plattform (Minter-Wallet) |
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
| **On-Chain-Tageslimit** | Der Contract lässt der Minter-Rolle pro UTC-Tag höchstens `dailyMintCap` zu; was nicht mehr ins Tagesbudget passt, wartet automatisch auf den nächsten Tag. Ein kompromittierter Minter-Schlüssel kann damit höchstens ein Tageslimit minten. |
| **Not-Aus** | Der Eigentümer kann das Minting jederzeit anhalten (`setMintingPaused`); Umwandlungen bleiben dann in der Warteschlange, Punkte gehen nicht verloren. Transfers der Nutzer sind nie betroffen. |
| **Sanktions-Screening** | Jede Zielwallet wird vor der Auszahlung gegen öffentliche Sanktionslisten geprüft (täglich aktualisiert); Treffer werden nicht ausgezahlt. |
| **Transparenz** | Öffentlicher Pool-Status via `GET /api/v1/tokens/pool` (Basis, recycelte Fees, ausgezahlt, Rest); On-Chain `rewardsRemaining()` und `dailyRemaining()`. |

Alle Backend-Parameter sind per Env konfigurierbar (`YEET_CONVERSION_POOL`,
`YEET_REWARD_RESERVE`, `YEET_TAPER_THRESHOLD_PCT`, `YEET_TAPER_FACTOR`); der effektive
Pool ist zusätzlich auf die On-Chain-Reserve gedeckelt, damit Gebühren-Recycling nie mehr
zusagt, als der Contract minten kann.

### Testnet-Phase und Mainnet-Umstellung

| Phase | Chain | Was gilt |
| --- | --- | --- |
| **Jetzt: Testnet** | BNB Smart Chain Testnet (97) | Punktesystem produktiv; Umwandlungen zahlen **Test-YEET ohne Wert** aus; Minter = Deployer zulässig; Zweck: Ablauf, Freigabe, Not-Aus und Explorer-Anzeige unter Realbedingungen prüfen |
| **Später: Mainnet** | BNB Smart Chain (56) | Voraussetzungen: externes Audit, Multisig als Eigentümer, Minter ≠ Deployer (Deploy-Skript erzwingt es), Tageslimit gesetzt; neue Contract-Adresse; Umwandlungen während der Umstellung pausiert; Test-YEET werden **nicht** migriert |

⟦TODO(strategie): Werden während der Testnet-Phase angesammelte **Punkte** zum Mainnet-Start
anerkannt (Umwandlung 1:1 in Mainnet-YEET) oder zurückgesetzt? Empfehlung: anerkennen — die
Punkte sind der Anspruch, die Test-YEET nur der Testlauf; vor Start in AGB und Changelog festlegen.⟧

### Offene Punkte (nicht Teil dieses Whitepapers)

- Smart Contract: `YeetToken` ist fertig (Minter-Rolle, Tageslimit, Not-Aus, 54 Tests), **externes Audit und Multisig-Übergabe stehen aus**; Vesting-Verträge für Developer/Team **fehlen noch** (die 25 % liegen nach dem Deploy ungesperrt beim Eigentümer). Entschieden: Rewards werden gemintet (nicht vorgemintet) — es gibt keinen Hot-Bestand des Pools in einer Wallet.
- Rechtliche MiCA-Prüfung + NCA-Notifizierung; Einstufung als Utility-Token ist **angestrebt, nicht bestätigt** — kein „fully compliant"-Claim vor Rechtsgutachten.
- Automatisierte Freigaberegeln für Umwandlungen (Betragsgrenzen, Kontoalter) als Ersatz der manuellen Freigabe; Höhe des Tageslimits für das Mainnet.
- Umgang mit Testnet-Punkten beim Mainnet-Start (siehe oben).

## Anhang: Interessenkonflikte, anwendbares Recht, Beschwerdeweg

⟦Interessenkonflikte (Plattform-Cut bei Tips!), zuständige NCA, Beschwerdeverfahren,
anwendbares Recht/Gerichtsstand.⟧
