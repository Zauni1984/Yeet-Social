# Sicherheits-Audit & Härtung — 2026-11-13

Umfang: Abwehr gegen Konto-Übernahme und unautorisierte Werttransfers, auf Wunsch des Betreibers.
Ergänzt den App-Audit `audit-2026-10-03.md`. Alle Punkte wurden im Code umgesetzt und lokal
(Postgres + Redis) end-to-end geprüft; ein Last-/Angriffstest gegen die **Live**-Seite ist bewusst
nicht erfolgt (siehe Abschnitt 5).

## 1. Zwei-Faktor-Authentifizierung (TOTP, Authenticator-App) — NEU

- Standard-TOTP (RFC 6238, SHA-1, 6 Stellen, 30 s, ±1 Fenster) kompatibel mit Google
  Authenticator, Aegis, 1Password usw. Einrichtung zweistufig: `setup` erzeugt ein **vorläufiges**
  Secret und liefert QR + otpauth-URL; `activate` schaltet 2FA erst ein, wenn ein gültiger Code
  eingegeben wurde, und gibt 10 einmalige Wiederherstellungscodes aus (nur einmal sichtbar).
- **Secret verschlüsselt at rest**: AES-256-GCM unter einem aus `JWT_SECRET` via HKDF-SHA256
  abgeleiteten Schlüssel. Ein DB-Dump allein liefert keine funktionierenden zweiten Faktoren.
  Wiederherstellungscodes nur als SHA-256-Hashes gespeichert; ein benutzter Code wird entfernt.
- **Login-Fluss**: Bei aktivem 2FA gibt der Passwort-Schritt **keine** Tokens zurück, sondern eine
  kurzlebige Challenge (`mfa_required`, 5 min, Redis). Erst `POST /auth/mfa` mit gültigem Code oder
  Wiederherstellungscode liefert die Tokens. `/auth/mfa` ist pro IP und pro Konto rate-limitiert.
- **Sensible Aktionen**: `api::reauth` akzeptiert jetzt zusätzlich einen 2FA-Code als Nachweis —
  für Kontolöschung und E2EE-Schlüsselersetzung bei 2FA-Konten.
- Deaktivieren verlangt einen aktuellen Code (oder Wiederherstellungscode): eine gekaperte Sitzung
  kann 2FA nicht still abschalten.
- UI: Einstellungen → Zwei-Faktor-Authentifizierung (QR, manueller Schlüssel, Code-Eingabe,
  Wiederherstellungscodes) und ein Code-Schritt im Login. Status über `GET /me/2fa/status`.

## 2. Login-Drossel (Brute-Force)

Bereits vorhanden und verifiziert: pro IP 10/min + 100/h, pro Konto 5/min + 30/h; unbekannte
E-Mail verbrennt denselben Argon2-Aufwand (A-M4), damit kein Timing-Leak. Lokaler Burst von 20
Fehlversuchen: nach 4–5 × 401 greift durchgehend 429, **kein** 5xx, kein Fehlversuch erfolgreich,
Server danach gesund (`/health` 200).

## 3. Unautorisierter Punkte-Transfer — geprüft, strukturell ausgeschlossen

- Alle Punkte-Abbuchungen (Tipps, Swap-Einzahlung, Paper-Wallet, Lives, Umwandlung) leiten das
  **Quellkonto ausschließlich aus der authentifizierten Sitzung** ab. Kein Endpunkt nimmt ein
  Quellkonto aus dem Request entgegen; es gibt keinen Admin-„Punkte-gutschreiben"-Pfad.
- Nachweis im Test: ein Angreifer mit 0 Punkten kann keine Punkte bewegen (`Insufficient points`),
  das Zielkonto bleibt unangetastet; es existiert kein Feld, um ein fremdes Quellkonto zu benennen.
- **Nebenläufige Doppelausgabe** ist durch `SELECT … FOR UPDATE` auf die Absenderzeile plus
  In-Transaction-Guthabenprüfung abgesichert. Test: 3 gleichzeitige Tipps über das volle Guthaben →
  **genau einer** settlet, die anderen beiden `Insufficient points`, kein negatives Guthaben.
- NaN/Inf-Beträge und Dust sind bereits abgefangen (M-C1, M-L3).

## 4. Backend-Härtung (Bestand, hier bestätigt)

JWT mit Access/Refresh-Rotation und Familien-Widerruf bei Reuse (A-L1/L2); Argon2id; scharfe CSP,
HSTS, nosniff, X-Frame-Options DENY, SRI auf allen CDN-Skripten; Upload-Magic-Byte-Prüfung; CORS
Allow-List; gebundene SQL-Parameter bzw. geprüfte dynamische SQL (sqlx 0.9 `SqlSafeStr`);
LIKE-Wildcards escaped (A-L7); Constraint-Fehler → 409/422 statt 500 (A-L5); Container ohne Root
(I-M7); Actions SHA-gepinnt (I-M1); PII-Vault AES-GCM/HKDF. Dependency-Advisory `h2 0.3` beseitigt
(I-M3, ethers → alloy).

## 5. Last- / Angriffstest

Kontrolliert **lokal** ausgeführt (nicht gegen justyeet.it): Login-Brute-Force-Burst,
nebenläufige Doppelausgabe, Feed-Leseburst — Drossel greift, keine 5xx, keine Überziehung, Server
bleibt erreichbar. Ein Last-/Penetrationstest gegen die **Live**-Instanz wurde absichtlich nicht
durchgeführt: er kann echte Nutzer beeinträchtigen und gehört in ein Wartungsfenster mit
ausdrücklicher Freigabe. Empfehlung für den geplanten Angriff:

- In einem Wartungsfenster oder gegen eine Staging-Kopie fahren; Nutzer vorab informieren.
- Werkzeuge: `k6`/`wrk` für Last, ein beauftragter Pentest (OWASP-Checkliste) für die Logik.
- Schwerpunkte: Login/MFA-Raten, Token-Rotation unter Last, Tip-/Swap-Nebenläufigkeit,
  Rate-Limit-Umgehung über wechselnde IPs, Upload-Grenzen, CSP-Report-Auswertung.
- Ein externer Security-Audit der Smart Contracts bleibt vor dem Mainnet-Deploy offen (Checkliste).

## Offen (Betreiber)
- 2FA als Pflicht für Admin-Konten erwägen (derzeit optional für alle).
- Live-Last-/Pentest beauftragen (Abschnitt 5).
- WebAuthn/Passkeys als zweiter Faktor als späterer Ausbau (stärker als TOTP gegen Phishing).
