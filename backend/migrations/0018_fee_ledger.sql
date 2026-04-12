-- Fee ledger: tracks all platform fees collected
CREATE TABLE IF NOT EXISTS fee_ledger (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_type TEXT NOT NULL, -- 'tip', 'ppv', 'permanent_post'
    source_id UUID,            -- tip_id or post_id
    gross_amount DOUBLE PRECISION NOT NULL,
    fee_amount DOUBLE PRECISION NOT NULL,
    creator_amount DOUBLE PRECISION NOT NULL,
    fee_wallet TEXT NOT NULL DEFAULT '0xFEE_DUMMY_TESTNET_YEET_PLATFORM_WALLET_001',
    status TEXT NOT NULL DEFAULT 'pending', -- 'pending', 'transferred'
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Total fees accumulated (for $250 threshold check)
CREATE TABLE IF NOT EXISTS fee_wallet_balance (
    id INTEGER PRIMARY KEY DEFAULT 1,
    total_yeet DOUBLE PRECISION NOT NULL DEFAULT 0,
    last_transfer_at TIMESTAMPTZ,
    cold_wallet TEXT NOT NULL DEFAULT '0xCOLD_DUMMY_TESTNET_YEET_COLD_WALLET_001'
);

INSERT INTO fee_wallet_balance (id, total_yeet) VALUES (1, 0)
    ON CONFLICT (id) DO NOTHING;
