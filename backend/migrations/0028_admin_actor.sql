-- Migration: record WHO performed each admin action.
--
-- Until now the admin_actions audit log only knew WHAT happened — the
-- ADMIN_SECRET shared-secret model gave us no way to attribute it. The
-- caller is also logged in to the app as a regular user though, so we
-- can capture the JWT subject and store their id + username next to
-- the action. admin_username is a snapshot so the row survives if the
-- admin's account is later deleted.

ALTER TABLE admin_actions
    ADD COLUMN IF NOT EXISTS admin_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS admin_username TEXT;

CREATE INDEX IF NOT EXISTS idx_admin_actions_admin_user ON admin_actions(admin_user_id);
