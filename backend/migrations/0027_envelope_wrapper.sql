-- Migration: Track who wrapped each group_key envelope so invitees can
-- actually unwrap it.
--
-- v1 of the messaging schema assumed every member could derive their
-- unwrap key as ECDH(self_sk, self_pk) — that worked for the conv
-- creator (who wrapped for themselves) but not for invited members,
-- whose envelopes were wrapped by the INVITER using
-- ECDH(inviter_sk, invitee_pk). The invitee needs to know the
-- inviter's pubkey to unwrap with ECDH(invitee_sk, inviter_pk).
--
-- We don't store the pubkey directly (it can change after a key
-- rotation upstream); instead we store the wrapper's user_id and let
-- the API surface their current pubkey at read time.

ALTER TABLE conversation_members
    ADD COLUMN IF NOT EXISTS wrapper_user_id UUID REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE group_invitations
    ADD COLUMN IF NOT EXISTS wrapper_user_id UUID REFERENCES users(id) ON DELETE SET NULL;

-- Backfill: existing members get the conversation's creator. For DMs
-- the wrapper is irrelevant (the conv_key comes from direct ECDH with
-- the peer), so this update is essentially a no-op for them.
UPDATE conversation_members cm
   SET wrapper_user_id = c.created_by
  FROM conversations c
 WHERE cm.conversation_id = c.id
   AND cm.wrapper_user_id IS NULL;

-- For pending invitations, the wrapper is the inviter.
UPDATE group_invitations
   SET wrapper_user_id = invited_by
 WHERE wrapper_user_id IS NULL;
