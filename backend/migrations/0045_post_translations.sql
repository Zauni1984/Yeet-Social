-- Post language + cached translations.
-- posts.lang: ISO 639-1 code detected at post time ('und' = undetermined,
-- so the sweep does not retry forever). NULL = not yet checked.
ALTER TABLE posts ADD COLUMN IF NOT EXISTS lang VARCHAR(8);
CREATE INDEX IF NOT EXISTS idx_posts_lang_null ON posts (created_at DESC) WHERE lang IS NULL;

CREATE TABLE IF NOT EXISTS post_translations (
    post_id     UUID        NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    target_lang VARCHAR(8)  NOT NULL,
    source_lang VARCHAR(8),
    text        TEXT        NOT NULL,
    provider    TEXT        NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (post_id, target_lang)
);
