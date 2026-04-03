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
cat > $DEPLOY_DIR/.env << EOF
POSTGRES_PASSWORD=YeetDB_5254a44ceae0a4a7!
DATABASE_URL=postgres://yeet:YeetDB_5254a44ceae0a4a7!@yeet-postgres:5432/yeet
REDIS_URL=redis://yeet-redis:6379
JWT_SECRET=f270e9a02377765cf70ac4ccf1e35af55be8e7d3bac3c71e08e5e17eed62a6c2310d8a24b3d23e4d
RUST_LOG=backend=info,tower_http=warn
EOF

# Pull configs
curl -sL "https://raw.githubusercontent.com/Zauni1984/Yeet-Social/main/docker-compose.yml" -o $DEPLOY_DIR/docker-compose.yml
curl -sL "https://raw.githubusercontent.com/Zauni1984/Yeet-Social/main/nginx.conf" -o $DEPLOY_DIR/nginx.conf

# Add SSH key for GitHub Actions
echo "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDzuDAis6M5T4NdVli/tfPrE4JVE+HxXBS7q6LdKWV25 github-actions-deploy" >> ~/.ssh/authorized_keys

# Enable PubkeyAuthentication
grep -q "^PubkeyAuthentication" /etc/ssh/sshd_config || echo "PubkeyAuthentication yes" >> /etc/ssh/sshd_config
systemctl reload sshd

cd $DEPLOY_DIR
docker compose up -d

echo ""
echo "=== Setup complete! ==="
echo "Stack is starting. Check with: docker ps"
