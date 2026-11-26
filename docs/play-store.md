# YEET Social im Google Play Store

Stand: 2026-11-26. Die Play-Regeln ändern sich laufend; vor der Einreichung die
verlinkten Google-Seiten gegenlesen.

## 1. Was die Android-App ist

`android/` ist eine **Trusted Web Activity (TWA)**: eine dünne Hülle, die
`https://justyeet.it` im Browser-Kern des Geräts im Vollbild zeigt. Die App
enthält keine eigene Logik. Jede Änderung an der Web-App ist sofort auch in der
Android-App, ohne neues Release.

| | |
|---|---|
| Paketname | `it.justyeet.app` (nach der ersten Veröffentlichung nicht mehr änderbar) |
| Mindestversion | Android 8.0 (API 26) |
| Ziel-API | 36 (von Play für neue Apps verlangt) |
| Berechtigungen | nur Benachrichtigungen |
| Links | `https://justyeet.it/...` öffnet sich in der App |
| Größe | ca. 0,8 MB |

Kamera, Mikrofon und Benachrichtigungen fragt der Browser ab, wie auf der
Website.

Die Zuordnung App ↔ Website steht in zwei Dateien, die zusammenpassen müssen:

- App: `asset_statements` in `android/app/src/main/res/values/strings.xml`
- Website: `frontend/.well-known/assetlinks.json` (Paketname und
  SHA-256-Fingerabdruck des Signaturschlüssels)

Fehlt oder stimmt der Fingerabdruck nicht, läuft die App trotzdem, zeigt aber
oben eine Adressleiste.

`mobile/` (React Native, Stand v0.1) ist ein früher Entwurf ohne
Android-Projekt und nicht Teil dieses Wegs.

## 2. Einmalige Schritte (Betreiber)

1. **Play-Console-Konto** als Organisation (BlockSocial UG) anlegen:
   25 USD einmalig, D-U-N-S-Nummer und Identitätsprüfung. Für private Konten
   verlangt Google vor der Veröffentlichung einen geschlossenen Test mit
   mehreren Testern über zwei Wochen; ob das für euer Konto gilt, zeigt die
   Console.
2. **Upload-Schlüssel** als GitHub-Secrets hinterlegen:
   - `ANDROID_KEYSTORE_B64`: die Schlüsseldatei, Base64-kodiert
     (`base64 -w0 upload.jks`)
   - `ANDROID_KEYSTORE_PASSWORD`: das Passwort dazu
   Die Datei zusätzlich sicher aufbewahren. Geht sie verloren, kann Google den
   Upload-Schlüssel zurücksetzen, solange Play App Signing aktiv ist.
3. **Bundle bauen**: GitHub → Actions → „Android — build Play Store bundle“
   → `version_name` (z. B. `1.0.0`) und `version_code` (ganze Zahl, bei jedem
   Upload höher). Das Artefakt enthält `app-release.aab` (für Play) und
   `app-release.apk` (zum direkten Installieren auf einem Testgerät).
4. **App in der Console anlegen**, Play App Signing aktiviert lassen, das
   `.aab` in den internen Test hochladen.
5. **Fingerabdruck von Google eintragen**: Console → App-Integrität →
   App-Signatur → „SHA-256-Zertifikatfingerabdruck“ des
   *App-Signaturschlüssels* kopieren und in
   `frontend/.well-known/assetlinks.json` als zweiten Eintrag in
   `sha256_cert_fingerprints` ergänzen. Erst danach läuft die aus dem Store
   installierte App ohne Adressleiste. Der bereits eingetragene Fingerabdruck
   gehört zum Upload-Schlüssel und deckt direkt installierte Test-APKs ab.
6. **Store-Eintrag ausfüllen** (Abschnitt 4), Inhaltseinstufung,
   Datensicherheit, Zielgruppe.

Lokal bauen: Android-SDK (Plattform 36) und JDK 17+ installieren, dann
`cd android && ./gradlew bundleRelease`. Ohne die Variablen
`ANDROID_KEYSTORE_FILE` und `ANDROID_KEYSTORE_PASSWORD` ist das Ergebnis
unsigniert.

