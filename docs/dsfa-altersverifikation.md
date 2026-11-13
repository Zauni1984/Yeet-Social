# Datenschutz-Folgenabschätzung (DSFA) — Altersverifikation mit optionaler Altersschätzung

**Verantwortlicher:** BlockSocial UG (haftungsbeschränkt), Betreiber von justyeet.it
**Verarbeitung:** 18+-Altersverifikation (Gesichtsfoto, optional Ausweisfoto, menschliche Prüfung)
und optionale KI-Altersschätzung als Hinweis für das Prüfpersonal
**Stand:** 2026-11-12 · **Status:** Entwurf zur Freigabe durch Geschäftsführung und
Datenschutzbeauftragte/n · **Nächste Überprüfung:** jährlich oder bei Änderung von Modell, Ablauf
oder Speicherdauer

Rechtsgrundlage für die DSFA: Art. 35 Abs. 3 lit. b DSGVO (umfangreiche Verarbeitung besonderer
Kategorien, Art. 9) sowie die DSK-Muss-Liste (biometrische Daten zur Identifizierung, Einsatz von
KI zur Bewertung natürlicher Personen).

## 1. Beschreibung der Verarbeitung

| | |
|---|---|
| **Zweck** | Nachweis der Volljährigkeit vor Zugang zu als 18+ gekennzeichneten Inhalten (JuSchG, JMStV); Vorsortierung der Prüf-Warteschlange zur schnelleren Bearbeitung |
| **Betroffene** | Nutzer, die freiwillig Zugang zu 18+-Inhalten beantragen (Opt-in) |
| **Daten** | Gesichtsfoto (biometrisch, Art. 9); optional Ausweisfoto (Identitätsdaten); Metadaten des Falls (Zeitpunkte, Status, Prüfvermerk, Einwilligungszeitpunkt); bei Einwilligung in die Schätzung: Altersklasse, Wahrscheinlichkeiten, Gesichts-Score, Anzahl Gesichter, Modellkennung |
| **Ablauf** | 1. Nutzer nimmt Foto im Browser auf, optional Ausweisfoto, optional Haken „Altersschätzung“ (Standard: aus). 2. Backend verschlüsselt jede Datei einzeln (AES-256-GCM, Schlüssel je Datei per HKDF aus `AGE_VERIFY_KEY`) und legt sie unter `PRIVATE_DIR` ab, das nicht vom Webserver ausgeliefert wird. 3. Nur bei Einwilligung: Backend führt lokal Gesichtsdetektor und Altersklassen-Modell aus und speichert den Hinweis. 4. Prüfperson sieht Fall, Foto(s) und ggf. Hinweis in der Admin-Queue und entscheidet; Entscheidung und Vermerk werden protokolliert. 5. Löschung der Dateien 7 Tage nach Freigabe / 30 Tage nach Ablehnung / sofort bei Rücknahme, durch Überschreiben und Löschen; Wahrscheinlichkeiten werden mitgelöscht. |
| **Speicherdauer** | Dateien und Wahrscheinlichkeiten: s. o.; Ergebniskennzeichnung, Vermerk, Hinweis und Protokoll: solange das Konto besteht |
| **Empfänger** | ausschließlich intern (Prüfpersonal mit Admin-Secret); keine Auftragsverarbeiter, keine Drittlandübermittlung; Hosting auf eigenem VPS (EU) |
| **Technik** | Rust-Backend; Modelle UltraFace RFB-320 (MIT) und age_googlenet (Apache-2.0) aus dem ONNX Model Zoo, lokal mit `tract` ausgeführt, per SHA-256 gepinnt; vgl. `docs/age-estimation.md`, `README_AGE_VERIFICATION.md` |

## 2. Notwendigkeit und Verhältnismäßigkeit

- **Rechtsgrundlagen:** Art. 6 Abs. 1 lit. c i. V. m. JuSchG/JMStV (Pflicht zur Altersprüfung) und
  Art. 9 Abs. 2 lit. a (ausdrückliche Einwilligung in die biometrische Verarbeitung); für die
  Schätzung eine **gesonderte** ausdrückliche Einwilligung nach Art. 9 Abs. 2 lit. a, jederzeit
  widerrufbar durch Rücknahme des Antrags.
- **Erforderlichkeit:** Ohne Sichtprüfung eines Fotos (ggf. mit Ausweis) ist eine belastbare
  Altersprüfung nicht möglich. Die Schätzung ist nicht erforderlich für die Prüfung selbst, sondern
  verkürzt Wartezeiten; deshalb optional und mit eigener Einwilligung.
- **Datenminimierung:** Nur ein Foto, Ausweis optional; keine Speicherung von Rohbildern über die
  Fristen hinaus; Schätzung speichert Klassen und Wahrscheinlichkeiten, keine Gesichtsmerkmale oder
  Embeddings; keine Wiederverwendung der Daten zu anderen Zwecken; kein Training mit Nutzerdaten.
- **Alternativen geprüft:** (a) Selbstauskunft — rechtlich unzureichend; (b) externer Anbieter
  (Yoti, IDnow u. a.) — zertifiziert, aber Datenübermittlung an Dritte und laufende Kosten;
  (c) vollautomatische Entscheidung — abgelehnt (Art. 22, Unschärfe um 18). Gewählt: menschliche
  Prüfung mit optionaler lokaler Vorsortierung, Anbieter als Eskalationsstufe vorgesehen.
