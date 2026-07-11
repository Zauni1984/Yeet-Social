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
- Token: YEET (⟦Ticker/Contract-Adresse⟧), Utility-Token der Social-Plattform Yeet (justyeet.it)
- Funktionen: Trinkgelder (Tips), Pay-per-View-Freischaltungen, Promotionen, Rewards
- TODO(strategie): Angebotstyp (kein Verkauf / Sale / nur Rewards) hier zusammenfassen

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
| Art | ⟦TODO(strategie): (i) kein öffentliches Angebot — Whitepaper freiwillig; (ii) Angebot; (iii) Admission⟧ |
| Emissionsvolumen / Höchstmenge | ⟦Total Supply, davon Rewards-Pool, Team, Treasury …⟧ |
| Preis / Preisermittlung | ⟦bzw. „unentgeltliche Zuteilung als Reward"⟧ |
| Zeichnungsfrist, Zielgruppe, Mitgliedstaaten | ⟦…⟧ |
| Widerrufsrecht (Art. 13) | 14 Tage für Kleinanleger bei Direkterwerb ⟦anpassen⟧ |
| Verwahrung der eingenommenen Mittel | ⟦…⟧ |

## Teil F — Der Kryptowert: Rechte und Pflichten

- **Rechte:** Nutzung innerhalb der Plattform (Tips, PPV, Promotion). **Keine** Dividenden-,
  Stimm-, Rückzahlungs- oder sonstigen Ansprüche gegen den Rechtsträger. ⟦prüfen/ergänzen⟧
- **Übertragbarkeit:** ⟦TODO(strategie): frei übertragbar on-chain? Einschränkungen?⟧
- **Bedingungen für Funktionsänderungen:** ⟦Governance/Upgrade-Prozess des Contracts⟧
- **Off-Chain-Guthaben:** ⟦TODO(strategie): Verhältnis internes Guthaben ↔ On-Chain-Token
  eindeutig definieren (Anspruch auf Auszahlung? Umtauschverhältnis?) — siehe Assessment §4⟧

## Teil G — Zugrunde liegende Technologie

- **Netzwerk:** ⟦TODO(strategie): BNB Smart Chain (BEP-20) — oder Alternative gem.
  Dokument 03⟧; Konsens: ⟦z. B. Proof of Staked Authority, ~45 Validatoren⟧
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

## Anhang: Interessenkonflikte, anwendbares Recht, Beschwerdeweg

⟦Interessenkonflikte (Plattform-Cut bei Tips!), zuständige NCA, Beschwerdeverfahren,
anwendbares Recht/Gerichtsstand.⟧