## 3. Offene Entscheidungen vor der Einreichung

Diese Punkte entscheiden, ob Google die App annimmt. Sie sind keine
Technikfragen.

### 3.1 18+-Inhalte

Play verbietet Apps, die sexuelle Inhalte enthalten oder bewerben. Bei
Plattformen mit Nutzerinhalten duldet Google beiläufige Erwachseneninhalte nur,
wenn sie standardmäßig ausgeblendet und für Minderjährige gesperrt sind.
Ausdrücklich verboten ist, solche Inhalte zu monetarisieren.

YEET hat einen 18+-Bereich und Pay-per-View. **Pay-per-View oder Trinkgeld auf
18+-Beiträge ist mit Play nicht vereinbar.** Möglichkeiten:

- a) In der Android-App (Aufruf mit `?source=android`) den 18+-Bereich ganz
  ausblenden. Sicherster Weg.
- b) 18+ in der App nur nach Altersverifikation zeigen, aber dort jede
  Bezahlfunktion sperren. Höheres Ablehnungsrisiko.
- c) Nichts ändern: Ablehnung oder spätere Sperre ist wahrscheinlich.

### 3.2 Punkte, Trinkgeld, Pay-per-View

Digitale Inhalte, die in einer App gegen Geld freigeschaltet werden, müssen
über Google Play Billing laufen (15–30 % Gebühr). Punkte bei YEET werden
verdient und nicht gekauft; das spricht gegen eine Billing-Pflicht. Sobald
Punkte oder YEET gegen Geld erworben werden können, ändert sich das.

### 3.3 Token und Umtausch

Apps mit Blockchain-Inhalten müssen das in der Console erklären
(„tokenisierte digitale Assets“). Die Umwandlung von Punkten in YEET und der
NOTE-Swap fallen darunter. Die App darf keine Gewinne aus Token in Aussicht
stellen. Für Wallet- und Börsenfunktionen verlangt Google je nach Land
Lizenznachweise; YEET verwahrt keine Guthaben auf der Chain, das ist in der
Erklärung so anzugeben.

### 3.4 Pflichten für Apps mit Nutzerinhalten

| Pflicht | Stand |
|---|---|
| Nutzungsbedingungen mit Regeln für Inhalte | vorhanden |
| Melden von Inhalten und Nutzern in der App | vorhanden |
| Blockieren von Nutzern | vorhanden |
| Moderation mit Reaktion auf Meldungen | Admin-Panel vorhanden; Prozess und Reaktionszeit festlegen |
| Konto löschen in der App | vorhanden (Einstellungen) |
| Konto löschen über eine Web-Adresse | Adresse für das Formular nötig, z. B. ein Abschnitt in der Datenschutzerklärung |
| Datenschutzerklärung als öffentliche URL | `https://justyeet.it/legal/privacy` |

### 3.5 Zielgruppe

Zielgruppe „18 und älter“ angeben. Mit jüngeren Zielgruppen greifen die
Familienrichtlinien, die mit Token und 18+-Inhalten nicht vereinbar sind.

## 4. Store-Eintrag

**Name (max. 30 Zeichen):** YEET Social

**Kurzbeschreibung (max. 80 Zeichen)**

- DE: Beiträge für 24 Stunden, Audio-Stories, Live-Streams, verschlüsselte Chats.
- EN: 24-hour posts, audio stories, live streams and encrypted chats.

**Beschreibung (DE)**

> YEET Social ist ein soziales Netzwerk für den Moment: Beiträge verschwinden
> nach 24 Stunden, außer du machst sie dauerhaft.
>
> • Beiträge mit Text, Bild und Video
> • Audio-Stories bis 60 Minuten
> • Live-Streams
> • Ende-zu-Ende-verschlüsselte Nachrichten
> • Eigene Webboards aus RSS-Feeds
> • Punkte für Beiträge, Kommentare und Weiterverbreitungen
> • Zwei-Faktor-Anmeldung
> • In 42 Sprachen
>
> Die Nutzung ist kostenlos. Eine Wallet ist nicht nötig.
>
> Betreiber: BlockSocial UG (haftungsbeschränkt), Deutschland.

