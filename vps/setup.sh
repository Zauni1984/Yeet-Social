#!/bin/bash
# ============================================================
#  Yeet Social — VPS Setup Script
#  VPS: 76.13.150.206 (IPv4) / 2a02:4780:79:1156::1 (IPv6)
#  Ubuntu 22.04 / 24.04 | Hostinger | Root SSH
#  Ausführen: bash setup.sh
# ============================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()    { echo -e "${BLUE}[INFO]${NC}  $1"; }
success() { echo -e "${GREEN}[OK]${NC}    $1"; }
warn()    { echo -e "${YELLOW}[WARN]${NC}  $1"; }
error()   { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

echo -e "${BLUE}"

cat << 'LOGO'
  ██╗   ██╗███████╗███████╗████████╗
  ╚██╗ ██╔╝██╔════╝██╔════╝╚══██╔══╝
   ╚████╔╝ █████╗  █████╗     ██║
   ╚██╔╝  ██╔══╝  ██╔══╝      ██║
     ██║   ███████╗███████╗   ██║
     ╚═╝   ╚══════╝╚══════╝   ╚═╝
LOGO

echo -e "${NC}"
echo "  VPS: 76.13.150.206 | Ubuntu 22.04/24.04"
echo ""

[[ $EUID -ne 0 ]] && error "Bitte als root ausführen: bash setup.sh"

VPS_IP="76.13.150.206"
VPS_IP6="2a02:4780:79:1156::1"
DOMAIN="$VPS_IP"

read -rp "Deploy-User anlegen (Standard: yeet): " DEPLOY_USER
[[ -z "$DEPLOY_USER" ]] && DEPLOY_USER="yeet"

read -rsp "Datenbank-Passwort: " DB_PASS; echo
[[ ${#DB_PASS} -lt 8 ]] && error "Passwort zu kurz!"

read -rsp "JWT Secret (leer = auto): " JWT_SECRET; echo
[[ -z "$JWT_SECRET" ]] && JWT_SECRET=$(openssl rand -hex 64)

echo ""
info "Konfiguration:"
info "  VPS IPv4:     $VPS_IP"
info "  VPS IPv6:     $VPS_IP6"
info "  Deploy-User:  $DEPLOY_USER"
echo ""

info "1/9 System aktualisieren..."
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get upgrade -y -qq
apt-get install -y -qq curl wget git unzip htop nano ufw fail2ban ca-certificates gnupg lsb-release openssl
success "System aktualisiert"

info "2/9 Deploy-User '$DEPLOY_USER' anlegen..."
if ! id "$DEPLOY_USER" &>/dev/null; then
    useradd -m -s /bin/bash "$DEPLOY_USER"
    usermod -aG sudo "$DEPLOY_USER"
    mkdir -p /home/$DEPLOY_USER/.ssh
    [[ -f /root/.ssh/authorized_keys ]] && cp /root/.ssh/authorized_keys /home/$DEPLOY_USER/.ssh/
    chown -R $DEPLOY_USER�$DEPLOY_USER /home/$DEPLOY_USER/.ssh
    chmod 700 /home/$DEPLOY_USER/.ssh
    chmod 600 /home/$DEPLOY_USER/.ssh/authorized_keys 2>/dev/null || true
    success "User '$DEPLOY_USER' erstellt"
else
    warn "User existiert bereits"
fi

info "3/9 Firewall (UFW)..."
ufw --force reset
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp
ufw allow 80/tcp
ufw allow 443/tcp
sed -i 's/IPV6=no/IPV6=yes/' /etc/default/ufw 2>/dev/null || true
ufw --force enable
success "Firewall aktiv (22, 80, 443)"

info "4/9 Fail2Ban..."
cat > /etc/fail2ban/jail.local <<EOF
[DEFAULT]
bantime=3600
findtime=600
maxretry=5
[sthd]
enabled=true
port=ssh
EOF
systemctl enable fail2ban --quiet
systemctl restart fail2ban
success "Fail2Ban aktiv"

info "5/9 Docker installieren..."
if ! command -v docker &>/dev/null; then
    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL https://download.docker.com/linux/ubuntu/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    chmod a+r /etc/apt/keyrings/docker.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list > /dev/null
    apt-get update -qq
    apt-get install -y -qq docker-ce docker-ce-cli containerd.io docker-compose-plugin
    systemctl enable docker --quiet
    systemctl start docker
    usermod -aG docker $DEPLOY_USER
    success "Docker installiert"
else
    warn "Docker bereits installiert"
    usermod -aG docker $DEPLOY_USER 2>/dev/null || true
fi

info "6/9 Nginx (IPv4 + IPv6)..."
apt-get install -y -qq nginx certbot python3-certbot-nginx
systemctl enable nginx --quiet

cat > /etc/nginx/sites-available/yeet <<EOF
server {
    listen 80;
    listen [::]:80;
    server_name $VPS_IP $VPS_IP6 _;

    location /api/ {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_read_timeout 60s;
    }
    location /ws/ {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade \$http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 3600s;
    }
    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
    }
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;
    access_log /var/log/nginx/yeet_access.log;
    error_log  /var/log/nginx/yeet_error.log;
}
EOF

ln -sf /etc/nginx/sites-available/yeet /etc/nginx/sites-enabled/yeet
rm -f /etc/nginx/sites-enabled/default
nginx -t && systemctl reload nginx
success "Nginx konfiguriert"

info "7/9 App-Verzeichnis /opt/yeet..."
APP_DIR="/opt/yeet"
mkdir -p $APP_DIR
chown $DEPLOY_USER�$DEPLOY_USER $APPE_DIR

cat > $APP_DIR/.env <<EOF
DATABASE_URL=postgres://yeet:${DB_PASS}@postgres:5432/yeet
POSTGRES_PASSWORD=$(DB_PASS)
REDIS_URL=redis://redis:6379
JWT_SECRET=${JWT_SECRET}
BSC_RPC_URL=https://bsc-dataseed.binance.org/
YEET_TOKEN_ADDRESS=0x0000000000000000000000000000000000000000
YEET_NFT_ADDRESS=0x0000000000000000000000000000000000000000
REWARDS_MINTER_PRIVKEY=
PORT=8080
RUST_LOG=backend=info,bower_http=warn
API_URL=http://${VPS_IP}/api/v1
EOF

chmod 600 $APP_DIR/.env
chown $DEPLOY_USER:$DEPLOY_USER $APP_DIR/.env

cat > $APP_DIR/docker-compose.yml <<'COMPOSE'
version: "3.9"
services:
  postgres:
    image: postgres:16-alpine
    container_name: yeet-postgres
    restart: always
    environment:
      POSTGRES_DB: yeet
      POSTGRES_USER: yeet
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD}
    volumes: [postgres_data:/var/lib/postgresql/data]
    networks: [yeet-net]
    expose: ["5432"]
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U yeet"]
      interval: 10s
      retries: 5
  redis:
    image: redis:7-alpine
    container_name: yeet-redis
    restart: always
    command: redis-server --maxmemory 256mb --maxmemory-policy allkeys-lru
    volumes: [redis_data:/data]
    networks: [yeet-net]
    expose: ["6379"]
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      retries: 5
  backend:
    image: ghcr.io/zauni1984/yeet-social/backend:main
    container_name: yeet-backend
    restart: always
    depends_on:
      postgres: {condition: service_healthy}
      redis: {condition: service_healthy}
    env_file: [.env]
    networks: [yeet-net]
    ports: ["127.0.0.1:8080:8080"]
  frontend:
    image: ghcr.io/zauni1984/yeet-social/frontend:main
    container_name: yeet-frontend
    restart: always
    networks: [yeet-net]
    ports: ["127.0.0.1:3000:80"]
networks:
  yeet-net:
    driver: bridge
volumes:
  postgres_data:
  redis_data:
COMPOSE

chown $DEPLOY_USER:$DEPLOY_USER $APP_DIR/docker-compose.yml
success "App-Verzeichnis eingerichtet"

info "8/9 SSH Deploy-Key..."
DEPLOY_KEY="/home/$DEPLOY_USER/.ssh/yeet_deploy"
if [[ ! -f "$DEPLOY_KEY" ]]; then
    sudo -u $DEPLOY_USER ssh-keygen -t ed25519 -C "yeet-deploy@$VPS_IP" -f "$DEPLOY_KEY" -N ""
    cat "$DEPLOY_KEY.pub" >> /home/$DEPLOY_USER/.ssh/authorized_keys
    chmod 600 /home/$DEPLOY_USER/.ssh/authorized_keys
fi

info "9/9 Systemd Autostart..."
cat > /etc/systemd/system/yeet.service <<EOF
[Unit]
Description=Yeet Social Media
After=docker.service network-online.target
Requires=docker.service
[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=/opt/yeet
ExecStart=/usr/bin/docker compose up -d
ExecStop=/usr/bin/docker compose down
TimeoutStartSec=300
User=$DEPLOY_USER
[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable yeet.service --quiet
success "Autostart aktiviert"

echo ""
echo -e "${GREEN}╔══════════════════════════════════════════════════════════════╗�{.NC}"
echo -e "${GREEN}║  ✅  VPS SETUP ABGESCHLOSSEN                               ║${NC}"
echo -e "${GREEN}╠══════════════════════════════════════════════════════════════╣${NC}"
echo -e "${YELLOW}  GITHUB SECRETS: ${NC}"
echo -e "  SSH_HOST = $VPS_IP"
echo -e "  SSH_USER = $DEPLOY_USER"
echo -e "  SSH_PORT = 22"
echo -e "  SSH_PRIVATE_KEY = (siehe unten)"
echo ""
echo -e "${YELLOW}Private Key:${NC}"
cat "$DEPLOY_KEY"
echo ""
echo -e "${YELLOW}NÄCHSTE SCHRITTE:${NC}"
echo "  1. Private Key → GitHub Secrets: SSH_PRIVATE_KEY"
echo "  2. git push origin main → Deployment startet"
echo "  3. App testen: http://$VPS_IP"
echo "  4. SSL: bash vps/setup-ssl.sh"
