#!/bin/bash
# Production deploy, run on the VPS by the CD workflow (fetched at the exact
# commit CI tested). Works as root (legacy paths under /root) or as the
# unprivileged `deploy` account (/opt/yeet). DEPLOY_SHA comes from the caller.
: "${DEPLOY_SHA:?DEPLOY_SHA not set}"
set -e
echo "[CD] $(date) — Starting deploy of ${DEPLOY_SHA} as $(id -un)"
if [ "$(id -u)" = "0" ]; then
  DEPLOY_DIR=/root/yeet-social
  HTML_DIR=/root/yeet-html
  NGINX_CONF=/root/yeet-nginx.conf
  BACKUP_DIR=/root/yeet-backups
else
  # Unprivileged deploy account (vps/setup-deploy-user.sh).
  DEPLOY_DIR=/opt/yeet/app
  HTML_DIR=/opt/yeet/html
  NGINX_CONF=/opt/yeet/app/nginx.conf
  BACKUP_DIR=/opt/yeet/backups
fi
mkdir -p $DEPLOY_DIR $HTML_DIR $BACKUP_DIR

REPO_RAW=https://raw.githubusercontent.com/Zauni1984/Yeet-Social/${DEPLOY_SHA}

curl -fsSL "$REPO_RAW/docker-compose.yml" -o $DEPLOY_DIR/docker-compose.yml
curl -fsSL "$REPO_RAW/nginx.conf"          -o $DEPLOY_DIR/nginx.conf
[ "$DEPLOY_DIR/nginx.conf" -ef "$NGINX_CONF" ] || cp $DEPLOY_DIR/nginx.conf $NGINX_CONF

if [ ! -f $DEPLOY_DIR/.env ]; then
  echo "ERROR: $DEPLOY_DIR/.env missing! Create it on the VPS first."
  exit 1
fi

cd $DEPLOY_DIR

# Pre-deploy safety copy of the database (ledger!), kept 7 days.
docker exec yeet-postgres pg_dump -U yeet yeet 2>/dev/null | gzip > "$BACKUP_DIR/predeploy-$(date +%Y%m%d-%H%M%S).sql.gz" || echo "[CD] WARN: pre-deploy dump failed (first deploy?)"
find $BACKUP_DIR -name 'predeploy-*.sql.gz' -mtime +7 -delete || true

docker pull ghcr.io/zauni1984/yeet-social/backend:main
docker compose up -d --remove-orphans

curl -fsSL "$REPO_RAW/frontend/index.html" -o $HTML_DIR/index.html.new
mv $HTML_DIR/index.html.new $HTML_DIR/index.html
ls -la $HTML_DIR/index.html

# Admin moderation dashboard is a separate static file served
# from the same docroot — keep it in sync on every deploy too.
curl -fsSL "$REPO_RAW/frontend/admin.html" -o $HTML_DIR/admin.html.new
mv $HTML_DIR/admin.html.new $HTML_DIR/admin.html
ls -la $HTML_DIR/admin.html

# Public NOTE→YEET swap page (swaps locked until the contract
# is live) — same static docroot, synced every deploy.
curl -fsSL "$REPO_RAW/frontend/swap.html" -o $HTML_DIR/swap.html.new
mv $HTML_DIR/swap.html.new $HTML_DIR/swap.html
ls -la $HTML_DIR/swap.html

# Developer API page + OpenAPI spec (docs/api). Without these
# copies nginx's SPA fallback serves index.html for /api.html.
# The public rewards guide (docs/rewards → scripts/build-rewards-pdf.cjs) is
# linked from the footer and must be a real file in the docroot as well.
for f in api.html openapi.json yeet-rewards-de.pdf yeet-rewards-en.pdf; do
  curl -fsSL "$REPO_RAW/frontend/$f" -o "$HTML_DIR/$f.new"
  mv "$HTML_DIR/$f.new" "$HTML_DIR/$f"
  ls -la "$HTML_DIR/$f"
done

# Web app manifest, icons and Android asset links: the installable app and
# the Play Store wrapper (docs/play-store.md) need them as real files.
mkdir -p "$HTML_DIR/icons" "$HTML_DIR/.well-known"
for f in manifest.json .well-known/assetlinks.json \
         icons/icon-192.png icons/icon-512.png icons/maskable-192.png icons/maskable-512.png \
         icons/monochrome-512.png icons/splash-512.png icons/apple-touch-icon.png \
         icons/favicon-32.png icons/favicon-48.png; do
  curl -fsSL "$REPO_RAW/frontend/$f" -o "$HTML_DIR/$f.new"
  mv "$HTML_DIR/$f.new" "$HTML_DIR/$f"
done
ls -la "$HTML_DIR/manifest.json" "$HTML_DIR/icons" "$HTML_DIR/.well-known"

docker exec yeet-nginx nginx -s reload || \
  docker compose restart nginx

sleep 10
STATUS=$(curl -sk http://127.0.0.1:8080/api/v1/health 2>/dev/null || echo "error")
echo "[CD] Backend health: $STATUS"
FE_HEAD=$(curl -sI http://127.0.0.1/ 2>/dev/null | head -1 || echo "no-fe")
echo "[CD] Frontend HEAD: $FE_HEAD"
docker ps --format "table {{.Names}}\t{{.Status}}"
# Marker for vps/ops.sh finalize-deploy-user: who ran the last good deploy.
id -un > "$DEPLOY_DIR/.last_deploy_user" 2>/dev/null || true
echo "[CD] Done: $(date)"
