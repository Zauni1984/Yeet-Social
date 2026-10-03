-- Versioned points→YEET conversion rate (docs/mica/09 D4, Leitplanke L7).
--
-- The rate used to be a code constant (1 point = 1 YEET). The Terms (§6)
-- promise that a change applies to FUTURE conversions only and is announced
-- in advance, so the rate becomes a dated table: the row with the latest
-- valid_from <= now() is the current rate; rows with valid_from in the
-- future are announced changes. The backend refuses to schedule a rate
-- that would take effect sooner than YEET_RATE_NOTICE_DAYS (default 14).
CREATE TABLE IF NOT EXISTS conversion_rates (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rate          NUMERIC(20,8) NOT NULL CHECK (rate > 0),   -- YEET per point
    valid_from    TIMESTAMPTZ NOT NULL UNIQUE,
    announced_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    note          TEXT,
    created_by    TEXT NOT NULL DEFAULT 'system'
);

INSERT INTO conversion_rates (rate, valid_from, announced_at, note)
SELECT 1, '1970-01-01', '1970-01-01', 'initial rate: 1 point = 1 YEET (docs/mica/02 Teil J)'
 WHERE NOT EXISTS (SELECT 1 FROM conversion_rates);

-- A conversion row now remembers how many POINTS were debited and at which
-- rate; `amount` stays the YEET to mint. Refunds return points_debited.
ALTER TABLE token_rewards
    ADD COLUMN IF NOT EXISTS points_debited NUMERIC(20,8),
    ADD COLUMN IF NOT EXISTS rate           NUMERIC(20,8);

UPDATE token_rewards
   SET points_debited = amount, rate = 1
 WHERE kind = 'conversion' AND points_debited IS NULL;
