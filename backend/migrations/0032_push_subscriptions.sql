-- Migration: web push subscriptions.
--
-- Each row is a single browser/device the user has opted in to push
-- notifications from. We never persist the message body in pushes —
-- they are "tickle" pushes only, which means the payload is empty
-- and the service worker renders a generic "New message"
-- notification. That keeps two invariants safe:
--   1. The push service (Google FCM, Mozilla autopush, Apple) never
--      sees plaintext message content.
--   2. The DB has no incentive to store anything decryptable.
--
-- Endpoint is unique so a given browser+origin can re-subscribe
-- idempotently. The auth key + p256dh key are required by the
-- Web Push spec (RFC 8291) but for the tickle pattern we use them
-- only on the wire format; the payload is empty.

CREATE TABLE IF NOT EXISTS push_subscriptions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Endpoint URL given by the browser. Different per device + origin.
    endpoint    TEXT NOT NULL UNIQUE,
    -- P-256 ECDH public key + auth secret that the browser would use
    -- if we shipped encrypted payloads. Kept for future upgrade to
    -- aes128gcm payloads; tickle pushes don't need them on the wire.
    p256dh_key  TEXT NOT NULL,
    auth_key    TEXT NOT NULL,
    user_agent  TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- When the push service responds with 410 Gone we mark the
    -- subscription dead instead of immediately deleting so the user
    -- can see in /me/sessions-style debugging that it was reaped.
    expired_at   TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_push_subs_user
    ON push_subscriptions(user_id)
    WHERE expired_at IS NULL;
