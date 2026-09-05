# Post-Übersetzung & Spracherkennung

Status: **implementiert, Provider optional.** Ohne konfigurierten Provider ist
alles inert: `GET /api/v1/translate/status` meldet `enabled:false`, das
Frontend blendet den „Übersetzen"-Button aus. Die Spracherkennung läuft
trotzdem (kostenlose Stoppwort-Heuristik), damit jeder Post ein `lang` trägt.

## Nutzerseite

- Unter jedem Post, dessen Sprache von der UI-Sprache abweicht (oder unbekannt
  ist), erscheint **Übersetzen**. Die Übersetzung wird als markierter Block mit
  Quellsprache angezeigt, „Original ausblenden/anzeigen" schaltet um.
- **Einstellungen → Barrierefreiheit → Übersetzen → „Posts automatisch
  übersetzen":** Posts mit *bekannter* Fremdsprache werden beim Rendern
  automatisch übersetzt (max. 2 parallel). Unbekannte Sprache (`und`) wird nie
  automatisch übersetzt, nur auf Klick — spart Provider-Kontingent.
- Der Vorleser liest die Übersetzung, wenn sie eingeblendet ist.

## Backend

| Teil | Ort |
| --- | --- |
| Service (Provider, Erkennung, Sweep) | `backend/src/services/translate.rs` |
| API | `backend/src/api/translate.rs` |
| Migration | `backend/migrations/0045_post_translations.sql` (`posts.lang`, `post_translations`) |

**Endpunkte**

- `GET /api/v1/translate/status` (öffentlich) → `{enabled, provider, languages}`
- `POST /api/v1/posts/:id/translate` `{target:"de"}` (Auth) →
  `{text, source_lang, target_lang, cached, same_language}`
  - Cache je Post+Zielsprache in `post_translations` (Provider wird nur einmal
    pro Kombination befragt).
  - Rate-Limit je Account: 20/Minute, 300/Stunde → `429 RATE_LIMITED`.
  - Ziel muss eine der sechs UI-Sprachen sein (`en de it fr es pt`), sonst
    `UNSUPPORTED_TARGET`; ohne Provider `403 TRANSLATION_DISABLED`.

**Spracherkennung**

- Beim Erstellen eines Posts sofort (`spawn_detect`), zusätzlich ein Sweep alle
  30 s für alles mit `lang IS NULL` (geplante Posts, Reposts, RSS-Webboards,
  Altbestand). Kein Ergebnis → `und` (wird nicht erneut geprüft).
- Reihenfolge: LibreTranslate `/detect` (falls Provider), sonst Heuristik.
  Die Heuristik ist absichtlich konservativ (mind. 2 Treffer, klarer Sieger).

**Env-Variablen**

| Variable | Default | Bedeutung |
| --- | --- | --- |
| `TRANSLATE_PROVIDER` | – | `libretranslate` oder `deepl`; leer = aus |
| `TRANSLATE_URL` | `http://libretranslate:5000` bzw. `https://api-free.deepl.com` | Basis-URL des Providers |
| `TRANSLATE_API_KEY` | – | Pflicht für DeepL (`DeepL-Auth-Key`), optional für LibreTranslate |

## Provider-Wahl (Entscheidung offen)

| | LibreTranslate (selbst gehostet) | DeepL API |
| --- | --- | --- |
| Kosten | 0 € laufend, aber ~2–4 GB RAM auf dem VPS (Docker-Container) | Free-Tarif 500.000 Zeichen/Monat, danach Pro (nutzungsabhängig) |
| Qualität | brauchbar, deutlich unter DeepL | sehr gut, v. a. DE/EN/FR/IT/ES/PT |
| Datenschutz | Daten bleiben auf unserem Server | Posts gehen an DeepL (DSGVO: AVV vorhanden, EU-Server) |
| Spracherkennung | eigener `/detect`-Endpunkt | nur implizit beim Übersetzen (wir nutzen die Heuristik) |
| Aktivierung | Container zur `docker-compose` hinzufügen, `TRANSLATE_PROVIDER=libretranslate` | Key erzeugen, `TRANSLATE_PROVIDER=deepl`, `TRANSLATE_API_KEY=…` |

Empfehlung für den Start: **DeepL Free** (Qualität, kein zusätzlicher
RAM-Bedarf, in Minuten aktiviert). Bei wachsendem Volumen auf LibreTranslate
oder DeepL Pro wechseln — der Code ist provider-neutral, Cache bleibt gültig.
