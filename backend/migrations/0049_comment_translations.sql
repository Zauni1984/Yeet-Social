-- Cached translations of comments (same shape as post_translations).
CREATE TABLE IF NOT EXISTS comment_translations (
    comment_id  UUID        NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    target_lang VARCHAR(8)  NOT NULL,
    source_lang VARCHAR(8),
    text        TEXT        NOT NULL,
    provider    TEXT        NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (comment_id, target_lang)
);
