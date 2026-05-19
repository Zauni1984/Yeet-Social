-- 0027_yeet_credit.sql
--
-- Formalises the "YEET Credit" architecture: a custodial off-chain
-- balance distinct from each user's on-chain YEET. Credit funds the
-- cheap micro-actions (paper-wallet vouchers, PPV unlocks, DM tips)
-- that would be uneconomic if every move paid BSC gas. Tips between
-- users still go on-chain; Credit enters via deposit, exits via
-- cashout.
--
-- Renames the existing column rather than adding a new one — the
-- platform is pre-launch, no real users yet, and a clean name makes
-- the rest of the code far easier to read.

-- 1. Rename the off-chain balance column.
ALTER TABLE users RENAME COLUMN yeet_token_balance TO yeet_credit_balance;

-- 2. Deposit ledger.
--
-- One row per confirmed on-chain Transfer into the platform's custodial
-- deposit address. `tx_hash UNIQUE` is the indexer's dedupe key — the
-- same Transfer log can't credit a user twice even if both the
-- frontend POST and the chain-watcher race.
CREATE TABLE IF NOT EXISTS credit_deposits (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tx_hash     TEXT NOT NULL UNIQUE,
    amount      NUMERIC(20, 8) NOT NULL CHECK (amount > 0),
    credited_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS credit_deposits_user_idx
    ON credit_deposits (user_id, credited_at DESC);

-- 3. Withdrawal queue.
--
-- A cashout request flows through the statuses:
--   pending   — user clicked Withdraw; credit already debited
--   submitted — broadcast tx, awaiting confirmation
--   confirmed — receipt landed, on-chain transfer settled
--   failed    — broadcast or confirmation failed; credit refunded
--
-- The hourly `services::credit_payout` job is the only writer past
-- `pending`. Refunds on failure happen inside the same job's
-- transaction so credit can't be lost.
CREATE TABLE IF NOT EXISTS credit_withdrawals (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    amount        NUMERIC(20, 8) NOT NULL CHECK (amount > 0),
    status        TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending', 'submitted', 'confirmed', 'failed')),
    tx_hash       TEXT,
    error_message TEXT,
    requested_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    settled_at    TIMESTAMPTZ
);

-- Pending-or-in-flight withdrawals are the worker's hot read; partial
-- index keeps it tiny even after thousands of confirmed rows.
CREATE INDEX IF NOT EXISTS credit_withdrawals_inflight_idx
    ON credit_withdrawals (requested_at)
    WHERE status IN ('pending', 'submitted');

CREATE INDEX IF NOT EXISTS credit_withdrawals_user_idx
    ON credit_withdrawals (user_id, requested_at DESC);
