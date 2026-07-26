-- Transaction ledger — append-only, tamper-evident record of ALL value
-- movements (points + on-chain). Serves as evidence (Nachweis) and as the
-- source for tax (Finanzamt) exports.
--
-- Design for GoBD-style audit friendliness:
--   * append-only: an UPDATE/DELETE trigger blocks any mutation
--   * gapless entry_no: assigned under an advisory lock in the app layer
--   * hash chain: each row hashes (its canonical content || prev entry_hash),
--     so any retroactive change to any earlier row is detectable
--   * fiat valuation columns for tax reporting (value at time of the tx)

CREATE TABLE IF NOT EXISTS ledger_entries (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entry_no        BIGINT NOT NULL UNIQUE,          -- gapless sequence (1,2,3,…)
    occurred_at     TIMESTAMPTZ NOT NULL,            -- when the economic event happened
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    tx_type         TEXT NOT NULL,                   -- see services/ledger.rs TxType
    asset           TEXT NOT NULL,                   -- 'POINTS' | 'YEET' | 'BNB' | 'EUR'
    -- Signed from the subject's perspective: credit (+) / debit (−).
    amount          NUMERIC(38,18) NOT NULL,
    fee_amount      NUMERIC(38,18) NOT NULL DEFAULT 0,

    user_id         UUID REFERENCES users(id) ON DELETE SET NULL,       -- subject
    counterparty_id UUID REFERENCES users(id) ON DELETE SET NULL,       -- other side
    user_wallet     TEXT,                            -- subject wallet (payouts/on-chain)
    counterparty_wallet TEXT,

    reference_type  TEXT,                            -- 'post' | 'tip' | 'payout' | 'paper_wallet' | 'onchain'
    reference_id    TEXT,                            -- uuid/serial/etc of the referenced object
    onchain_tx_hash TEXT,                            -- BSC tx hash when on-chain

    -- Tax valuation (value of the movement at occurred_at). Nullable until a
    -- market price exists; structurally present for Finanzamt exports.
    fiat_currency   TEXT NOT NULL DEFAULT 'EUR',
    fiat_value      NUMERIC(38,18),
    fx_rate         NUMERIC(38,18),
    fx_source       TEXT,

    description     TEXT,
    created_by      TEXT NOT NULL DEFAULT 'system',  -- 'system' | 'admin:<id>'

    -- Tamper-evidence chain.
    prev_hash       TEXT NOT NULL,                   -- previous row's entry_hash ('GENESIS' for #1)
    entry_hash      TEXT NOT NULL                    -- sha256 hex of canonical content || prev_hash
);

CREATE INDEX IF NOT EXISTS idx_ledger_occurred   ON ledger_entries (occurred_at);
CREATE INDEX IF NOT EXISTS idx_ledger_type       ON ledger_entries (tx_type);
CREATE INDEX IF NOT EXISTS idx_ledger_user       ON ledger_entries (user_id);
CREATE INDEX IF NOT EXISTS idx_ledger_asset      ON ledger_entries (asset);
CREATE INDEX IF NOT EXISTS idx_ledger_txhash     ON ledger_entries (onchain_tx_hash);

-- Append-only enforcement: block UPDATE and DELETE at the DB level so the
-- ledger cannot be altered even by a bug or a direct query. Corrections are
-- made by appending a reversing entry, never by editing history.
CREATE OR REPLACE FUNCTION ledger_block_mutation()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'ledger_entries is append-only (% blocked)', TG_OP;
END;
$$;

DROP TRIGGER IF EXISTS trg_ledger_no_update ON ledger_entries;
CREATE TRIGGER trg_ledger_no_update
    BEFORE UPDATE ON ledger_entries
    FOR EACH ROW EXECUTE FUNCTION ledger_block_mutation();

DROP TRIGGER IF EXISTS trg_ledger_no_delete ON ledger_entries;
CREATE TRIGGER trg_ledger_no_delete
    BEFORE DELETE ON ledger_entries
    FOR EACH ROW EXECUTE FUNCTION ledger_block_mutation();
