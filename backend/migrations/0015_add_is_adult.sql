-- Add is_adult column to posts table
ALTER TABLE posts ADD COLUMN IF NOT EXISTS is_adult BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX IF NOT EXISTS idx_posts_is_adult ON posts(is_adult);
