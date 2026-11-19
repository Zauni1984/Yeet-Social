# YEET Social — VPS Deploy Cheatsheet

## Frontend deploy (instant)
```bash
set +H && curl -sL -H "Authorization: Bearer GITHUB_PAT" \
  "https://api.github.com/repos/Zauni1984/Yeet-Social/contents/frontend/index.html" \
  | python3 -c "import sys,json,base64; d=json.load(sys.stdin); open('/root/yeet-html/index.html','wb').write(base64.b64decode(d['content'].replace('\\n','')))"

# Fix forEach bug after every deploy:
python3 -c "
f=open('/root/yeet-html/index.html','rb').read()
old=b'      wrap.appendChild(div);\n    var composer'
new=b'      wrap.appendChild(div);\n    });\n    var composer'
if old in f:
    open('/root/yeet-html/index.html','wb').write(f.replace(old,new,1))
    print('FIXED')
"
```

## Backend deploy (after CI build)
```bash
bash /tmp/docker compose (start_backend.sh was removed in the 2026-10 audit: it edited pg_hba.conf, ran ad-hoc DDL and published :8080 on all interfaces)
```

## Quick health check
```bash
curl -s http://127.0.0.1:8080/api/v1/health
curl -s "http://127.0.0.1:8080/api/v1/link-preview?url=https://hanfjack.de" | python3 -m json.tool
```

## Container status
```bash
docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
```

## Key facts
- Backend listens on **:8080** internally
- Nginx proxies from 80/443 → backend
- Frontend at `/root/yeet-html/index.html`
- Network: `yeet-social_yeet-net`
- Secrets: in `/root/yeet-social/.env` (mode 600, see `vps/.env.example`)

## Secrets / .env (VPS)
`docker-compose.yml` and `docker compose (start_backend.sh was removed in the 2026-10 audit: it edited pg_hba.conf, ran ad-hoc DDL and published :8080 on all interfaces)` both expect these at
`/root/yeet-social/.env`:
```
POSTGRES_PASSWORD=...
JWT_SECRET=...
ADMIN_SECRET=...
RUST_LOG=backend=info,tower_http=warn   # optional

# SMTP for GDPR double-opt-in verification email (Hostinger)
SMTP_HOST=smtp.hostinger.com
SMTP_PORT=465
SMTP_USER=noreply@justyeet.it
SMTP_PASS=...
SMTP_FROM=noreply@justyeet.it
PUBLIC_BASE_URL=https://justyeet.it

# Post translation: azure (2M chars/month free) | google (500k free) | deepl | libretranslate. Empty = off.
TRANSLATE_PROVIDER=azure
TRANSLATE_API_KEY=...
TRANSLATE_REGION=germanywestcentral   # Azure only
```
Generate fresh values with `openssl rand -hex 64` and `openssl rand -hex 32`,
then `chmod 600 /root/yeet-social/.env`. Never commit.

After changing `/root/yeet-social/.env`, restart the backend container:
```bash
docker compose -f /root/yeet-social/docker-compose.yml restart yeet-api
# or, if running via docker compose (start_backend.sh was removed in the 2026-10 audit: it edited pg_hba.conf, ran ad-hoc DDL and published :8080 on all interfaces):
bash /tmp/docker compose (start_backend.sh was removed in the 2026-10 audit: it edited pg_hba.conf, ran ad-hoc DDL and published :8080 on all interfaces)
```

Migration `0020_email_verification.sql` adds the
`email_verification_tokens` table and `email_verified_at` / `email_pending`
columns; it runs automatically on backend start.

To rotate the JWT secret without breaking active sessions, deploy the new
secret during low-traffic windows  all existing JWTs become invalid and users
must re-login.

## Deploy-User statt root (Audit I-H1)

Die CD loggt sich standardmäßig noch als `root` ein. Umstellung auf das unprivilegierte Konto
`deploy` in vier Schritten (einmalig, ca. 5 Minuten, ohne Downtime):

1. Auf dem VPS als root: `bash vps/setup-deploy-user.sh` (liegt im Repo; `setup.sh` ruft es auf
   frischen Servern automatisch auf). Legt `deploy` an (Docker-Gruppe, kein Passwort, CD-Key nur in
   seinem `authorized_keys`), kopiert `.env`, Compose-Datei, `nginx.conf`, Static-HTML und Backups
   nach `/opt/yeet/{app,html,backups}` und trägt `YEET_HTML_DIR`/`YEET_NGINX_CONF` in die kopierte
   `.env` ein. Die `/root`-Kopien bleiben als Fallback liegen.
2. In GitHub: Settings → Secrets and variables → Actions → **Variables** → `DEPLOY_USER` = `deploy`.
   Die CD nimmt den Wert beim nächsten Lauf (`username: ${{ vars.DEPLOY_USER || 'root' }}`) und
   wählt die `/opt/yeet`-Pfade automatisch, sobald sie nicht als uid 0 läuft.
