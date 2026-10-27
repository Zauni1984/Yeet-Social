-- 0054: make the `tips` table match the points model that writes to it.
--
-- `send_tip_tx` (api/tips.rs) records tips, pay-per-view unlocks, DM tips and
-- live-stream tips as POINTS moves. On a database created from these
-- migrations the insert failed with a DB error (HTTP 500) for three reasons:
--
--   1. `currency` was the enum `tip_currency` ('yeet','bnb','fiat') while the
--      backend writes/reads the text 'YEET' (lives.rs sums `currency = 'YEET'`).
--   2. `post_id` was NOT NULL, but DM tips and live tips carry no post.
--   3. amounts were bound as text; the code now binds float8 (see tips.rs).
--
-- Idempotent: only converts the column when it is still the enum.
DO $$
BEGIN
  IF EXISTS (
      SELECT 1 FROM information_schema.columns
       WHERE table_schema = 'public' AND table_name = 'tips'
         AND column_name = 'currency' AND udt_name = 'tip_currency'
  ) THEN
    ALTER TABLE tips ALTER COLUMN currency TYPE TEXT USING UPPER(currency::text);
  END IF;
END $$;

UPDATE tips SET currency = UPPER(currency) WHERE currency <> UPPER(currency);
ALTER TABLE tips ALTER COLUMN currency SET DEFAULT 'YEET';
ALTER TABLE tips DROP CONSTRAINT IF EXISTS tips_currency_check;
-- NOT VALID: enforced for new rows; legacy rows (if any) are left untouched.
ALTER TABLE tips ADD CONSTRAINT tips_currency_check
  CHECK (currency IN ('YEET', 'BNB', 'FIAT')) NOT VALID;

ALTER TABLE tips ALTER COLUMN post_id DROP NOT NULL;

-- ─── ppv_unlocks: reconcile the two historical shapes ────────────────────
-- 0001 created `ppv_unlocks (amount_paid, tx_hash, created_at)`; 0025 then
-- used CREATE TABLE IF NOT EXISTS with the shape the code uses
-- (`price_paid`, `tip_id`, `unlocked_at`) — a no-op on databases built from
-- 0001, so POST /posts/:id/unlock failed there with a DB error. Bring every
-- database to the 0025 shape without losing rows.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.columns
              WHERE table_schema='public' AND table_name='ppv_unlocks' AND column_name='amount_paid')
     AND NOT EXISTS (SELECT 1 FROM information_schema.columns
              WHERE table_schema='public' AND table_name='ppv_unlocks' AND column_name='price_paid') THEN
    ALTER TABLE ppv_unlocks RENAME COLUMN amount_paid TO price_paid;
  END IF;
  IF EXISTS (SELECT 1 FROM information_schema.columns
              WHERE table_schema='public' AND table_name='ppv_unlocks' AND column_name='created_at')
     AND NOT EXISTS (SELECT 1 FROM information_schema.columns
              WHERE table_schema='public' AND table_name='ppv_unlocks' AND column_name='unlocked_at') THEN
    ALTER TABLE ppv_unlocks RENAME COLUMN created_at TO unlocked_at;
  END IF;
END $$;

ALTER TABLE ppv_unlocks
    ADD COLUMN IF NOT EXISTS price_paid  NUMERIC(20,8) NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS tip_id      UUID REFERENCES tips(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS unlocked_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE ppv_unlocks ALTER COLUMN price_paid DROP DEFAULT;
ALTER TABLE ppv_unlocks DROP CONSTRAINT IF EXISTS ppv_unlocks_price_paid_check;
ALTER TABLE ppv_unlocks ADD CONSTRAINT ppv_unlocks_price_paid_check CHECK (price_paid > 0) NOT VALID;
CREATE INDEX IF NOT EXISTS idx_ppv_unlocks_post ON ppv_unlocks(post_id);
