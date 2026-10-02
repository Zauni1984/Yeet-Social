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
