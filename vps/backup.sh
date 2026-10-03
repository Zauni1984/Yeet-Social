#!/bin/bash
# Nightly backup of everything that cannot be rebuilt from git: the Postgres
# database (users, points, the append-only ledger: 10-year retention duty,
# docs/mica/09 section 8), uploaded media and the encrypted age-verification vault.
#
# Install on the VPS:   chmod +x /root/yeet-social/vps/backup.sh
#   crontab -e  ->  15 3 * * * /root/yeet-social/vps/backup.sh >> /var/log/yeet-backup.log 2>&1
# Off-host copy (strongly recommended): set BACKUP_RCLONE_REMOTE (e.g.
# "b2:yeet-backups") after `rclone config`; the script then uploads each run.
set -euo pipefail
DEST=${BACKUP_DIR:-/root/yeet-backups}
KEEP_DAYS=${BACKUP_KEEP_DAYS:-30}
STAMP=$(date +%Y%m%d-%H%M%S)
mkdir -p "$DEST"

docker exec yeet-postgres pg_dump -U yeet --no-owner yeet | gzip -9 > "$DEST/db-$STAMP.sql.gz"
docker run --rm -v yeet-social_yeet_uploads:/src:ro -v "$DEST":/dst alpine \
  tar czf "/dst/uploads-$STAMP.tgz" -C /src . 2>/dev/null || echo "uploads volume not found (name differs?)"
docker run --rm -v yeet-social_yeet_private:/src:ro -v "$DEST":/dst alpine \
  tar czf "/dst/private-$STAMP.tgz" -C /src . 2>/dev/null || echo "private volume not found (age verification unused?)"

# Encrypt at rest when a recipient is configured (age), else keep as is.
if [ -n "${BACKUP_AGE_RECIPIENT:-}" ] && command -v age >/dev/null; then
  for f in "$DEST"/*-"$STAMP".*; do age -r "$BACKUP_AGE_RECIPIENT" -o "$f.age" "$f" && rm -f "$f"; done
fi

find "$DEST" -type f -mtime +"$KEEP_DAYS" -delete
if [ -n "${BACKUP_RCLONE_REMOTE:-}" ] && command -v rclone >/dev/null; then
  rclone copy "$DEST" "$BACKUP_RCLONE_REMOTE" --include "*-$STAMP.*"
fi
echo "[backup] $STAMP done -> $DEST (keep ${KEEP_DAYS}d)"
