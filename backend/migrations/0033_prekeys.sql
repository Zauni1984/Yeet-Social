-- Migration: Signal-style prekey infrastructure (Forward Secrecy phase 1).
--
-- This is the foundation an X3DH handshake + Double Ratchet will build
-- on. It does NOT by itself change how messages are encrypted today
-- (that's the ratchet, a later phase) — but it lets a sender fetch a
-- fresh, one-time key bundle for a recipient, which is the prerequisite
-- for per-session forward secrecy.
--
-- Two key types, mirroring libsignal:
--   * signed prekey — a medium-lived ECDH P-256 public key, signed by
--     the user's long-term identity key so a peer can verify it really
--     belongs to them. Rotated periodically by the client. Exactly one
--     is active per user at a time.
--   * one-time prekeys — single-use ECDH P-256 public keys. The bundle
--     endpoint hands out (and atomically consumes) one per fetch. The
--     client replenishes when the unused count runs low.
--
-- All key material is opaque base64 to the server — same server-blind
-- posture as the existing identity keys. The signature is verified by
-- the *recipient's peer*, not the server.

-- The ECDH identity key (users.e2ee_public_key) can't itself produce
-- signatures — a WebCrypto ECDH key has no signing usage. So clients
-- additionally generate a dedicated ECDSA P-256 "signing identity key"
-- whose public half is stored here and shipped in the bundle, letting
-- a peer verify the signed-prekey signature. NULL until a client
-- provisions prekeys.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS e2ee_signing_public_key TEXT;

CREATE TABLE IF NOT EXISTS signed_prekeys (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Client-assigned monotonic id so the ratchet can reference which
    -- signed prekey a session was established with.
    key_id      INTEGER NOT NULL,
    public_key  TEXT NOT NULL,          -- base64 SPKI-DER P-256
    signature   TEXT NOT NULL,          -- base64 sig by identity key
    active      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, key_id)
);

-- Only one active signed prekey per user. Enforced with a partial
-- unique index so rotation = (insert new active, flip old inactive).
CREATE UNIQUE INDEX IF NOT EXISTS uq_signed_prekey_active
    ON signed_prekeys(user_id)
    WHERE active = TRUE;

CREATE TABLE IF NOT EXISTS one_time_prekeys (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    key_id      INTEGER NOT NULL,
    public_key  TEXT NOT NULL,          -- base64 SPKI-DER P-256
    -- NULL = still available. Set when the bundle endpoint hands it
    -- out, so it's never reused (the whole point of a one-time key).
    used_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, key_id)
);

-- Hot path: "give me one unused OTP for this user" + "how many are
-- left". Partial index keeps it tight as used rows accumulate.
CREATE INDEX IF NOT EXISTS idx_otp_available
    ON one_time_prekeys(user_id)
    WHERE used_at IS NULL;
