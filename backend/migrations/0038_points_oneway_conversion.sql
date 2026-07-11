-- Points model + one-way conversion (docs/mica/05–08).
--
-- The internal `users.yeet_token_balance` is now POINTS (not a crypto-asset):
-- earned only, spent on in-app tips/PPV, and convertible ONE-WAY to on-chain
-- YEET paid to the user's verified EXTERNAL wallet. Engagement rewards no
-- longer auto-mint on-chain; only an explicit conversion mints.

-- Distinguish audit rewards from payout requests in the mint queue.
ALTER TABLE token_rewards
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'reward'; -- 'reward' | 'conversion'

-- 1) Fold every still-unminted engagement reward into the points balance so
--    no earned value is lost when we stop auto-minting rewards.
UPDATE users u
   SET yeet_token_balance = COALESCE(u.yeet_token_balance, 0) + agg.pts
  FROM (
    SELECT user_id, SUM(amount)::float8 AS pts
      FROM token_rewards
     WHERE status = 'pending' AND tx_hash IS NULL AND kind = 'reward'
     GROUP BY user_id
  ) agg
 WHERE u.id = agg.user_id;

-- 2) Retire those rows so the batch minter never pays them out on-chain.
UPDATE token_rewards
   SET status = 'folded'
 WHERE status = 'pending' AND tx_hash IS NULL AND kind = 'reward';

-- Helpful index for the (now conversion-only) mint scan.
CREATE INDEX IF NOT EXISTS idx_token_rewards_mint
    ON token_rewards (kind, status)
    WHERE tx_hash IS NULL;
