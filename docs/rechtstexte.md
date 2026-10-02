# Rechtstexte (Impressum, Datenschutz, AGB, Cookies, Disclaimer)

Die Rechtstexte liegen in `frontend/index.html` als fünf `<article>`-Blöcke pro Sprache:
`legal-<slug>` (Englisch) und `legal-<slug>-de` (Deutsch). Slugs: `imprint`, `privacy`,
`terms`, `cookies`, `disclaimer`; URLs `/legal/<slug>`.

- Angezeigt wird die Sprache der Oberfläche (Deutsch → deutsche Fassung, alle anderen
  Sprachen → englische Fassung). Der Umschalter „Deutsch | English" oben auf der Seite
  überschreibt das und wird in `localStorage.yeet_legal_lang` gemerkt.
- Die deutsche Fassung wurde aus der englischen übersetzt (Impressum nach § 5 DDG statt
  § 5 TMG, DSGVO-Artikel identisch). **Vor dem offiziellen Bezug (MiCA-Whitepaper,
  Token-Start) anwaltlich prüfen lassen** – insbesondere Impressum, Datenschutzerklärung
  (Auftragsverarbeiter, Speicherfristen) und Nutzungsbedingungen (Punkte/Token-Klauseln).
- Änderungen immer in **beiden** Fassungen nachziehen und das „Stand"-Datum aktualisieren.

## Pay-per-View: Verbraucher-Consent und Widerrufsbelehrung (F7)

Seit Oktober 2026 verlangt die App vor jedem Pay-per-View-Kauf ein
ausdrückliches Verlangen auf sofortige Bereitstellung und die Kenntnisnahme,
dass das Widerrufsrecht damit erlischt (§ 356 Abs. 5 BGB, Art. 16 lit. m
RL 2011/83/EU). Technik: `PPV_CONSENT_VERSION` (Frontend + Backend),
gespeichert auf `ppv_unlocks` (Version, Zeitpunkt, Sprache); Bestätigung per
E-Mail (`send_ppv_confirmation`) und Beleg in der App. Der Consent-Text liegt
wie die AGB nur auf Deutsch und Englisch vor (`PPV_CONSENT_TEXT` in
`frontend/index.html`). **Anwaltlich zu prüfen:** Wortlaut des Consent-Texts,
die Widerrufsbelehrung in AGB §6 (DE) und ob ein Muster-Widerrufsformular
verlinkt werden muss. Textänderungen → `PPV_CONSENT_VERSION` erhöhen.

## AGB §1/§6: Punkte-Modell statt „Verwahrverbindlichkeit“ (Prüfpunkt A1)

Bis zum 2. Oktober 2026 beschrieben AGB §1 und §6 (EN/DE) ein „verwahrendes
Off-Chain-Kontobuch“ und nannten Off-Chain-Guthaben eine „Verwahrverbindlichkeit der
BlockSocial UG“. Das stammte aus der Zeit vor dem Punkte-Modell (docs/mica/05),
widersprach §3 derselben AGB („Punkte haben keinen Geldwert“) und ist genau die
Verwahr-Formulierung, die docs/mica/01 §4 als Kernrisiko (CASP-Zulassungspflicht)
benennt. Beide Fassungen sind jetzt auf das tatsächliche System umgeschrieben
(Abläufe: docs/mica/09):

- **§1:** YEET liegt stets nur in einer vom Nutzer kontrollierten Wallet; daneben ein
  Punkteguthaben im Konto; Punkte sind Plattform-Punkte, kein Kryptowert.
- **§6 „Punkte“:** nur verdienbar (nicht gegen Geld/Krypto erwerbbar), kein Kryptowert,
  kein E-Geld, keine Einlage, kein Geldwert, keine Auszahlung in Geld, nicht übertragbar
  (Verweis §3); Verwendung in der App; manipulationssichere Protokollierung;
  Änderungs-/Einstellungsvorbehalt für die Zukunft mit angemessener Ankündigung,
  bereits erworbene Punkte bleiben nutzbar.
- **§6 „YEET-Token“:** keine Verwahrung/Verwaltung/Kontrolle für Nutzer; Mint direkt in
  die Nutzer-Wallet; kein Verkauf, kein Tausch, keine Annahme von Kryptowerten gegen
  Punkte (Leitplanken L1/L2).
- **§6 „Umwandlung“:** einseitig, ganze Punkte, min. 100, derzeit 1 Punkt = 1 YEET,
  Änderung nur prospektiv mit Vorankündigung (L7); Prüfung vor Freigabe, Sammelausführung,
  Sanktionslisten-Abgleich der Zieladresse (F6), Vorbehalt des Umwandlungs-Kontingents,
  keine zugesicherte Bearbeitungszeit; Rückbuchung bei Ablehnung/Nichtausführung; kein
  Anspruch auf Geldzahlung.
- Nebenänderungen: Plattformgebühr-Klausel nennt jetzt auch den PPV-Kauf; Paper-Wallet-
  Klausel spricht von „gesperrten Punkten“ (Gutscheine sperren derzeit Punkte, nicht
  Token); Überschrift §6 „Punkte, Trinkgelder, …“; „Stand“-Datum beider Fassungen
  aktualisiert.

**Anwaltlich zu prüfen:** Wortlaut und AGB-Kontrolle (§§ 305c, 307 BGB: Änderungs- und
Einstellungsvorbehalt, Umwandlungsvorbehalte, Transparenzgebot), ob die Zusicherung
„bereits erworbene Punkte bleiben nutzbar“ gewollt ist, und ob eine Verfall-/
Verjährungsregel für Punkte ergänzt werden soll (derzeit kein Verfall implementiert).
Die Liste der juristischen Prüfpunkte A1–A9 steht in docs/mica/09 §10.
