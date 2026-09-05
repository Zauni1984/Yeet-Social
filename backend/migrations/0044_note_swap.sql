-- NOTE → YEET swap (docs/swap-note-to-yeet.md), backend preparation.
-- One-way deposit bridge: each KYC'd user with a linked BSC wallet gets a
-- personal NOTE deposit address from our own Note full node; a watcher
-- credits confirmed deposits as YEET payouts (100 NOTE = 1 YEET) through the
-- existing conversion pipeline (admin approval + pool guard apply).
-- Everything here is inert until SWAP_ENABLED=true and the Note RPC is set.

CREATE TABLE IF NOT EXISTS swap_addresses (
    user_id      UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    note_address TEXT NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS swap_deposits (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    note_address  TEXT NOT NULL,
    txid          TEXT NOT NULL,
    vout          INTEGER NOT NULL,
    amount_note   NUMERIC(24,8) NOT NULL,
    confirmations INTEGER NOT NULL DEFAULT 0,
    -- seen: detected, waiting for confirmations
    -- credited: YEET payout queued (payout_id set)
    -- failed: could not credit (see last_error), needs admin attention
    status        TEXT NOT NULL DEFAULT 'seen',
    payout_id     UUID REFERENCES token_rewards(id),
    last_error    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Idempotency: a chain output is credited at most once.
    UNIQUE (txid, vout)
);

CREATE INDEX IF NOT EXISTS idx_swap_deposits_user
    ON swap_deposits (user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_swap_deposits_seen
    ON swap_deposits (confirmations) WHERE status = 'seen';
