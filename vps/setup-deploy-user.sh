#!/bin/bash
# Unprivileged deploy account for GitHub Actions (audit I-H1).
#
# Run ONCE as root on the VPS:
#   bash vps/setup-deploy-user.sh
#
# What it does:
#   1. creates the user `deploy` (no password, docker group) with the CD
#      public key in its own authorized_keys (no root login for CD any more);
#   2. creates /opt/yeet/{app,html,backups} owned by `deploy` and copies the
#      current state over (.env, compose file, nginx.conf, static html,
#      backups) — the /root copies are left untouched as a fallback;
#   3. writes YEET_HTML_DIR / YEET_NGINX_CONF into /opt/yeet/app/.env so the
#      nginx container mounts the new paths.
#
# Afterwards (manual, see vps/DEPLOY.md → "Deploy-User statt root"):
#   - set the repository variable DEPLOY_USER=deploy; the next deploy logs in
#     as `deploy` and uses /opt/yeet;
#   - once a deploy went through, delete the CD key line (github-cd) from
#     /root/.ssh/authorized_keys and set `PermitRootLogin no` in sshd_config.
#
# Note: membership in the `docker` group is root-equivalent on the host (it
# can start privileged containers). The gain is a dedicated, revocable
# identity without a root shell; rootless Docker would be the next step.
set -euo pipefail

DEPLOY_USER=${DEPLOY_USER:-deploy}
APP_HOME=/opt/yeet
OLD_DIR=/root/yeet-social
OLD_HTML=/root/yeet-html
OLD_CONF=/root/yeet-nginx.conf
OLD_BACKUPS=/root/yeet-backups
# The CD public key is taken from root's authorized_keys (the line whose
# comment is github-cd or github-actions-deploy), so it is always the key the
# VPS_SSH_KEY secret really belongs to.
CD_KEY_RE='(github-cd|github-actions-deploy)[[:space:]]*$'

[ "$(id -u)" = "0" ] || { echo "run as root"; exit 1; }
command -v docker >/dev/null || { echo "docker is not installed (run vps/setup.sh first)"; exit 1; }

# 1. account + key
if ! id -u "$DEPLOY_USER" >/dev/null 2>&1; then
  useradd --create-home --shell /bin/bash "$DEPLOY_USER"
  echo "created user $DEPLOY_USER"
fi
passwd -l "$DEPLOY_USER" >/dev/null 2>&1 || true
usermod -aG docker "$DEPLOY_USER"
HOME_DIR=$(getent passwd "$DEPLOY_USER" | cut -d: -f6)
install -d -m 700 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "$HOME_DIR/.ssh"
touch "$HOME_DIR/.ssh/authorized_keys"
CD_KEYS=$(grep -E "$CD_KEY_RE" /root/.ssh/authorized_keys 2>/dev/null | grep -oE '(ssh-(ed25519|rsa)|ecdsa-[a-z0-9-]+) [A-Za-z0-9+/=]+ .*$' || true)
[ -n "$CD_KEYS" ] || { echo "no CD key (comment github-cd / github-actions-deploy) in /root/.ssh/authorized_keys"; exit 1; }
while IFS= read -r K; do
  BLOB=$(echo "$K" | awk '{print $2}')
  grep -qF "$BLOB" "$HOME_DIR/.ssh/authorized_keys" || \
    echo "no-port-forwarding,no-agent-forwarding,no-X11-forwarding $K" >> "$HOME_DIR/.ssh/authorized_keys"
done <<< "$CD_KEYS"
echo "deploy has $(grep -c . "$HOME_DIR/.ssh/authorized_keys") CD key(s)"
chmod 600 "$HOME_DIR/.ssh/authorized_keys"
chown -R "$DEPLOY_USER:$DEPLOY_USER" "$HOME_DIR/.ssh"
grep -q "^PubkeyAuthentication" /etc/ssh/sshd_config || echo "PubkeyAuthentication yes" >> /etc/ssh/sshd_config
systemctl reload sshd || systemctl reload ssh || true

# 2. state under /opt/yeet
install -d -m 750 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "$APP_HOME" "$APP_HOME/app" "$APP_HOME/html" "$APP_HOME/backups"
if [ -f "$OLD_DIR/.env" ] && [ ! -f "$APP_HOME/app/.env" ]; then
  install -m 600 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "$OLD_DIR/.env" "$APP_HOME/app/.env"
  echo "copied .env"
fi
[ -f "$OLD_DIR/docker-compose.yml" ] && install -m 640 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "$OLD_DIR/docker-compose.yml" "$APP_HOME/app/docker-compose.yml"
if   [ -f "$OLD_DIR/nginx.conf" ]; then install -m 640 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "$OLD_DIR/nginx.conf" "$APP_HOME/app/nginx.conf"
elif [ -f "$OLD_CONF" ];           then install -m 640 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "$OLD_CONF"           "$APP_HOME/app/nginx.conf"; fi
if [ -d "$OLD_HTML" ]; then cp -a "$OLD_HTML/." "$APP_HOME/html/"; fi
if [ -d "$OLD_BACKUPS" ]; then cp -a "$OLD_BACKUPS/." "$APP_HOME/backups/"; fi
chown -R "$DEPLOY_USER:$DEPLOY_USER" "$APP_HOME"

# 3. compose paths for the nginx mounts (defaults in docker-compose.yml stay
#    the /root paths, so nothing changes until this .env is used)
ENV_FILE="$APP_HOME/app/.env"
if [ -f "$ENV_FILE" ]; then
  grep -q "^YEET_HTML_DIR="   "$ENV_FILE" || echo "YEET_HTML_DIR=$APP_HOME/html"             >> "$ENV_FILE"
  grep -q "^YEET_NGINX_CONF=" "$ENV_FILE" || echo "YEET_NGINX_CONF=$APP_HOME/app/nginx.conf" >> "$ENV_FILE"
else
  echo "WARN: no .env found at $OLD_DIR/.env — create $ENV_FILE before the first deploy"
fi

echo
echo "=== done ==="
echo "Next: set the GitHub repository variable DEPLOY_USER=$DEPLOY_USER"
echo "      (Settings → Secrets and variables → Actions → Variables),"
echo "      then push or re-run CD. After the first successful deploy as"
echo "      $DEPLOY_USER, remove the CD key line (github-cd) from"
echo "      /root/.ssh/authorized_keys and set PermitRootLogin no."
