-- Per-user feed filters: only show posts in these languages / from these
-- countries. Empty array = no filter. Country of a post = its author's
-- users.country_code (ISO 3166-1 alpha-2, set in Settings).
ALTER TABLE user_settings ADD COLUMN IF NOT EXISTS feed_langs     TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE user_settings ADD COLUMN IF NOT EXISTS feed_countries TEXT[] NOT NULL DEFAULT '{}';
CREATE INDEX IF NOT EXISTS idx_users_country ON users (country_code) WHERE country_code IS NOT NULL;
