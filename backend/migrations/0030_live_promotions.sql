-- Migration: paid promotions for live streams.
--
-- A host can buy a promotion when scheduling or starting a live. Two
-- tiers, both paid in YEET (debited at booking time, refunded if the
-- live is cancelled before it starts):
--
--   basic — 10 YEET — at start, an auto-post lands in the feed:
--                     "🔴 <host> is live now: <title>"
--   boost — 50 YEET — same auto-post + pinned to the top of the feed
--                     for 60 minutes after start. Pinning uses a new
--                     posts.pinned_until column so feed queries can
--                     prefer pinned > recency without a separate table.
--
-- Cost goes to the platform fee wallet (no creator split — this is an
-- ad fee, not a tip). The booking row carries the auto-post id so we
-- can clean up correctly if the live ends or is removed by moderation.

ALTER TABLE posts
    ADD COLUMN IF NOT EXISTS pinned_until TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS promoted_live_id UUID REFERENCES lives(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_posts_pinned
    ON posts(pinned_until)
    WHERE pinned_until IS NOT NULL AND pinned_until > '2000-01-01';

CREATE TABLE IF NOT EXISTS live_promotions (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    live_id                UUID NOT NULL REFERENCES lives(id) ON DELETE CASCADE,
    user_id                UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- 'basic' | 'boost'
    tier                   TEXT NOT NULL,
    cost_yeet              NUMERIC(20,8) NOT NULL,
    -- Only set for 'boost'. Drives posts.pinned_until when the promo
    -- is applied at live start.
    boost_minutes          INTEGER,
    -- Filled in when the live actually starts and the auto-post is
    -- created. NULL until then.
    auto_post_id           UUID REFERENCES posts(id) ON DELETE SET NULL,
    -- 'booked'   — paid, waiting for the live to start
    -- 'applied'  — auto-post created, charge final
    -- 'refunded' — live cancelled before start, YEET returned
    status                 TEXT NOT NULL DEFAULT 'booked',
    booked_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    applied_at             TIMESTAMPTZ,
    refunded_at            TIMESTAMPTZ
);

-- One active (booked|applied) promo per live, so the host can't
-- accidentally double-book and platform doesn't double-charge.
CREATE UNIQUE INDEX IF NOT EXISTS uq_live_promotions_active
    ON live_promotions(live_id)
    WHERE status IN ('booked','applied');

CREATE INDEX IF NOT EXISTS idx_live_promotions_user ON live_promotions(user_id);