3. Einen Deploy auslösen (Merge oder „Re-run" der CD) und im Log `Starting deploy … as deploy` sowie
   die Health-Zeile prüfen. Der Compose-Projektname ist auf `yeet-social` gepinnt, deshalb benutzt
   der Stack unter `/opt/yeet/app` dieselben Volumes wie zuvor (keine leere zweite Datenbank).
4. Danach aufräumen: CD-Schlüssel-Zeile (`github-cd`) aus `/root/.ssh/authorized_keys` löschen und in
   `/etc/ssh/sshd_config` `PermitRootLogin no` setzen (`systemctl reload sshd`). `backup.sh` per
   `BACKUP_DIR=/opt/yeet/backups` betreiben bzw. den Cron auf den `deploy`-User umziehen.

Rückweg: Variable `DEPLOY_USER` löschen → die CD läuft wieder als root mit den `/root`-Pfaden.
Hinweis: Mitglied der Docker-Gruppe zu sein ist auf dem Host root-äquivalent; der Gewinn ist eine
eigene, widerrufbare Identität ohne Root-Shell. Rootless Docker wäre der nächste Schritt.

## Ops-Workflow (Server-Aufgaben per Knopfdruck)

Actions → „Ops — server tasks (manual)" → *Run workflow* → Task wählen. Läuft mit demselben Key wie
die CD, bevorzugt als `deploy`, sonst als root.

| Task | Wirkung |
| --- | --- |
| `status` | Konten, Platte/RAM, Container, Health |
| `setup-deploy-user` | legt `deploy` an und kopiert den Zustand nach `/opt/yeet` (als root) |
| `finalize-deploy-user` | entfernt den CD-Key aus roots `authorized_keys` — nur wenn der letzte Deploy als `deploy` lief. `PermitRootLogin` bleibt unangetastet (eigener Zugang!) |
| `note-install` (erneut ausführen nach Änderungen an `vps/note-node`: baut das neue Image im Hintergrund und stellt erst danach um) | baut den Note-Daemon aus gepinntem Quellcode (v1.1.0.0, Hintergrund, ~20–40 min), erzeugt RPC-Zugang in `.env`, startet `yeet-note` |
| `note-status` | Build-Log, Container, Blockhöhe/Peers, Plattenverbrauch |
| `note-import` | spielt eine Chain-Sicherung ein; `arg` = URL oder absoluter Server-Pfad (optional gefolgt von ` sha256=<hex>`) zu `.7z`/`.tar.gz`/`.zip` mit `blocks/`+`chainstate/` oder `bootstrap.dat`. Alte Daten bleiben als `*.old`, die Wallet wird nicht angefasst |
| `note-addnodes` | setzt zusätzliche Peers (`arg` = `ip:port,ip:port`) in `.env` und startet den Dienst neu |
| `note-mine` | lässt den Knoten selbst Blöcke erzeugen (`arg` = Anzahl oder `stop`); läuft im Hintergrund, Fortschritt per `note-status`, Belohnung an eine Adresse der Knoten-Wallet |
| `note-miner-access` | externer Miner: `arg` = `on <salt> <hmac> <ssh-public-key>` richtet einen zweiten RPC-Zugang (nur Salt+HMAC gespeichert), `maxtipage` und einen SSH-Schlüssel ein, der ausschließlich den Tunnel zu `127.0.0.1:9332` öffnen darf; `off` entfernt alles wieder. Vorher `note-install` (Image `-lr1b`) |
| `note-logs` | durchsucht das Knoten-Log; `arg` = `[Stunden] [Regex]`, Standard: 24 h und alles, was nach einem Problem aussieht |
| `note-restart` | Dienst neu erstellen (z. B. nach `NOTE_ADDNODES=ip:port,…` in `.env`) |

Die CD versucht seit dem 16. Nov. zuerst `deploy` und fällt auf root zurück; nach
`setup-deploy-user` wechselt sie also von selbst, eine Repo-Variable ist nicht mehr nötig.
Nach dem Wechsel ist `/opt/yeet/app/.env` maßgeblich.

## Note-Node

Dienst `note-node` (Container `yeet-note`) im Compose-Profil `note`; RPC nur im internen Netz
(`http://yeet-note:9332`), kein Host-Port. Volume `yeet-social_note_data` enthält Chain **und
`wallet.dat`** (Einzahlungsschlüssel des Swaps) → ins Backup aufnehmen. `SWAP_ENABLED` bleibt
`false`, bis der Token-Contract steht; der Node kann vorher synchronisieren. Lädt die Chain nicht:
Peers per `NOTE_ADDNODES` setzen und `note-restart`, sonst Sicherung per `note-import`.

## Admin-2FA

Der Admin-Bereich verlangt zusätzlich zum Secret einen Authenticator-Code. Beim ersten Öffnen
nach dem Deploy zeigt das Panel den QR-Code zur Einrichtung — **sofort erledigen**, bis dahin kann
jeder mit dem Secret den Authenticator einrichten. Verlorener Authenticator ohne
Wiederherstellungscodes: `docker exec yeet-postgres psql -U yeet yeet -c "UPDATE admin_totp SET
secret_enc=NULL, pending_enc=NULL, enabled_at=NULL, recovery_hashes='{}' WHERE id=1"` und neu
einrichten.