- **Transparenz:** Datenschutzerklärung § 4.6/4.7, § 5, § 9, § 11; Hinweistext an der Checkbox;
  Dokumentation öffentlich im Repository.
- **Betroffenenrechte:** Auskunft über Fall und Hinweis; Rücknahme (Löschung sofort); Widerspruch
  gegen Entscheidung über Einspruchsfrist (30 Tage); Löschung mit Konto.

## 3. Risiken für die Rechte und Freiheiten der Betroffenen

| Nr. | Risiko | Eintritt | Schwere | Begründung |
|---|---|---|---|---|
| R1 | Unbefugter Zugriff auf Fotos/Ausweise (Datenleck, Identitätsdiebstahl) | gering | hoch | Dateien einzeln verschlüsselt, Schlüssel nur im Backend-Prozess, Verzeichnis nicht ausgeliefert, Zugriff nur mit Admin-Secret, kurze Speicherfristen |
| R2 | Falsche Ablehnung Volljähriger / falsche Freigabe Minderjähriger | mittel | mittel | Entscheidung durch Mensch; Schätzung nur Hinweis mit Puffer (≥ 85 % auf 25+); „unsicher“ und „möglicherweise minderjährig“ erzwingen Sichtprüfung mit Ausweis; Einspruchsfrist |
| R3 | Diskriminierung durch Modell-Bias (Herkunft, Geschlecht, Hauttyp) | mittel | mittel | Hinweis ersetzt nie die Entscheidung; keine Nachteile allein aus dem Hinweis; Prüfpersonal angewiesen, bei Abweichung Ausweis anzufordern; Modell dokumentiert, austauschbar |
| R4 | Zweckentfremdung (Profilbildung, Wiedererkennung) | gering | hoch | Keine Embeddings/Templates gespeichert, keine Suche über Fotos, keine Verknüpfung außerhalb des Falls, kein Training |
| R5 | Fehlende oder unwirksame Einwilligung | gering | mittel | Checkbox standardmäßig aus, eigener Text, Zeitpunkt gespeichert (`ai_consent_at`), jederzeit widerrufbar |
| R6 | Übermäßige Speicherdauer | gering | mittel | Automatischer Lösch-Job (7/30 Tage, sofort bei Rücknahme) mit Überschreiben; `blobs_purged_at` als Nachweis |
| R7 | Innentäter (Prüfpersonal) | gering | hoch | Admin-Aktionen protokolliert (`admin_actions`), Zugriff nur per Secret, Sichtung in sandboxed Popup ohne Download-Funktion; Vier-Augen-Prinzip empfohlen (organisatorisch) |

## 4. Maßnahmen (Auswahl, umgesetzt)

- Verschlüsselung at rest je Datei (AES-256-GCM, HKDF-Schlüsselableitung, AAD = Fall-ID + Slot).
- Getrennter Speicherpfad außerhalb jedes Webserver-Roots; Backend-Container ohne Root-Rechte.
- Zugriff nur mit Admin-Secret (≥ 24 Zeichen), Rate-Limits, Protokollierung aller Entscheidungen.
- Löschung durch Überschreiben + Unlink nach Frist bzw. sofort bei Rücknahme; Wahrscheinlichkeiten
  werden mitgelöscht.
- Schätzung vollständig lokal (keine Cloud-API), Modelle per Prüfsumme gepinnt, abschaltbar
  (`AGE_AI_ENABLED=false`).
- Konservative Schwellen; Hinweise sind farblich als Hinweis gekennzeichnet; Entscheidung immer
  durch Mensch; Widerruf einer Freigabe möglich.
- Transport: TLS, HSTS, scharfe Content-Security-Policy, SRI auf allen externen Skripten.

## 5. Restrisiko und Ergebnis

Nach den Maßnahmen wird das Restrisiko als **gering bis mittel** bewertet. Das verbleibende Risiko
liegt vor allem in Fehleinschätzungen durch Menschen oder Modell (R2, R3), das durch die
Einspruchsmöglichkeit und die Pflicht zur Sichtprüfung begrenzt ist. Eine vorherige Konsultation
der Aufsichtsbehörde (Art. 36) ist nicht erforderlich.

## 6. Organisatorische Pflichten (offen, Betreiber)

- [ ] Freigabe dieser DSFA durch Geschäftsführung; Stellungnahme DSB einholen (Art. 35 Abs. 2).
- [ ] Prüfpersonal schriftlich anweisen: Hinweis ist kein Entscheidungsersatz; bei „unsicher“ oder
      „möglicherweise minderjährig“ Ausweis anfordern; Vier-Augen-Prinzip bei Ablehnungen.
- [ ] Verzeichnis von Verarbeitungstätigkeiten (Art. 30) um diesen Vorgang ergänzen.
- [ ] Jährliche Überprüfung; bei Modellwechsel Bias-Stichprobe (mind. 50 Fälle, Verteilung der
      Hinweise nach Entscheidung) dokumentieren.
