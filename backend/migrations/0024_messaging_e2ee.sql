-- Migration: Messaging (E2EE) + Blocks + Retention
--
-- Adds the schema for end-to-end-encrypted direct messages and group chats,
-- per-user retention preference (1/7/30 days), user blocks, and the group
-- invitation acceptance flow.
--
-- Server stores only ciphertext: a 30-day hard-cap is enforced via the
-- messages.expires_at default + a cleanup job; per-user retention shorter
-- than that is filtered client-side.

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS dm_retention_days SMALLINT NOT NULL DEFAULT 7
    CHECK (dm_retention_days IN (1, 7, 30)),
  ADD COLUMN IF NOT EXISTS e2ee_public_key TEXT,
  ADD COLUMN IF NOT EXISTS e2ee_encrypted_private_key TEXT;

-- ---------------------------------------------------------------------
-- Blocks
-- ---------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS user_blocks (
  blocker_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  blocked_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (blocker_id, blocked_id),
  CHECK (blocker_id <> blocked_id)
);
CREATE INDEX IF NOT EXISTS idx_user_blocks_blocked ON user_blocks(blocked_id);

-- ---------------------------------------------------------------------
-- Conversations + members
-- ---------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS conversations (
  id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  kind         TEXT NOT NULL CHECK (kind IN ('dm', 'group')),
  name         TEXT,
  created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
  -- For DMs: 'least(uuid_a, uuid_b)::text || ":" || greatest(...)'.
  -- The UNIQUE index below ensures only one DM conversation per pair.
  dm_pair_key  TEXT,
  created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_conv_dm_pair
  ON conversations(dm_pair_key) WHERE kind = 'dm';

CREATE TABLE IF NOT EXISTS conversation_members (
  conversation_id      UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  user_id              UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role                 TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('member', 'admin')),
  -- Per-member envelope: AES-GCM(member_dh_key, group_key) for groups; NULL for DMs.
  -- Set to NULL by the kick handler to force admin-driven rotation.
  encrypted_group_key  TEXT,
  -- Set when the conversation is hidden for this user (e.g. mutual block).
  hidden_at            TIMESTAMPTZ,
  joined_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (conversation_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_conv_members_user ON conversation_members(user_id);

-- ---------------------------------------------------------------------
-- Messages
-- ---------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS messages (
  id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  conversation_id  UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  sender_id        UUID REFERENCES users(id) ON DELETE SET NULL,
  kind             TEXT NOT NULL CHECK (kind IN ('text', 'image', 'tip')),
  -- AES-GCM ciphertext of the JSON-encoded payload, base64-encoded.
  ciphertext       TEXT NOT NULL,
  -- 12-byte AES-GCM IV, base64-encoded.
  iv               TEXT NOT NULL,
  -- For kind='tip': FK into tips ledger so fee accounting stays unified.
  tip_id           UUID REFERENCES tips(id) ON DELETE SET NULL,
  -- For kind='image': on-disk relative path to the encrypted blob.
  blob_path        TEXT,
  blob_size_bytes  BIGINT,
  -- Tombstone for user-initiated delete: ciphertext blanked, row kept
  -- so the other party sees "[deleted]" rather than a confusing gap.
  deleted_at       TIMESTAMPTZ,
  created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  -- Hard 30-day server-side retention cap, enforced in DB defaults +
  -- by the messages-cleanup job. Per-user shorter retention is a
  -- client-side filter on top.
  expires_at       TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '30 days')
);
CREATE INDEX IF NOT EXISTS idx_messages_conv_created ON messages(conversation_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_expires     ON messages(expires_at);

-- ---------------------------------------------------------------------
-- Group invitations
-- ---------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS group_invitations (
  id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  conversation_id      UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  invited_by           UUID REFERENCES users(id) ON DELETE SET NULL,
  invited_user         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  status               TEXT NOT NULL DEFAULT 'pending'
                          CHECK (status IN ('pending', 'accepted', 'declined')),
  encrypted_group_key  TEXT,
  created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  responded_at         TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_invitations_user_pending
  ON group_invitations(invited_user) WHERE status = 'pending';
