-- Migration: real 18+ verification with manual admin review.
--
-- Replaces the self-declared age_verified_at flag from migration 0021
-- (which was set client-side via a self-confirmed face scan) with a
-- queue + admin-approval workflow:
--
--   1. User records a face scan and optionally uploads an ID document
--      (passport / driver's license / national ID).
--   2. Server stores the encrypted-at-rest blobs to a PRIVATE on-disk
--      path that nginx does NOT proxy and ServeDir does NOT mount,
--      and inserts a row in age_verification_cases (status=pending).
--   3. An admin reviews the queue, sees the face + ID side by side,
--      approves or rejects. On approval the user's
--      users.age_verified_at is set and the badge becomes visible.
--   4. The blobs are scrubbed shortly after the decision (7 days for
--      approved, 30 for rejected as an appeal window) by the cleanup
--      job. Once purged, blobs_purged_at is set and the case keeps
--      only metadata for the audit trail.
--
-- Privacy posture:
--   * Biometric face data and government IDs live ONLY in the private
--     path + this table, accessible exclusively via authenticated
--     admin endpoints (shared-secret + JWT, both audit-logged).
--   * The new users.age_badge_hidden lets a verified user hide the
--     purple badge without revoking the verification itself, so a
--     toggle in Settings is reversible without re-submitting.
--   * Withdrawing a pending case wipes the blobs immediately.
--
-- Carry-over from 0021: existing rows in users with age_verified_at
-- set were granted via the old self-declaration. We do NOT auto-
-- invalidate them (would lock those users out of 18+ content with no
-- recourse) but the new flow is the only way to get verified going
-- forward.

CREATE TABLE IF NOT EXISTS age_verification_cases (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- 'pending' | 'approved' | 'rejected' | 'withdrawn'
    status              TEXT NOT NULL DEFAULT 'pending',

    -- Relative paths under PRIVATE_DIR (env, default /app/private).
    -- NEVER under uploads_dir — that one is served by ServeDir and
    -- nginx proxies it. NULL once the blob has been purged.
    face_scan_path      TEXT,
    id_document_path    TEXT,
    -- 'passport' | 'driver_license' | 'national_id' | 'other'
    id_document_type    TEXT,

    submitted_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reviewed_at         TIMESTAMPTZ,
    reviewed_by         UUID REFERENCES users(id) ON DELETE SET NULL,
    decision_note       TEXT,
    -- Set by the scrub job once the on-disk blobs are gone. Acts as a
    -- safety check that the case can no longer leak PII.
    blobs_purged_at     TIMESTAMPTZ
);

-- Only ONE active (pending) case per user at a time — re-submitting
-- while one is pending should withdraw the old one first.
CREATE UNIQUE INDEX IF NOT EXISTS uq_age_verification_pending
    ON age_verification_cases(user_id)
    WHERE status = 'pending';

-- Admin queue hot path.
CREATE INDEX IF NOT EXISTS idx_age_verification_pending_queue
    ON age_verification_cases(submitted_at)
    WHERE status = 'pending';

-- Cleanup job scans rows past their grace window.
CREATE INDEX IF NOT EXISTS idx_age_verification_purge_due
    ON age_verification_cases(reviewed_at)
    WHERE blobs_purged_at IS NULL AND reviewed_at IS NOT NULL;

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS age_badge_hidden BOOLEAN NOT NULL DEFAULT FALSE;
