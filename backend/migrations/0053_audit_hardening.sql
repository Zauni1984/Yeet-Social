-- Audit 2026-10-03 (docs/audit-2026-10-03.md) — schema side of the fixes.

-- A1/M-C1: a NaN tip amount could poison balances (NaN compares false to
-- everything, so every "balance < amount" guard passed). Repair any NaN row
-- and forbid it at the DB level (NaN <> NaN is the standard NaN test).
UPDATE users SET yeet_token_balance = 0 WHERE yeet_token_balance <> yeet_token_balance;
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_points_is_number;
ALTER TABLE users ADD CONSTRAINT users_points_is_number
    CHECK (yeet_token_balance IS NULL OR yeet_token_balance = yeet_token_balance);

-- M-C2/H1: batch minter keeps an in-flight state so a lost receipt can be
-- reconciled instead of re-minted, and pays to the wallet recorded at
-- conversion time (not whatever is linked at mint time).
ALTER TABLE token_rewards
    ADD COLUMN IF NOT EXISTS wallet_address     TEXT,
    ADD COLUMN IF NOT EXISTS pending_tx_hash    TEXT,
    ADD COLUMN IF NOT EXISTS minting_started_at TIMESTAMPTZ;

-- C-M1: one reshare per user and post (was: unbounded, any id).
CREATE TABLE IF NOT EXISTS post_reshares (
    post_id    UUID NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (post_id, user_id)
);
