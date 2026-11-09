-- 0055: developer API keys (autoposters, AI agents) — docs/api/README.md
--
-- A user applies for a key in the app; the key is inactive ("pending") until
-- an admin approves it. Only the SHA-256 of the secret is stored; the secret
-- is shown once at creation. Scopes gate which endpoints a key may call
-- (api/api_keys.rs::route_allowed); per-key rate limits are enforced in Redis.
CREATE TABLE IF NOT EXISTS api_keys (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    purpose         TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('autoposter', 'ai', 'other')),
    scopes          TEXT[] NOT NULL DEFAULT '{read}',
    key_prefix      TEXT NOT NULL,
    key_hash        TEXT NOT NULL UNIQUE,
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'approved', 'rejected', 'revoked')),
    rate_per_minute INT  NOT NULL DEFAULT 60,
    rate_per_day    INT  NOT NULL DEFAULT 2000,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    decided_at      TIMESTAMPTZ,
    decided_by      TEXT,
    decision_note   TEXT,
    last_used_at    TIMESTAMPTZ,
    use_count       BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_api_keys_user   ON api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_status ON api_keys(status);

-- Posts created through a key carry the key and its kind so the UI can label
-- automated / AI-generated content (transparency, AI Act Art. 50).
ALTER TABLE posts
    ADD COLUMN IF NOT EXISTS api_key_id      UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS automation_kind TEXT;

-- Schema repair for databases built from this repo's migrations: the feed
-- query reads posts.tip_total_yeet, which only exists on the production
-- database (migrations 0002–0011 are missing from the repo). Idempotent.
ALTER TABLE posts ADD COLUMN IF NOT EXISTS tip_total_yeet DOUBLE PRECISION NOT NULL DEFAULT 0;
