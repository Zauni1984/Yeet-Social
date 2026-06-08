-- Migration: messaging-system hardening sprint.
--
-- Bundles the schema half of a focused security + UX hardening pass
-- on the existing E2EE messaging layer. The crypto model (P-256 ECDH
-- identity keys + AES-GCM ciphertext, server-blind) is preserved end-
-- to-end — none of the new columns or tables ever see plaintext
-- except the explicit, opt-in moderation copy described below.
--
-- Sections in order:
--   1. messages: idempotency, edit/delete-for-everyone, audit cols
--   2. conversations: per-conversation self-destruct timer
--   3. conversation_members: mute + archive flags
--   4. message_deliveries + message_read_receipts (per-recipient state)
--   5. message_reports (opt-in plaintext for moderators only)
--   6. user_sessions (refresh-token rotation + reuse detection)
--   7. user preference toggles (read-receipt + typing privacy)


-- ─── 1. messages: idempotency + edit + delete-for-everyone ───────────────
ALTER TABLE messages
    -- Client-generated UUID. The unique partial index below makes the
    -- INSERT idempotent per (sender, conversation) so a network retry
    -- that double-fires the POST yields the original row back, not a
    -- duplicate ciphertext in the timeline.
    ADD COLUMN IF NOT EXISTS client_message_id UUID,
    -- Set when the sender edits the ciphertext (still E2EE; the server
    -- never sees plaintext). Surfaces as an "edited" badge in the UI.
    ADD COLUMN IF NOT EXISTS edited_at TIMESTAMPTZ,
    -- Distinct from the existing `deleted_at` tombstone, which means
    -- "deleted-for-me" (the sender removed it from their own view).
    -- `deleted_for_all_at` means the sender unsent the message for
    -- every participant; clients render a placeholder and drop the
    -- ciphertext locally.
    ADD COLUMN IF NOT EXISTS deleted_for_all_at TIMESTAMPTZ;

-- Idempotency: same client_message_id from the same sender into the
-- same conversation can only land once. NULL = legacy / not provided
-- (still allowed; only the explicit key gets the constraint).
CREATE UNIQUE INDEX IF NOT EXISTS uq_messages_idempotency
    ON messages(sender_id, conversation_id, client_message_id)
    WHERE client_message_id IS NOT NULL;

-- Hot path for the rate-limiter's "how many messages did this sender
-- send in conv X in the last N seconds" query.
CREATE INDEX IF NOT EXISTS idx_messages_sender_conv_time
    ON messages(sender_id, conversation_id, created_at DESC);


-- ─── 2. conversations: per-conversation self-destruct ────────────────────
ALTER TABLE conversations
    -- NULL = use the user's global retention preference (1/7/30 days)
    -- which is already enforced by the cleanup job. Anything > 0 hard-
    -- caps the lifetime of *new* messages in this conversation; the
    -- send handler computes expires_at = NOW() + LEAST(...).
    -- Bounded server-side to keep the 30-day hard wall intact.
    ADD COLUMN IF NOT EXISTS self_destruct_seconds INTEGER
        CHECK (self_destruct_seconds IS NULL
            OR (self_destruct_seconds BETWEEN 5 AND 30 * 24 * 3600));


-- ─── 3. conversation_members: mute + archive ─────────────────────────────
ALTER TABLE conversation_members
    -- NULL = not muted. Until the timestamp passes, the notify() call
    -- in send() skips this member; the conversation still appears in
    -- the list, just without a push/badge.
    ADD COLUMN IF NOT EXISTS muted_until TIMESTAMPTZ,
    -- Archive is independent of mute and hidden_at. Archived
    -- conversations are filtered out of list_mine() by default and
    -- only surface via ?archived=true.
    ADD COLUMN IF NOT EXISTS archived_at TIMESTAMPTZ;


