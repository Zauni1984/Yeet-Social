-- Migration: Pay-per-view unlocks
--
-- Records which user has paid to unlock which post. A row in this
-- table is the source of truth for "is this post unlocked for this
-- viewer"; the previous frontend-only localStorage scheme let the
-- author re-charge users who cleared their browser state.
--
-- The actual fee accounting (10% platform cut / 90% to creator)
-- lives in the existing `tips` table; `tip_id` links a ppv_unlocks
-- row to the corresponding tip so the ledger stays unified.

CREATE TABLE IF NOT EXISTS ppv_unlocks (
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    post_id      UUID NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    price_paid   NUMERIC(20,8) NOT NULL CHECK (price_paid > 0),
    tip_id       UUID REFERENCES tips(id) ON DELETE SET NULL,
    unlocked_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, post_id)
);

CREATE INDEX IF NOT EXISTS idx_ppv_unlocks_post ON ppv_unlocks(post_id);
