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

### Technische Absicherung der Kursklausel (§6 „Umwandlung“)

Die Zusage „Änderung des Verhältnisses nur für künftige Umwandlungen, vorab angekündigt“
ist seit 3. Oktober 2026 im System erzwungen: Kurse liegen versioniert mit Gültig-ab-Datum
in `conversion_rates`; der Admin-Endpoint lehnt Änderungen mit weniger als
`YEET_RATE_NOTICE_DAYS` (14) Tagen Vorlauf ab; jede Umwandlung speichert den angewandten
Kurs; der Dialog zeigt angekündigte Änderungen mit Datum an (docs/mica/09 §6.3).
**Anwalt:** Ist eine 14-Tage-Frist angemessen, und genügt die Anzeige im Dialog plus
Changelog-Post als „Ankündigung“, oder ist eine E-Mail an alle Nutzer mit Punkteguthaben nötig?

## Datenschutzerklärung § 4.6/4.7, § 5, § 9, § 11: Altersverifikation und optionale Altersschätzung (2026-11-12)

Anlass: Die bisherige Fassung beschrieb die Altersverifikation als „in der Sitzung verarbeitete“
Aufnahme, von der nur das Ergebnis bleibt. Seit der Umstellung auf menschliche Prüfung werden
Gesichtsfoto und optionales Ausweisfoto verschlüsselt gespeichert (7 Tage nach Freigabe, 30 Tage
nach Ablehnung, sofort bei Rücknahme), und seit dem 12. November gibt es die optionale
KI-Altersschätzung als Hinweis für das Prüfpersonal. Geändert (DE und EN):

- § 4.6 beschreibt jetzt Speicherung, Verschlüsselung, Zugriff und Fristen der Fotos korrekt.
- Neuer § 4.7 „Altersschätzung (optional)“: gesonderte Einwilligung, Verarbeitung nur auf dem
  eigenen Server, Hinweis statt Entscheidung, Löschung mit dem Foto, Widerruf durch Rücknahme.
- § 5: zusätzlicher Zweck mit Rechtsgrundlage Art. 9 Abs. 2 lit. a DSGVO.
- § 9: Speicherdauer der Fotos und der Schätzwerte ergänzt.
- § 11: klargestellt, dass keine automatisierte Entscheidung stattfindet (Mensch entscheidet).
- Stand-Datum auf 12. November 2026.

Die zugehörige Datenschutz-Folgenabschätzung liegt als Entwurf unter
`docs/dsfa-altersverifikation.md`; offene organisatorische Punkte stehen dort in Abschnitt 6.

## 19. November 2026 — Umwandlung setzt Altersverifikation voraus

Nutzungsbedingungen § 6 (DE und EN), Absatz „Umwandlung von Punkten in YEET“: ergänzt um den
Satz, dass eine abgeschlossene Altersverifikation Voraussetzung ist. Hintergrund: Das Whitepaper
nennt die Verifikation seit jeher als Voraussetzung der Auszahlung; der Server hat sie bisher nur
beim NOTE-Swap geprüft und prüft sie jetzt auch bei `POST /api/v1/points/convert`
(Fehlercode `KYC_REQUIRED`).

Gleichzeitig veröffentlicht: der Leitfaden „Das YEET-Belohnungssystem“ (DE/EN) als PDF, verlinkt
im Footer. Er erklärt die Regeln, ersetzt die Nutzungsbedingungen aber nicht; Quelle unter
`docs/rewards/`, Erzeugung mit `node scripts/build-rewards-pdf.cjs`.

## 27. November 2026 — Mindestalter 18 statt 16

Nutzungsbedingungen, Abschnitt zur Nutzungsberechtigung (DE und EN): Der Dienst richtet sich
ausschließlich an Erwachsene; Mindestalter für Konto und Nutzung ist 18 Jahre (bisher 16). Der
Zugang zu 18+-Inhalten setzt weiterhin zusätzlich die Altersverifikation voraus. Stand-Datum der
Nutzungsbedingungen auf den 27. November 2026 gesetzt.

Technisch: Die Registrierung per E-Mail verlangt die Bestätigung „Ich bin mindestens 18 Jahre
alt“ (`age_confirmed`, Fehlercode `AGE_REQUIRED`); der Zeitpunkt steht in
`users.age_declared_at`. Bestehende Konten und Anmeldungen per Wallet haben keinen Eintrag; für
sie gilt die Erklärung aus den Nutzungsbedingungen.

Offen (Betreiber): ob bestehende Nutzer über die Änderung informiert werden und ob Konten, die
bei der Altersverifikation als minderjährig auffallen, gesperrt werden.

Zusammenhang: Vorbereitung der Android-App für Google Play (`docs/play-store.md`). Dort werden
18+-Inhalte gar nicht angezeigt.
