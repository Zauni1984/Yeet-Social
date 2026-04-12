-- Permanent post visibility control + repost tracking
ALTER TABLE posts ADD COLUMN IF NOT EXISTS visibility TEXT NOT NULL DEFAULT 'public';
  -- 'public' = all profile visitors, 'followers' = followers only
ALTER TABLE posts ADD COLUMN IF NOT EXISTS repost_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE posts ADD COLUMN IF NOT EXISTS reposted_from UUID REFERENCES posts(id);
-- Index for profile timeline queries (permanent posts by user)
CREATE INDEX IF NOT EXISTS idx_posts_permanent ON posts (author_id, is_permanent) WHERE is_permanent = TRUE;