**Beschreibung (EN)**

> YEET Social is a social network for the moment: posts disappear after 24
> hours unless you make them permanent.
>
> • Posts with text, images and video
> • Audio stories up to 60 minutes
> • Live streams
> • End-to-end encrypted messages
> • Your own webboards from RSS feeds
> • Points for posts, comments and reshares
> • Two-factor sign-in
> • Available in 42 languages
>
> Free to use. No wallet needed.
>
> Operated by BlockSocial UG (haftungsbeschränkt), Germany.

Die Beschreibung nennt bewusst keine Token und keine Verdienstmöglichkeit
(Abschnitt 3.3). Vor der Einreichung mit dem tatsächlichen Funktionsumfang
abgleichen.

**Grafiken**

| Was | Vorgabe | Stand |
|---|---|---|
| App-Symbol | 512 × 512 PNG | `frontend/icons/store-icon-512.png` |
| Feature-Grafik | 1024 × 500 | fehlt |
| Screenshots Telefon | mindestens 2, Hochformat | fehlen |

**Kategorie:** Soziale Netzwerke. **Kontakt:** eine Support-E-Mail-Adresse und
die Website.

## 5. Formular „Datensicherheit“ (Entwurf)

Angaben nach dem Stand des Codes; vor dem Absenden mit der Datenschutzerklärung
abgleichen.

| Datenart | Erhoben | Zweck | Pflicht |
|---|---|---|---|
| E-Mail-Adresse | ja | Konto, Sicherheit | bei E-Mail-Konten |
| Nutzername, Anzeigename, Profilbild | ja | App-Funktion | Nutzername ja |
| Beiträge, Kommentare, Medien | ja | App-Funktion | nein |
| Nachrichten | ja, Ende-zu-Ende-verschlüsselt | App-Funktion | nein |
| Ausweis- und Gesichtsbild | ja, nur bei Altersverifikation, verschlüsselt gespeichert | Altersprüfung | nein |
| Wallet-Adresse | ja, wenn verknüpft | Konto, Auszahlung | nein |
| IP-Adresse | ja | Sicherheit, Missbrauchsschutz | ja |
| Push-Abonnement | ja, wenn aktiviert | Benachrichtigungen | nein |

- Übertragung verschlüsselt: ja (HTTPS).
- Löschung auf Anfrage: ja (Konto löschen in den Einstellungen).
- Weitergabe an Dritte: nach Datenschutzerklärung angeben (E-Mail-Versand,
  optional Übersetzungsdienst, LiveKit für Streams).
- Werbung und Tracking: keine.

## 6. Ein neues Release

Nur nötig, wenn sich an `android/` etwas ändert (Symbol, Name, Ziel-API) oder
Google eine höhere Ziel-API verlangt. Dann den Workflow mit höherem
`version_code` starten und das `.aab` hochladen. Änderungen an der Web-App
brauchen kein Release.

Die Symbole stammen aus `scripts/build-icons.cjs` (`node
scripts/build-icons.cjs`, benötigt Playwright) und sind eingecheckt.

## 7. Geprüft und nicht geprüft

Geprüft (2026-11-26): Lint und Release-Build laufen durch; Bundle und APK sind
signiert; Paketname, Ziel-API 36, Berechtigungen und Link-Filter stehen wie
oben im fertigen Paket; die neuen nginx-Regeln liefern Manifest, Symbole und
`assetlinks.json` als Dateien aus.

Nicht geprüft: Start der App auf einem Gerät oder Emulator, Vollbild ohne
Adressleiste, Benachrichtigungen, Verhalten der Links. Das geht erst, wenn die
Website-Dateien dieses Stands ausgeliefert sind.
