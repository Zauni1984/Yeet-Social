-- Report system for posts
CREATE TABLE IF NOT EXISTS post_reports (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    post_id     UUID NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    reporter_id UUID REFERENCES users(id) ON DELETE SET NULL,
    reason      VARCHAR(100) NOT NULL DEFAULT 'inappropriate',
    details     TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Flag posts as reported/removed by admin
ALTER TABLE posts
    ADD COLUMN IF NOT EXISTS report_count   INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS is_flagged     BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS is_removed     BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS removed_at     TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS removed_reason TEXT;

-- Index for fast admin queries
CREATE INDEX IF NOT EXISTS idx_post_reports_post_id ON post_reports(post_id);
CREATE INDEX IF NOT EXISTS idx_posts_flagged ON posts(is_flagged) WHERE is_flagged = TRUE;
CREATE INDEX IF NOT EXISTS idx_posts_removed ON posts(is_removed) WHERE is_removed = TRUE;

-- Auto-increment report_count trigger
CREATE OR REPLACE FUNCTION trg_increment_report_count()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    UPDATE posts SET
        report_count = report_count + 1,
        is_flagged = TRUE
    WHERE id = NEW.post_id;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_report_count ON post_reports;
CREATE TRIGGER trg_report_count
    AFTER INSERT ON post_reports
    FOR EACH ROW EXECUTE FUNCTION trg_increment_report_count();
