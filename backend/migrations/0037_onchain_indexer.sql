-- On-chain event indexer state + idempotent ingestion.
--
-- The non-custodial model (docs/mica/05–07) moves tips/PPV/paper-wallets
-- on-chain. The backend stops booking these off-chain and instead INDEXES
-- contract events. These tables make ingestion crash-safe and exactly-once.

-- Per-contract scan cursor: how far we've confirmed-processed.
CREATE TABLE IF NOT EXISTS indexer_cursors (
    contract        TEXT PRIMARY KEY,          -- 'payments' | 'paper_escrow'
    chain_id        BIGINT NOT NULL,
    last_block      BIGINT NOT NULL DEFAULT 0, -- last fully-processed block
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Dedup ledger: one row per processed log. (tx_hash, log_index) is globally
-- unique on a chain, so re-scans / reorg replays can't double-apply.
CREATE TABLE IF NOT EXISTS onchain_events (
    tx_hash         TEXT NOT NULL,
    log_index       INTEGER NOT NULL,
    block_number    BIGINT NOT NULL,
    contract        TEXT NOT NULL,
    event           TEXT NOT NULL,             -- 'Paid' | 'VoucherClaimed' | ...
    payload         JSONB NOT NULL,            -- decoded args (for audit/debug)
    processed_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tx_hash, log_index)
);
CREATE INDEX IF NOT EXISTS idx_onchain_events_block ON onchain_events(block_number);

-- On-chain PPV unlocks land in the existing ppv_unlocks table; add a source
-- + tx_hash so on-chain and any legacy off-chain unlocks are distinguishable
-- and an on-chain unlock is idempotent per (post, user).
ALTER TABLE ppv_unlocks
    ADD COLUMN IF NOT EXISTS source  TEXT NOT NULL DEFAULT 'offchain', -- 'onchain' | 'offchain'
    ADD COLUMN IF NOT EXISTS tx_hash TEXT;

-- Optional: on-chain tip mirror for analytics (the chain is source of truth).
CREATE TABLE IF NOT EXISTS onchain_tips (
    tx_hash         TEXT NOT NULL,
    log_index       INTEGER NOT NULL,
    payer_address   TEXT NOT NULL,
    recipient_address TEXT NOT NULL,
    ref             TEXT,                       -- post/content id (bytes32 hex)
    gross           NUMERIC(78,0) NOT NULL,     -- wei
    fee             NUMERIC(78,0) NOT NULL,
    net             NUMERIC(78,0) NOT NULL,
    block_number    BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tx_hash, log_index)
);
CREATE INDEX IF NOT EXISTS idx_onchain_tips_recipient ON onchain_tips(recipient_address);
