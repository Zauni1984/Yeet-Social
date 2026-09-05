-- Post kind: 'text' (default, incl. image/video posts) or 'audio' (Audio Story).
ALTER TABLE posts ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'text';
CREATE INDEX IF NOT EXISTS idx_posts_kind_audio ON posts (created_at DESC) WHERE kind = 'audio';
