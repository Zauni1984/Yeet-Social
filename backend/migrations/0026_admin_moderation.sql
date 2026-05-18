-- Migration: Admin moderation tools
--
-- Adds posting-ban support on users + an admin_actions audit log.
-- Authentication for moderation endpoints continues to use the
-- existing shared ADMIN_SECRET pattern (see backend/src/api/report.rs);
-- this migration only adds the data shape that admin endpoints write
-- to and that the rest of the app reads from.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS posting_banned_until TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS post_ban_reason      TEXT;

CREATE INDEX IF NOT EXISTS idx_users_post_ban_active
    ON users(posting_banned_until)
 WHERE posting_banned_until IS NOT NULL;

-- Audit trail. action_type is one of:
--   ban_post   - posting ban applied; duration_hours required
--   unban_post - posting ban lifted (early or natural expiry); duration_hours NULL
--   delete_user - account hard-deleted; duration_hours NULL
-- target_user_id intentionally allows NULL via ON DELETE SET NULL so
-- the audit row survives even after the user has been purged.
CREATE TABLE IF NOT EXISTS admin_actions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_user_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    target_username TEXT,                -- snapshot, survives the user deletion
    action_type     TEXT NOT NULL,
    duration_hours  INT,
    reason          TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_admin_actions_target  ON admin_actions(target_user_id);
CREATE INDEX IF NOT EXISTS idx_admin_actions_created ON admin_actions(created_at DESC);
