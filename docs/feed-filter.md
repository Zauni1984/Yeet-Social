# Feed-Filter nach Sprache und Land

Nutzer können ihren Feed auf bestimmte Sprachen und/oder Länder einschränken –
jederzeit änderbar unter **Einstellungen → Feed-Filter**.

## Verhalten

- **Sprachen:** „Alle Sprachen", „Nur meine Sprache" (folgt der UI-Sprache, auch
  wenn sie später gewechselt wird) oder „Ausgewählte Sprachen" (Mehrfachauswahl
  aus den zehn UI-Sprachen). Posts mit unbekannter Sprache (`und`/NULL, z. B.
  reine Emoji-Posts) bleiben sichtbar.
- **Länder:** „Mein Land" setzt das eigene Land (ISO 3166-1 alpha-2, Namen über
  `Intl.DisplayNames` in der UI-Sprache). „Nur Posts aus diesen Ländern" filtert
  nach dem Land des Autors; Autoren ohne gesetztes Land werden dann ausgeblendet.
- **Gilt für:** globaler Feed (inkl. Trending, das daraus ableitet), Tab AUDIO
  und der 18+-Feed. **„Gefolgt" bleibt immer vollständig** (bewusst gefolgte
  Accounts). Permanente Posts und Profile sind nicht betroffen.
- Ein Chip im Feed-Kopf („Filter: Deutsch · DE, AT") zeigt den aktiven Filter
  und führt per Klick in die Einstellungen.

## Technik

| Teil | Ort |
| --- | --- |
| Migration | `backend/migrations/0048_feed_filters.sql` – `user_settings.feed_langs`, `feed_countries` (TEXT[], leer = kein Filter), Index auf `users.country_code` |
| API | `GET/PATCH /api/v1/settings` (`feed_langs`, `feed_countries`; Codes werden normalisiert, max. 20/50), `PATCH /api/v1/users/me` (`country_code`, `""` löscht) |
| Feed | `get_feed`, `get_adult_feed`: `(cardinality($n) = 0 OR p.lang = ANY($n) OR p.lang IS NULL OR p.lang = 'und')` und `(cardinality($m) = 0 OR u.country_code = ANY($m))`, Count-Queries identisch |
| Profil | `UserProfile.country_code` wird ausgeliefert (für spätere Anzeige/Flaggen) |
| Frontend | Modul `YeetFeedFilter` (Einstellungsbereich, Speichern, Chip, Modus „meine Sprache" folgt `setLang`) |

Die Sprache eines Posts stammt aus der Spracherkennung beim Posten
(`docs/uebersetzung.md`), das Land aus dem Profil des Autors.
