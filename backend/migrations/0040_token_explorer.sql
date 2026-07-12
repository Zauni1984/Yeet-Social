-- On-chain YEET token explorer: holders (richlist), transfers, and stats.
-- Populated by the token indexer (scans ERC-20 Transfer events); the public
-- read API serves from these tables. Empty until the token is deployed and
-- the indexer is enabled (YEET_TOKEN_ADDRESS set + indexer spawned).

-- Current balance per address — the richlist source of truth.
CREATE TABLE IF NOT EXISTS token_holders (
    address       TEXT PRIMARY KEY,             -- 0x… lowercase
    balance       NUMERIC(78,0) NOT NULL DEFAULT 0,  -- wei
    tx_count      BIGINT NOT NULL DEFAULT 0,
    first_seen    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_active   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
-- Richlist ordering (largest balances first); partial to skip zero balances.
CREATE INDEX IF NOT EXISTS idx_token_holders_balance
    ON token_holders (balance DESC) WHERE balance > 0;

-- Full transfer history (one row per Transfer log).
CREATE TABLE IF NOT EXISTS token_transfers (
    tx_hash       TEXT NOT NULL,
    log_index     INTEGER NOT NULL,
    block_number  BIGINT NOT NULL,
    block_time    TIMESTAMPTZ,
    from_address  TEXT NOT NULL,
    to_address    TEXT NOT NULL,
    value         NUMERIC(78,0) NOT NULL,        -- wei
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tx_hash, log_index)
);
CREATE INDEX IF NOT EXISTS idx_token_transfers_from  ON token_transfers (from_address, block_number DESC);
CREATE INDEX IF NOT EXISTS idx_token_transfers_to    ON token_transfers (to_address, block_number DESC);
CREATE INDEX IF NOT EXISTS idx_token_transfers_block ON token_transfers (block_number DESC);

-- Cheap singleton stats cache (holder/transfer counts, circulating supply).
CREATE TABLE IF NOT EXISTS token_stats (
    id                SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    holder_count      BIGINT NOT NULL DEFAULT 0,
    transfer_count    BIGINT NOT NULL DEFAULT 0,
    circulating       NUMERIC(78,0) NOT NULL DEFAULT 0,  -- sum of positive balances
    last_indexed_block BIGINT NOT NULL DEFAULT 0,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
INSERT INTO token_stats (id) VALUES (1) ON CONFLICT (id) DO NOTHING;
