-- Migration: live streams + scheduled posts.
--
-- Adds three things in one go because the front-end "Live" tab and the
-- "Schedule" composer ship together.
--
-- 1. `lives` — one row per scheduled or live broadcast. Status moves
--    scheduled → live → ended. tip_total_yeet drives feed ranking so a
--    YEET tip pushes a host up the list. livekit_room is reserved for
--    Phase 2 when we wire the real WebRTC ingest; in Phase 1 it stays
--    NULL and the host page shows a placeholder.
--
-- 2. `scheduled_posts` — staging area for posts the user wants to
--    publish later. A worker job (see services/batch_rewards.rs)
--    moves a row into `posts` when publish_at is due. We don't put
--    `publish_at` directly on `posts` because every existing feed
--    query already filters `expires_at > NOW()` and would otherwise
--    leak hidden rows; keeping pending posts in a separate table is a
--    far smaller blast radius.
--
-- 3. `tips.live_id` — lets `send_tip_tx` attribute a tip to a live
--    broadcast in addition to the existing `post_id` attribution. The
--    backend lives endpoint reads SUM(amount) from this for the
--    ranking score so we never need a separate counter to keep in sync.

-- ─── 1. lives ─────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS lives (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    host_user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    description     TEXT,
    -- 'scheduled' | 'live' | 'ended' | 'cancelled'
    status          TEXT NOT NULL DEFAULT 'scheduled',
    scheduled_for   TIMESTAMPTZ,        -- NULL = "go live now"
    started_at      TIMESTAMPTZ,
    ended_at        TIMESTAMPTZ,
    viewer_count    INTEGER NOT NULL DEFAULT 0,
    is_adult        BOOLEAN NOT NULL DEFAULT FALSE,
    -- LiveKit room name. Generated at start; NULL until Phase 2 wires
    -- the real ingest path.
    livekit_room    TEXT,
    cover_url       TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_lives_host          ON lives(host_user_id);
CREATE INDEX IF NOT EXISTS idx_lives_active        ON lives(status) WHERE status = 'live';
CREATE INDEX IF NOT EXISTS idx_lives_scheduled_for ON lives(scheduled_for) WHERE status = 'scheduled';

-- ─── 2. scheduled_posts ───────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS scheduled_posts (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content         TEXT NOT NULL,
    media_url       TEXT,
    is_adult        BOOLEAN NOT NULL DEFAULT FALSE,
    is_nft          BOOLEAN NOT NULL DEFAULT FALSE,
    nft_price_yeet  NUMERIC(20,8),
    is_permanent    BOOLEAN NOT NULL DEFAULT FALSE,
    ppv_price_yeet  NUMERIC(20,8),
    publish_at      TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_scheduled_posts_due    ON scheduled_posts(publish_at);
CREATE INDEX IF NOT EXISTS idx_scheduled_posts_author ON scheduled_posts(author_id);

-- ─── 3. tips.live_id ──────────────────────────────────────────────────────
ALTER TABLE tips
    ADD COLUMN IF NOT EXISTS live_id UUID REFERENCES lives(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_tips_live ON tips(live_id) WHERE live_id IS NOT NULL;
