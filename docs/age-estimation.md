# KI-Vorsortierung der 18+-Queue (Altersschätzung)

Stand: 2026-11-12. Ergänzt `README_AGE_VERIFICATION.md`; ändert nichts daran, dass **ein Mensch
entscheidet**.

## Was es tut

Beim Einreichen eines Gesichtsfotos zur 18+-Verifikation kann der Nutzer optional zustimmen, dass
unser Server das Alter aus dem Foto schätzt. Die Schätzung erscheint in der Admin-Queue als Hinweis
neben dem Fall:

| Hinweis | Bedeutung | Regel |
|---|---|---|
| **wahrscheinlich volljährig (xx %)** | Modell sieht ≥ 85 % Wahrscheinlichkeit für 25 Jahre oder älter | `p(25+) ≥ 0,85` |
| **Hinweis — möglicherweise minderjährig** | ≥ 50 % Wahrscheinlichkeit für die Kinder-Klassen (0–12) | `p(0–12) ≥ 0,5` |
| **unsicher (xx % volljährig)** | alles dazwischen, insbesondere die Klasse 15–20, die die 18 schneidet | sonst |
| **kein Gesicht erkannt** | Detektor findet kein Gesicht mit Score ≥ 0,7 | — |
| **n Gesichter** | mehr als ein Gesicht; das größte wurde bewertet | — |
| keine Einwilligung / Schätzung läuft / Fehler | keine Schätzung vorhanden | — |

Bewusst konservativ: „wahrscheinlich volljährig" verlangt einen deutlichen Abstand zur 18 (Puffer
wie von der KJM für Altersschätzung gefordert, dort ≥ 25). Die Entscheidung Freigeben/Ablehnen
trifft weiterhin der Admin nach Sichtung von Foto und ggf. Ausweis.

## Technik

- Zwei kleine ONNX-Modelle aus dem ONNX Model Zoo, lokal im Backend ausgeführt (`tract`, reines
  Rust, kein externer Dienst, keine GPU nötig): UltraFace RFB-320 (Gesichtsdetektor, MIT) und
  `age_googlenet` (8 Altersklassen nach Levi/Hassner, Apache-2.0). Dateien liegen im Docker-Image
  unter `/app/models`, per SHA-256 gepinnt; fehlen sie, ist die Funktion aus.
- Ablauf je Fall: Foto dekodieren → Detektor (320×240) → größtes Gesicht mit 20 % Rand ausschneiden
  → 224×224 → Altersklassen-Wahrscheinlichkeiten → Hinweis. Läuft im Hintergrund nach dem
  Speichern, ca. 0,3–1 s CPU je Fall; die Antwort an den Nutzer wartet nicht darauf.
- Gespeichert werden Hinweis, Wahrscheinlichkeiten, Altersklasse, Gesichts-Score, Anzahl Gesichter,
  Modell-Kennung und Zeitpunkt (`age_verification_cases.ai_*`). Die Wahrscheinlichkeiten werden
  zusammen mit den Fotos gelöscht (7 Tage nach Freigabe, 30 Tage nach Ablehnung); der Hinweis bleibt
  als Teil des Prüfprotokolls.
- Abschalten: `AGE_AI_ENABLED=false` in der `.env`.

## Datenschutz

- Rechtsgrundlage: ausdrückliche Einwilligung (Art. 9 Abs. 2 lit. a DSGVO), da die Auswertung
  biometrische Merkmale betrifft. Die Checkbox ist standardmäßig **aus**; ohne Haken wird nichts
  geschätzt, der Fall geht unverändert in die manuelle Prüfung. Der Zeitpunkt der Einwilligung wird
  gespeichert (`ai_consent_at`).
- Keine Weitergabe: Verarbeitung ausschließlich auf dem eigenen Server, keine Cloud-API, keine
  Protokollierung des Bildes.
- Keine automatisierte Entscheidung im Sinne von Art. 22 DSGVO: der Hinweis beeinflusst nur die
  Reihenfolge/Aufmerksamkeit, die Entscheidung trifft eine Person.
- Noch zu tun durch den Betreiber: Datenschutzerklärung um den Absatz „Altersschätzung (optional)"
  ergänzen; Datenschutz-Folgenabschätzung kurz dokumentieren (Zweck, Modell, Speicherdauer,
  Einwilligung, kein Drittzugriff).

## Grenzen

Altersschätzung aus dem Gesicht ist um die 18 herum unscharf (typisch ± 4–6 Jahre), bei
schlechtem Licht, Masken, starkem Make-up oder Fotos vom Bildschirm noch mehr. Das Modell ist auf
westlich geprägten Datensätzen trainiert und kann bei anderen Gesichtern systematisch abweichen.
Deshalb: nie als alleiniger Nachweis, immer mit Ausweisprüfung durch den Admin bei „unsicher" und
„möglicherweise minderjährig".
