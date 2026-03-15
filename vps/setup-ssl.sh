#!/bin/bash
set -euo pipefail
[[ $EUID -ne 0 ]] && echo "Als root ausführen" && exit 1
read -rp "Domain (z.B. yeet.social): " DOMAIN
read -rp "E-Mail für Let's Encrypt: " EMAIL
certbot --nginx -d "$DOMAIN" --non-interactive --agree-tos --email "$EMAIL" --redirect
# .env API_URL aktualisieren
sed -i "s|API_URL=.*|API_URL=https://${DOMAIN}/api/v1|" /opt/yeet/.env
systemctl enable certbot.timer && systemctl start certbot.timer
echo "✅ SSL aktiv: https://$DOMAIN"
echo "✅ Auto-Renewal konfiguriert"
