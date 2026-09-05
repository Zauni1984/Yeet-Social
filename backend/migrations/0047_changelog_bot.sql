-- Changelog bot: a system user that posts release notes as permanent posts.
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_bot BOOLEAN NOT NULL DEFAULT FALSE;

-- One row per published changelog entry so a redeploy never re-posts it.
CREATE TABLE IF NOT EXISTS changelog_posts (
    entry_id   TEXT PRIMARY KEY,
    post_id    UUID REFERENCES posts(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Bot accounts have neither wallet nor email (nobody can log in as them);
-- widen the identity rule accordingly.
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_must_have_identity;
ALTER TABLE users ADD CONSTRAINT users_must_have_identity
  CHECK (wallet_address IS NOT NULL OR email IS NOT NULL OR is_bot);
