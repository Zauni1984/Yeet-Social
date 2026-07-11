# 06 — Adversariale Validierung der Leitplanken (L1–L7) + Funktionsanpassungen

**Status: ENTWURF.** Methode: Jede Leitplanke wird aus Sicht einer skeptischen Aufsicht
(BaFin/FMA/ESMA) bzw. gegnerischen Kanzlei angegriffen; danach Nachbarregime-Check.
**Dies simuliert eine fachanwaltliche Prüfung, ersetzt sie aber nicht** — die Mandatierung
einer Kanzlei bleibt Hoch-Prio-Punkt (Checkliste A). Ergebnis-Legende:
✅ hält · ⚠️ hält nur mit Verschärfung · ❌ hält nicht.

---

## 1. Angriff auf L1 — „Punkte nie käuflich"

**Angriffe:**
- *Indirekter Erwerb:* Gibt es Pfade, auf denen Geld/Krypto mittelbar zu Punkten wird
  (gekaufte Promotion-Pakete mit Punkte-Bonus, Cashback, bezahlte Verifizierung)?
  → Code-Audit: Punkte entstehen ausschließlich über `token_rewards`-Aktionen
  (Post erstellt etc.). Kein Kauf-Pfad vorhanden. Muss so bleiben.
- *Sekundärmarkt:* Account-Verkauf überträgt faktisch Punkte. Nicht der Plattform
  zurechenbar, aber AGB müssen Account-/Punkteübertragung verbieten.
- *Transferierbarkeit:* Tips in Punkten machen Punkte P2P-übertragbar. Ein aggressiver
  Prüfer fragt: übertragbar + in Kryptowert wandelbar ≈ Zahlungsinstrument? Analyse:
  **kein E-Geld** (nicht gegen Zahlung ausgegeben, § 1 Abs. 2 S. 3 ZAG / Art. 2 Nr. 2 EMD2
  setzt Ausgabe gegen Geldbetrag voraus), **kein Kryptowert** (keine DLT). Bleibt ein
  geschlossenes Bonussystem. Haltbar — solange L1 strikt gilt.

**Ergebnis L1: ⚠️ hält mit Verschärfung** →
**L1+**: Jeder neue Punkte-Gutschriftspfad durchläuft einen Compliance-Check (Checkliste E);
AGB-Verbot der Account-/Punkteübertragung; keine Punkte als Gegenleistung für Zahlungen
jeglicher Art (auch nicht „Bonuspunkte" bei künftigen Bezahl-Features).

## 2. Angriff auf L2 — „Einbahnstraße Punkte→YEET"

**Angriffe:**
- *Cross-Kohorten-Zahlungen:* Wallet-Nutzer will einen E-Mail-Only-Creator tippen oder
  dessen PPV-Inhalt kaufen. Wohin fließt das On-Chain-YEET? Jede Variante, in der die
  Plattform es „für den Creator aufbewahrt" (auch als Punkte-Gutschrift gegen
  YEET-Eingang!), ist **Annahme von Kundenkrypto** → Verwahrung, L2 gebrochen.
  ❌ in der bisherigen Skizze ungelöst.
