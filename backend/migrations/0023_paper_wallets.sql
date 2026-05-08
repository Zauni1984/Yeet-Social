-- Migration: Paper Wallets ("printable" YEET banknotes)
-- A paper wallet is a custodial claim ticket: at issuance the issuer's
-- yeet_token_balance is debited and the amount is locked against a hashed
-- secret. Whoever later submits the secret receives the credit. Active
-- (un-claimed) bills can be voided by the issuer to refund the balance.

CREATE TABLE IF NOT EXISTS paper_wallets (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    issuer_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    amount             NUMERIC(20,8) NOT NULL CHECK (amount > 0),
    currency           TEXT NOT NULL DEFAULT 'YEET',
    serial             TEXT NOT NULL UNIQUE,             -- short human-readable id, printed on the bill
    claim_secret_hash  BYTEA NOT NULL UNIQUE,            -- sha256(secret); secret is only shown once at creation
    status             TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','claimed','voided')),
    claimed_by_id      UUID REFERENCES users(id) ON DELETE SET NULL,
    claimed_at         TIMESTAMPTZ,
    voided_at          TIMESTAMPTZ,
    note               TEXT,                              -- optional issuer note (e.g. "Tip for John")
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_paper_wallets_issuer  ON paper_wallets(issuer_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_paper_wallets_status  ON paper_wallets(status);
