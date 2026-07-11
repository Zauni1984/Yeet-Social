-- Points model + one-way conversion (docs/mica/05–08).
--
-- The internal `users.yeet_token_balance` is now POINTS (not a crypto-asset):
-- earned only, spent on in-app tips/PPV, and convertible ONE-WAY to on-chain
-- YEET paid to the user's verified EXTERNAL wallet. Engagement rewards no
-- longer auto-mint on-chain; only an explicit conversion mints.
--
-- NB: token_rewards (migration 0001) never actually had a `status` column and
-- its `action` was the `reward_action` ENUM. The old reward code referenced a
-- `status` column, but its errors were swallowed (`let _ = grant_reward(...)`),
-- so it silently no-op'd. This migration reconciles the schema with the code.

-- 1) Add the status column the code always assumed.
ALTER TABLE token_rewards
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'pending';

-- 2) Convert `action` from the reward_action enum to free text so payout rows
--    can use action='conversion' (not an enum label). Existing enum labels are
--    preserved as their text form.
ALTER TABLE token_rewards
    ALTER COLUMN action TYPE TEXT USING action::text;

-- 3) Distinguish audit rewards from payout requests in the mint queue.
ALTER TABLE token_rewards
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'reward'; -- 'reward' | 'conversion'

-- 4) Any already-minted historical rows (tx_hash set) are 'minted', not pending.
UPDATE token_rewards SET status = 'minted' WHERE tx_hash IS NOT NULL;

-- 5) Fold every still-unminted engagement reward into the points balance so no
--    earned value is lost when we stop auto-minting rewards.
UPDATE users u
   SET yeet_token_balance = COALESCE(u.yeet_token_balance, 0) + agg.pts
  FROM (
    SELECT user_id, SUM(amount)::float8 AS pts
      FROM token_rewards
     WHERE status = 'pending' AND tx_hash IS NULL AND kind = 'reward'
     GROUP BY user_id
  ) agg
 WHERE u.id = agg.user_id;

-- 6) Retire those rows so the batch minter never pays them out on-chain.
UPDATE token_rewards
   SET status = 'folded'
 WHERE status = 'pending' AND tx_hash IS NULL AND kind = 'reward';

-- Helpful index for the (now conversion-only) mint scan.
CREATE INDEX IF NOT EXISTS idx_token_rewards_mint
    ON token_rewards (kind, status)
    WHERE tx_hash IS NULL;
