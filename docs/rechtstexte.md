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
