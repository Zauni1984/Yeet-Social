-- ─── Enums ────────────────────────────────────────────────────────────────────

CREATE TYPE post_visibility AS ENUM (
    'public',
    'followers_only',
    'age_restricted',
    'pay_per_view'
);

CREATE TYPE tip_currency AS ENUM ('yeet', 'bnb', 'fiat');

CREATE TYPE reward_action AS ENUM (
    'daily_login',
    'comment',
    'share',
    'reshare',
    'downvote',
    'mint_nft',
    'referral_signup'
);

-- ─── Users ────────────────────────────────────────────────────────────────────

CREATE TABLE users (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username            TEXT UNIQUE NOT NULL,
    display_name        TEXT,
    bio                 TEXT,
    avatar_url          TEXT,
    wallet_address      TEXT UNIQUE,           -- BSC wallet
    country_code        CHAR(2),               -- ISO 3166-1 alpha-2
    is_verified         BOOLEAN DEFAULT FALSE,
    age_verified        BOOLEAN DEFAULT FALSE,
    yeet_token_balance  NUMERIC(20,8) DEFAULT 0,
    referral_code       TEXT UNIQUE,
    referred_by         UUID REFERENCES users(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_users_wallet ON users(wallet_address);
CREATE INDEX idx_users_username ON users(username);

-- ─── Follows ──────────────────────────────────────────────────────────────────

CREATE TABLE follows (
    follower_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    following_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (follower_id, following_id)
);

-- ─── Posts ────────────────────────────────────────────────────────────────────

CREATE TABLE posts (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id            UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content              TEXT NOT NULL,
    media_urls           TEXT[],
    visibility           post_visibility NOT NULL DEFAULT 'public',
    source_type          TEXT DEFAULT 'yeet',     -- 'yeet' | 'web_board'
    source_domain        TEXT,                    -- e.g. 'campingsite.de'
    pay_per_view_price   NUMERIC(20,8),           -- price in YEET tokens
    is_nft               BOOLEAN DEFAULT FALSE,
    nft_token_id         TEXT,
    nft_contract_address TEXT,
    like_count           BIGINT DEFAULT 0,
    comment_count        BIGINT DEFAULT 0,
    reshare_count        BIGINT DEFAULT 0,
    tip_total            NUMERIC(20,8) DEFAULT 0,
    reshared_from        UUID REFERENCES posts(id),
    -- Hybrid 24h timer: reset on reshare; cleared if post becomes NFT
    expires_at           TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '24 hours',
    deleted_at           TIMESTAMPTZ,            -- soft-delete
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_posts_author    ON posts(author_id);
CREATE INDEX idx_posts_expires   ON posts(expires_at) WHERE deleted_at IS NULL;
CREATE INDEX idx_posts_feed      ON posts(created_at DESC) WHERE deleted_at IS NULL;

-- ─── Comments ─────────────────────────────────────────────────────────────────

CREATE TABLE comments (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    post_id     UUID NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    author_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_comments_post ON comments(post_id, created_at);

-- ─── Likes ────────────────────────────────────────────────────────────────────

CREATE TABLE post_likes (
    post_id     UUID NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (post_id, user_id)
);

-- ─── Tips ─────────────────────────────────────────────────────────────────────

CREATE TABLE tips (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_user_id    UUID NOT NULL REFERENCES users(id),
    to_user_id      UUID NOT NULL REFERENCES users(id),
    post_id         UUID NOT NULL REFERENCES posts(id),
    amount          NUMERIC(20,8) NOT NULL,
    creator_amount  NUMERIC(20,8) NOT NULL,  -- after platform cut
    platform_cut    NUMERIC(20,8) NOT NULL,
    currency        tip_currency NOT NULL,
    tx_hash         TEXT,                    -- BSC tx hash for crypto
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_tips_user ON tips(to_user_id, created_at DESC);

-- ─── Token Rewards ────────────────────────────────────────────────────────────

CREATE TABLE token_rewards (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    action      reward_action NOT NULL,
    amount      NUMERIC(20,8) NOT NULL,
    tx_hash     TEXT,           -- BSC tx hash when batched on-chain
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_rewards_user ON token_rewards(user_id, created_at DESC);

-- ─── Pay-per-view unlocks ─────────────────────────────────────────────────────

CREATE TABLE ppv_unlocks (
    user_id     UUID NOT NULL REFERENCES users(id),
    post_id     UUID NOT NULL REFERENCES posts(id),
    amount_paid NUMERIC(20,8) NOT NULL,
    tx_hash     TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, post_id)
);

-- ─── Subscriptions ────────────────────────────────────────────────────────────

CREATE TABLE subscriptions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subscriber_id   UUID NOT NULL REFERENCES users(id),
    creator_id      UUID NOT NULL REFERENCES users(id),
    tier            TEXT NOT NULL DEFAULT 'monthly',
    valid_until     TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(subscriber_id, creator_id)
);

-- ─── Web Board Connections ────────────────────────────────────────────────────

CREATE TABLE webboard_connections (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    domain      TEXT NOT NULL,
    feed_url    TEXT NOT NULL,   -- RSS/Atom/API endpoint
    username    TEXT,            -- user's handle on that board
    is_active   BOOLEAN DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, domain)
);

-- ─── Auto-expire job function ─────────────────────────────────────────────────
-- Run via pg_cron or a Tokio background task every 5 minutes

CREATE OR REPLACE FUNCTION cleanup_expired_posts()
RETURNS INTEGER AS $$
DECLARE deleted_count INTEGER;
BEGIN
    -- Soft-delete expired non-NFT posts
    UPDATE posts
    SET deleted_at = NOW()
    WHERE expires_at < NOW()
      AND is_nft = FALSE
      AND deleted_at IS NULL;

    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$ LANGUAGE plpgsql;