-- ─── 4. delivery + read receipts ────────────────────────────────────────
-- Per-recipient state. Existence of a row in message_deliveries means
-- the client confirmed it pulled the message from the server.
-- Existence of a row in message_read_receipts means the client
-- confirmed the user actually viewed it.
--
-- Both are gated by the recipient's preference toggles (see § 7) so a
-- user who turned receipts off never writes a row, and queries from
-- senders never leak that they viewed.
CREATE TABLE IF NOT EXISTS message_deliveries (
    message_id   UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    delivered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (message_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_deliveries_user ON message_deliveries(user_id);

CREATE TABLE IF NOT EXISTS message_read_receipts (
    message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    read_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (message_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_read_receipts_user ON message_read_receipts(user_id);


-- ─── 5. message_reports ─────────────────────────────────────────────────
-- Because messages are E2EE, the server cannot decrypt a flagged
-- message. The reporting UX therefore asks the reporter to confirm a
-- one-time, opt-in disclosure: the client decrypts the offending
-- message locally and submits the plaintext (and original ciphertext
-- for audit) only for THIS report. The plaintext lives ONLY in this
-- row, never on the messages table, and is purged when the case is
-- resolved past `resolved_at + 90 days` by the cleanup job.
--
-- This is the only path through which a moderator can ever see DM
-- content, and the reporter (not the server) chooses what gets shared.
CREATE TABLE IF NOT EXISTS message_reports (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    message_id          UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    conversation_id     UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    reporter_id         UUID NOT NULL REFERENCES users(id) ON DELETE SET NULL,
    reported_user_id    UUID REFERENCES users(id) ON DELETE SET NULL,
    -- 'spam' | 'abuse' | 'sexual' | 'illegal' | 'other'
    category            TEXT NOT NULL,
    reason              TEXT,
    -- Reporter-supplied plaintext copy of the reported message. NULL
    -- means the reporter chose not to share (we still record the
    -- metadata for abuse pattern detection — see services/abuse.rs).
    disclosed_plaintext TEXT,
    -- 'pending' | 'dismissed' | 'actioned' | 'invalid'
    status              TEXT NOT NULL DEFAULT 'pending',
    resolution          TEXT,
    resolved_by         UUID REFERENCES users(id) ON DELETE SET NULL,
    resolved_at         TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- One report per (message, reporter) — spam-click protection.
    UNIQUE (message_id, reporter_id)
);
CREATE INDEX IF NOT EXISTS idx_message_reports_pending
    ON message_reports(created_at DESC)
    WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_message_reports_reported_user
    ON message_reports(reported_user_id, created_at DESC);


-- ─── 6. user_sessions: refresh-token families ───────────────────────────
-- Refresh tokens are now tracked server-side so we can:
--  * rotate on every refresh (each refresh issues a new JTI, the old
--    one is marked rotated-to-new-jti)
--  * detect reuse — a refresh whose row is already rotated or revoked
--    means the family was leaked; we then revoke the WHOLE family
--    (all descendant JTIs blacklisted in Redis)
--  * give the user a real device list to revoke individual sessions
--    without nuking every device they own.
CREATE TABLE IF NOT EXISTS user_sessions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- The current refresh-token JTI for this session row. Unique
    -- across the whole table so reuse detection is a single index hit.
    jti             TEXT NOT NULL UNIQUE,
    -- All rotations descending from the same login share family_id.
    family_id       UUID NOT NULL,
    parent_jti      TEXT,
    -- Set when this session was rotated; points to the new JTI that
    -- replaced it. A re-presentation of this row's jti is reuse.
    rotated_to_jti  TEXT,
    -- Set when the user (or reuse detection) revoked this session.
    revoked_at      TIMESTAMPTZ,
    revoked_reason  TEXT,
    -- Lightweight device fingerprint for the user's session list. We
    -- explicitly do NOT store full UA strings or IPs by default — only
    -- a short label the client can set on register and a coarse
    -- country code resolved at issuance time.
    device_label    TEXT,
    ip_country      TEXT,
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_user_sessions_user_active
    ON user_sessions(user_id)
    WHERE revoked_at IS NULL AND rotated_to_jti IS NULL;
CREATE INDEX IF NOT EXISTS idx_user_sessions_family
    ON user_sessions(family_id);


-- ─── 7. user preferences ────────────────────────────────────────────────
ALTER TABLE users
    -- When false, this user never writes message_read_receipts rows
    -- AND the server filters out other receipts when reading on their
    -- behalf so they can't peek at others either ("symmetric" model).
    ADD COLUMN IF NOT EXISTS read_receipts_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    -- Typing indicators are pure UX; toggled separately from receipts.
    ADD COLUMN IF NOT EXISTS typing_indicators_enabled BOOLEAN NOT NULL DEFAULT TRUE;
