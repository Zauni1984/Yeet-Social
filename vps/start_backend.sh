#!/bin/bash
# YEET Social — Backend Start Script
# Includes: pg_hba fix, DB migrations, correct column types

set +H

docker rm -f yeet-backend 2>/dev/null || true
docker pull ghcr.io/zauni1984/yeet-social/backend:main

# Fix pg_hba.conf (removes scram-sha-256 lines that appear after container restarts)
docker exec yeet-postgres sh -c "grep -v 'scram-sha-256' /var/lib/postgresql/data/pg_hba.conf > /tmp/f && mv /tmp/f /var/lib/postgresql/data/pg_hba.conf" 2>/dev/null || true
docker exec yeet-postgres psql -U yeet -d yeet -c "SELECT pg_reload_conf();" 2>/dev/null || true

# Apply all DB migrations (idempotent)
docker exec yeet-postgres psql -U yeet -d yeet -c "
ALTER TABLE posts ADD COLUMN IF NOT EXISTS is_adult BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE posts ADD COLUMN IF NOT EXISTS media_url TEXT;
ALTER TABLE posts ADD COLUMN IF NOT EXISTS is_nft BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE posts ADD COLUMN IF NOT EXISTS nft_price_yeet DOUBLE PRECISION;
ALTER TABLE posts ADD COLUMN IF NOT EXISTS is_permanent BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE posts ADD COLUMN IF NOT EXISTS ppv_price_yeet DOUBLE PRECISION;
ALTER TABLE posts ADD COLUMN IF NOT EXISTS reshare_count BIGINT NOT NULL DEFAULT 0;
ALTER TABLE posts ADD COLUMN IF NOT EXISTS tip_total_yeet DOUBLE PRECISION;
ALTER TABLE users ADD COLUMN IF NOT EXISTS avatar_url TEXT;
CREATE TABLE IF NOT EXISTS fee_ledger (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_type TEXT NOT NULL,
    source_id UUID,
    gross_amount DOUBLE PRECISION NOT NULL,
    fee_amount DOUBLE PRECISION NOT NULL,
    creator_amount DOUBLE PRECISION NOT NULL,
    fee_wallet TEXT NOT NULL DEFAULT '0xFEE_DUMMY_TESTNET_YEET_PLATFORM_WALLET_001',
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE TABLE IF NOT EXISTS fee_wallet_balance (
    id INTEGER PRIMARY KEY DEFAULT 1,
    total_yeet DOUBLE PRECISION NOT NULL DEFAULT 0,
    last_transfer_at TIMESTAMPTZ,
    cold_wallet TEXT NOT NULL DEFAULT '0xCOLD_DUMMY_TESTNET_YEET_COLD_WALLET_001'
);
INSERT INTO fee_wallet_balance (id, total_yeet) VALUES (1, 0) ON CONFLICT (id) DO NOTHING;
ALTER TABLE posts ALTER COLUMN nft_price_yeet TYPE DOUBLE PRECISION USING nft_price_yeet::DOUBLE PRECISION;
ALTER TABLE posts ALTER COLUMN ppv_price_yeet TYPE DOUBLE PRECISION USING ppv_price_yeet::DOUBLE PRECISION;
ALTER TABLE posts ALTER COLUMN tip_total_yeet TYPE DOUBLE PRECISION USING tip_total_yeet::DOUBLE PRECISION;
" 2>/dev/null || true

docker run -d --name yeet-backend \
  --network yeet-social_yeet-net \
  -p 8080:8080 \
  -e DATABASE_URL="postgres://yeet:YeetDB_5254a44ceae0a4a7!@yeet-postgres:5432/yeet" \
  -e REDIS_URL="redis://yeet-redis:6379" \
  -e JWT_SECRET="f270e9a02377765cf70ac4ccf1e35af55be8e7d3bac3c71e08e5e17eed62a6c2310d8a24b3d23e4d" \
  -e RUST_LOG="backend=info,tower_http=warn" \
  -e ADMIN_SECRET="yeet_admin_2024" \
  ghcr.io/zauni1984/yeet-social/backend:main
echo "Backend started on :8080"
sleep 5 && curl -s http://127.0.0.1:8080/api/v1/health
