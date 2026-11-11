#!/bin/bash
# VPS Fresh Setup Script for YEET Social
# Run this ONCE on a fresh VPS before enabling CD

set -e
echo "=== YEET Social VPS Setup ==="

DEPLOY_DIR=/root/yeet-social
mkdir -p $DEPLOY_DIR

# Install Docker if not present
if ! command -v docker &> /dev/null; then
  curl -fsSL https://get.docker.com | sh
  systemctl enable docker
  systemctl start docker
fi

# Create .env file
# Create .env with freshly generated secrets (never commit real values).
if [ -f "$DEPLOY_DIR/.env" ]; then
  echo "$DEPLOY_DIR/.env exists, keeping it."
else
  PG_PW="$(openssl rand -hex 24)"
  cat > "$DEPLOY_DIR/.env" << EOF
POSTGRES_PASSWORD=${PG_PW}
DATABASE_URL=postgres://yeet:${PG_PW}@yeet-postgres:5432/yeet
REDIS_URL=redis://yeet-redis:6379
JWT_SECRET=$(openssl rand -hex 64)
ADMIN_SECRET=$(openssl rand -hex 32)
AGE_VERIFY_KEY=$(openssl rand -hex 32)
RUST_LOG=backend=info,tower_http=warn
# Chain: 97 = BSC Testnet (test phase), 56 = Mainnet
YEET_CHAIN_ID=97
BSC_RPC_URL=https://bsc-testnet-dataseed.bnbchain.org
EOF
  chmod 600 "$DEPLOY_DIR/.env"
  echo "Generated $DEPLOY_DIR/.env; store ADMIN_SECRET from it in your password manager."
fi

# Pull configs
curl -fsSL "https://raw.githubusercontent.com/Zauni1984/Yeet-Social/main/docker-compose.yml" -o $DEPLOY_DIR/docker-compose.yml
curl -fsSL "https://raw.githubusercontent.com/Zauni1984/Yeet-Social/main/nginx.conf" -o $DEPLOY_DIR/nginx.conf

cd $DEPLOY_DIR
docker compose up -d

# GitHub Actions deploys as the unprivileged `deploy` account (audit I-H1):
# the CD key never goes into root's authorized_keys. The helper also moves
# the deploy state to /opt/yeet and prints the remaining manual steps.
curl -fsSL "https://raw.githubusercontent.com/Zauni1984/Yeet-Social/main/vps/setup-deploy-user.sh" -o /tmp/setup-deploy-user.sh
bash /tmp/setup-deploy-user.sh

echo ""
echo "=== Setup complete! ==="
echo "Stack is starting. Check with: docker ps"
