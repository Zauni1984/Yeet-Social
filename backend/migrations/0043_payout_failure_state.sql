-- Payout failure path. Until now a conversion whose on-chain mint failed
-- simply stayed status='pending' and was retried by the hourly batch job
-- forever, with no visibility and no way to close it out. Track attempts so
-- the minter can give up after N failures and park the row as
-- status='failed' (points stay debited until an admin rejects it, which
-- refunds them via the existing reject flow).
ALTER TABLE token_rewards
    ADD COLUMN IF NOT EXISTS mint_attempts INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS last_error    TEXT;

CREATE INDEX IF NOT EXISTS idx_token_rewards_failed
    ON token_rewards (created_at)
    WHERE kind = 'conversion' AND status = 'failed';