- *Attestierter Escrow:* Tip in einen Claim-Contract, Freigabe nur mit Plattform-Signatur
  („gehört Account X") → Plattform kontrolliert Freigabe = verwahrungsähnlich. Verwerfen.

**Ergebnis L2: ⚠️ Lücke gefunden → Funktionsanpassung F1/F2 (siehe §8):**
On-Chain-YEET-Zahlungen (Tip, PPV, NFT-Kauf) sind **nur Wallet↔Wallet** möglich.
Ist der Empfänger nicht wallet-verknüpft, gibt es genau zwei zulässige Wege:
(a) Zahlung in **Punkten** (wenn der Zahler Punkte hat) oder (b) Feature ist für diese
Kombination **deaktiviert** („Creator muss zuerst eine Wallet verknüpfen").
Niemals: YEET annehmen und intern gutschreiben.

## 3. Angriff auf L3 — „kein Fiat-Ramp"

**Angriff:** Integration eines On-Ramp-Widgets (MoonPay & Co.) mit Order-Datenweitergabe
oder Revenue-Share → Nähe zu „Annahme und Übermittlung von Aufträgen"
(Art. 3 Abs. 1 Nr. 16 lit. i MiCA) bzw. nationaler Vermittlungstatbestände.

**Ergebnis L3: ⚠️ hält mit Verschärfung** →
**L3+**: Falls je ein Drittanbieter-Ramp verlinkt wird: reiner Link-out ohne
Auftragsdaten-Weitergabe, ohne Vorbefüllung von Beträgen, zunächst ohne Revenue-Share;
vorher anwaltlich freigeben.

## 4. Angriff auf L4 — „signaturverifizierte Auszahlungsadresse"

**Angriffe:**
- Adresswechsel nach Account-Übernahme → Auszahlung an Angreifer.
- Auszahlung an sanktionierte Adresse: Der Emittent ist zwar (ohne CASP-Dienste) kein
  GwG-Verpflichteter, aber **EU-Sanktionsrecht gilt für jedermann** — ein Transfer an
  eine gelistete Adresse wäre ein Sanktionsverstoß, unabhängig von MiCA.

**Ergebnis L4: ⚠️ hält mit Verschärfung** →
**L4+**: Adressänderung = Re-Verifizierung + 48h-Cooldown vor nächster Auszahlung +
E-Mail-Notice; Screening der Auszahlungsadressen gegen EU-/OFAC-Sanktionslisten vor
jedem Batch-Mint (F6); Auszahlungs-Log unveränderlich aufbewahren.

## 5. Angriff auf L5 — „Escrow ohne Admin-Zugriff"

**Angriffe:**
- *Upgradeable Proxy:* „Kein adminWithdraw" ist wertlos, wenn die Implementierung per
  Proxy austauschbar ist — Upgrade-Recht = faktische Verfügungsgewalt.
- *Pause-Funktion:* Ein Pause, das auch `claim`/`refund` blockiert, hält fremde Mittel
  auf unbestimmte Zeit fest → verwahrungsähnliche Kontrolle.

**Ergebnis L5: ⚠️ hält mit Verschärfung** →
**L5+**: Escrow- und Payment-Contracts **nicht upgradeable** (kein Proxy); falls Pause,
dann nur für `create` — `claim`/`refund` sind immer möglich; externes Audit vor Deploy;
Verzicht/Timelock auf Owner-Funktionen dokumentieren.

## 6. Angriff auf L6/L7 — Marketing & Umwandlungsverhältnis

**Befunde aus dem Code (kritisch):**
1. **Fiktiver Marktpreis:** Der Header zeigt einen hartcodierten YEET-Kurs
   (`_yState = {usd: .001, chg: 2.4}` → „$0.00100 / +2,4 %", `frontend/index.html`).
   Ein angezeigter Preis samt Kursänderung für einen Token **ohne Markt** ist
   irreführend (MiCA Art. 7 bei Angebotsnähe; jedenfalls UWG §§ 5, 5a).
   **❌ nicht haltbar → F4: entfernen** oder durch klar gekennzeichneten internen
   Referenzwert ersetzen („kein Marktpreis").
2. **„NFT"-Wording ohne NFT:** `mint_nft` ist ein Stub (`"NFT minting not yet available"`),
   `is_nft` ist ein reines DB-Flag; die UI wirbt aber mit „Sell as NFT / Als NFT
   verkaufen". **❌ irreführend → F5: Wording ändern** („Permanenter Post") bis echtes
   On-Chain-Minting existiert.
3. *Garantiertes Umtauschversprechen:* „Jederzeit 1:1 einlösbar" würde Punkte zu einem
   festen Anspruch machen und die Abgrenzung schwächen.

**Ergebnis L6/L7: ⚠️ →**
**L7+**: Umwandlung „innerhalb von Batch-Fenstern, Verhältnis anpassbar (nur prospektiv),
Mindestmenge, kein Rechtsanspruch auf einen bestimmten Gegenwert"; keine
Kursdarstellungen ohne echten, quellenbelegten Marktpreis.

## 7. Nachbarregime-Check (über MiCA hinaus)

| Regime | Ergebnis | Handlungsbedarf |
| --- | --- | --- |
| **MiFID II / WpPG** (Wertpapier?) | YEET: keine Gewinn-/Stimm-/Rückzahlungsrechte → kein Wertpapier/keine Vermögensanlage | Rechte-Ausschluss im Whitepaper Teil F fixieren |
| **EMD2 / ZAG** (E-Geld, Zahlungsdienste) | Punkte nicht gegen Zahlung ausgegeben → kein E-Geld; Plattform hält keine Gelder → kein Zahlungsdienst | L1 strikt halten |
| **GwG / TFR** | Ohne CASP-Dienste kein Verpflichteter; TFR (Travel Rule) gilt für CASPs | Freiwillig: Sanktions-Screening (L4+), Paper-Wallet-Limits (L10) |
| **EU-Sanktionsrecht** | Gilt unmittelbar für jedermann | F6 Screening vor Auszahlungen |
| **Verbraucherrecht (Fernabsatz)** | PPV = digitaler Inhalt; Widerrufsrecht erlischt nur mit ausdrücklicher Zustimmung + Kenntnisnahme (§ 356 Abs. 5 BGB) | F7: Consent-Text vor erstem PPV-Kauf |
| **UWG** | Fiktiver Kurs + „NFT"-Wording (s. o.) | F4, F5 |
| **DSGVO** | Wallet-Adressen = personenbezogene Daten; On-Chain-Daten unlöschbar | Datenschutzerklärung ergänzen (Hinweis: Auszahlung schreibt Adresse unwiderruflich on-chain); Grundlage Art. 6 Abs. 1 lit. b |
| **Gewinnspiel-/Glücksspielrecht** | Rewards ohne Einsatz → kein Glücksspiel | — |

## 8. Konsolidierte Funktionsanpassungen (F1–F8)

| # | Anpassung | Betroffener Code | Priorität |
| --- | --- | --- | --- |
| F1 | **Tips:** Standard = Punkte. On-Chain-YEET-Tip nur, wenn **beide** Seiten wallet-verknüpft; sonst Punkte oder Hinweis „Creator hat noch keine Wallet" | `backend/src/api/tips.rs`, `YeetPayments`-Contract, Frontend-Tip-Modal | Hoch |
| F2 | **PPV/Promotion/Live-Tips:** gleiche Wallet↔Wallet-Regel; kein YEET-Eingang wird jemals intern gutgeschrieben | `posts.rs (unlock)`, `lives.rs (tip_live, book_promotion)` | Hoch |
| F3 | **Paper Wallets:** On-Chain-Escrow gem. Doc 05 §4.4 + **Limits**: Obergrenze je Gutschein (TODO(strategie): z. B. 150 € Gegenwert), Rate-Limit je Aussteller, Ausstellungs-Log; Alt-System (interner Ledger) einfrieren und auslaufen lassen | `paper_wallets.rs`, `PaperWalletEscrow` | Hoch |
| F4 | **Preis-Widget entfernen/ersetzen:** kein fiktiver Kurs; erst wieder anzeigen, wenn belegbarer Marktpreis existiert (Quelle + Zeitstempel), sonst „Punkte"-Anzeige ohne Fiat-Wert | `frontend/index.html` (`yeetPriceWidget`, `_yState`, `_fetchEur`) | **Sofort** |
| F5 | **„NFT"-Wording ersetzen** durch „Permanenter Post", solange `mint_nft` Stub ist; i18n-Keys `nft.*` bereinigen | `frontend/index.html`, `posts.rs` | **Sofort** |
| F6 | **Sanktions-Screening** der Zieladressen vor jedem Batch-Mint; Treffer → Auszahlung blockieren + manueller Review | `batch_rewards.rs` (Pre-Mint-Hook) | Hoch |
| F7 | **PPV-Verbraucher-Consent** („sofortige Bereitstellung, Verlust des Widerrufsrechts") beim ersten Kauf; AGB ergänzen (auch Account-/Punkteübertragungsverbot aus L1+) | Frontend + AGB | Mittel |
| F8 | **Token-Contract-Härtung:** Supply-Cap/Emissionsplan, Mint nur Multisig, Owner-Funktionen minimiert — Voraussetzung für redliches Whitepaper Teil G | Solidity-Contract | Hoch |

## 9. Gesamtergebnis

- **Kein K.-o. für die Zielarchitektur.** Das Non-Custodial-Modell (Doc 05) bleibt der
  richtige Weg; L1, L3, L4, L5, L7 halten mit den Verschärfungen (+).
- **Eine echte Lücke wurde geschlossen:** Cross-Kohorten-Zahlungen (L2) hätten das
  Verwahrungsproblem durch die Hintertür wieder eingeführt → gelöst durch F1/F2
  (strikt Wallet↔Wallet oder Punkte, niemals interne YEET-Gutschrift).
- **Zwei Sofortmaßnahmen unabhängig von der Strategie:** F4 (fiktiver Kurs) und
  F5 („NFT"-Wording) — beide sind heute schon ein Irreführungsrisiko, ganz ohne MiCA.
- Verbleibende externe Validierung: die hier getroffenen Auslegungen (insb. „Punkte ≠
  Kryptowert", Escrow-Contract ≠ Verwahrung, Reward-Ausschüttung ≠ öffentliches Angebot)
  sind gut begründbar, aber nicht höchstrichterlich geklärt → Kanzlei-Review bleibt
  Pflichtpunkt, bevor Sale/Listing oder große Reichweite erreicht wird.
