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
| `TRANSLATE_PROVIDER` | – | `azure`, `google`, `deepl` oder `libretranslate`; leer = aus |
| `TRANSLATE_URL` | je Provider (Azure `api.cognitive.microsofttranslator.com`, Google `translation.googleapis.com`, DeepL `api.deepl.com`, LibreTranslate `libretranslate:5000`) | Basis-URL, nur bei Bedarf überschreiben |
| `TRANSLATE_API_KEY` | – | Pflicht für Azure, Google, DeepL; optional für LibreTranslate |
| `TRANSLATE_REGION` | – | Nur Azure: Region der Ressource (z. B. `germanywestcentral`) |

## Provider-Wahl (Stand September 2026)

DeepL bietet den früheren **API-Free-Tarif nicht mehr an** (nur noch „Developer" mit
einmalig 1 Mio. Zeichen zum Testen und das Abo „Growth"). Kostenlose, dauerhafte
Kontingente gibt es bei:

| Provider | Gratis-Kontingent | Danach | Konto nötig | Bemerkung |
| --- | --- | --- | --- | --- |
| **Azure AI Translator (F0)** | **2 Mio. Zeichen/Monat**, dauerhaft | Stufe S1 ≈ 10 $/Mio. Zeichen | Azure-Konto (Kreditkarte zur Verifizierung), Ressource in EU-Region möglich | Empfehlung: größtes Gratis-Kontingent, EU-Hosting (`germanywestcentral`), gute Qualität für DE/EN/IT/FR/ES/PT. Bei Überschreitung 429/403, kein automatisches Kostenrisiko. |
| Google Cloud Translation (v2/v3 NMT) | 500.000 Zeichen/Monat, dauerhaft | 20 $/Mio. Zeichen | Google-Cloud-Projekt mit Abrechnungskonto | Sehr gute Qualität; Abrechnung schaltet nach dem Gratis-Kontingent automatisch weiter (Budget-Alarm setzen). |
| DeepL API (Growth) | – (Developer: 1 Mio. einmalig) | Abo + Nutzung | DeepL-Konto | Beste Qualität für DE, aber kein Gratis-Betrieb mehr. |
| LibreTranslate (selbst gehostet) | unbegrenzt | 0 € | – | Braucht ca. 4–8 GB RAM auf dem VPS für unsere Sprachpaare; Qualität deutlich unter den Cloud-Diensten. Gehostete Variante 29 $/Monat. |

Der Code ist provider-neutral (`TRANSLATE_PROVIDER=azure|google|deepl|libretranslate`),
der Übersetzungs-Cache bleibt bei einem Wechsel gültig. Die Spracherkennung nutzt zuerst
die kostenlose Heuristik und fragt den Provider nur bei unklaren Fällen.

## Aktivierung: Azure AI Translator (empfohlen)

1. Im Azure-Portal eine Ressource **„Translator"** anlegen, Tarif **F0 (Free)**, Region
   z. B. `germanywestcentral` oder `westeurope`.
2. Unter „Schlüssel und Endpunkt" Schlüssel 1 und die Region kopieren.
3. Auf dem VPS in `/root/yeet-social/.env` (siehe `vps/.env.example`):
   ```
   TRANSLATE_PROVIDER=azure
   TRANSLATE_API_KEY=<Schlüssel 1>
   TRANSLATE_REGION=germanywestcentral
   ```
   (`TRANSLATE_URL` leer lassen → globaler Endpunkt `api.cognitive.microsofttranslator.com`.)
4. Backend neu starten (`bash /tmp/start_backend.sh` bzw. `docker compose restart yeet-api`).
5. Prüfen: `curl -s https://justyeet.it/api/v1/translate/status` → `"enabled":true,"provider":"azure"`.
   Ab dann erscheint der Übersetzen-Button automatisch; kein Frontend-Deploy nötig.

Alternative Google: Cloud-Projekt → „Cloud Translation API" aktivieren → API-Schlüssel
(auf die Translation API einschränken) → `TRANSLATE_PROVIDER=google`,
`TRANSLATE_API_KEY=<Key>`; Budget-Alarm bei 0 € setzen, damit nichts durchrutscht.

## Provider-Vergleich (ältere Notiz)

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
