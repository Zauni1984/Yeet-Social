# Changelog-Bot („YEET Updates")

Ein System-Nutzer, der jede Neuerung und jeden Fix als **permanenten Post**
auf Yeet veröffentlicht – englisch, maximal 420 Zeichen, mit Hashtags.

## Wie es funktioniert

- Quelle ist `backend/changelog.json`. Die Datei wird beim Build in das
  Backend eingebettet; jedes deployte Image bringt seine Release-Notes mit.
- Nach dem Start (und danach stündlich) postet das Backend alle Einträge, die
  noch nicht veröffentlicht wurden (Tabelle `changelog_posts`), älteste zuerst,
  höchstens `CHANGELOG_BOT_MAX_PER_RUN` (Standard 3) pro Lauf – so flutet ein
  Rückstand nicht den Feed. Ein Eintrag wird nie doppelt gepostet, auch nicht
  bei Neustarts oder mehreren Instanzen.
- Der Bot-Nutzer (`@yeet_updates`, Anzeigename „YEET Updates") wird beim ersten
  Lauf angelegt: kein Wallet, keine E-Mail, kein Passwort → niemand kann sich
  als Bot anmelden. `users.is_bot = TRUE` blendet im Frontend ein **BOT**-Badge
  ein; Posts des Bots erhalten keine Punkte.
- Posts: `is_permanent = TRUE` (24 h im Feed, dauerhaft unter „Permanente
  Posts" und im Bot-Profil), `lang = 'en'`, Standard-Hashtag `#YeetUpdate` plus
  die Tags des Eintrags.

## Einen Eintrag hinzufügen (Konvention für jeden PR mit sichtbaren Änderungen)

```json
{
  "id": "2026-09-12-01-kurzer-slug",
  "text": "What changed, written for users. One or two sentences, English.",
  "tags": ["#Fix", "#NewFeature"]
}
```

- `id` muss eindeutig und aufsteigend sortierbar sein (`YYYY-MM-DD-NN-slug`).
- `text` + Hashtags müssen in 420 Zeichen passen. `cargo test changelog` prüft
  das (läuft in CI) – ein zu langer Eintrag lässt den Build rot werden.
- Keine Duplikate von `#YeetUpdate` nötig, der Bot ergänzt ihn selbst.

## Env-Variablen

| Variable | Default | Bedeutung |
| --- | --- | --- |
| `CHANGELOG_BOT_ENABLED` | `true` | `false` schaltet den Bot ab (bestehende Posts bleiben) |
| `CHANGELOG_BOT_USERNAME` | `yeet_updates` | Benutzername des Bot-Kontos |
| `CHANGELOG_BOT_MAX_PER_RUN` | `3` | Einträge pro Lauf (Start + stündlich) |

## Betrieb

- Text eines bereits geposteten Eintrags ändern → wird **nicht** nachgezogen
  (Post ist veröffentlicht). Für eine Korrektur einen neuen Eintrag anlegen.
- Einen Eintrag zurückziehen: Post im Admin-Dashboard entfernen; die Zeile in
  `changelog_posts` verhindert ein erneutes Posten.
- Bot-Profil: `https://justyeet.it/#` → Suche nach `yeet_updates` oder über
  einen seiner Posts.
