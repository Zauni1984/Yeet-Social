-- Migration: multi-device support for the Double Ratchet.
--
-- Phase 1/2 stored prekeys per USER, which is implicitly single-device:
-- a second device's provisioning overwrites the first device's signed
-- prekey, and a message encrypted to device A's session can't be read
-- on device B. This migration makes prekeys, signing keys and bundles
-- per-DEVICE while keeping the ECDH identity key shared across a user's
-- devices (it stays recoverable via the wallet/password master, which
-- is the app's existing identity-recovery model).
--
-- Model: one identity (shared), many devices. Each device has its own
-- ECDSA signing key, its own signed prekey, and its own one-time
-- prekeys. A sender fans a message out to every recipient device plus
-- their own other devices; each device decrypts its own entry.
--
-- The signed-prekey signature is now verified per device against that
-- device's signing key (user_devices.signing_public_key), so the old
-- per-user users.e2ee_signing_public_key is superseded (kept nullable
-- for backward compat but no longer read by the bundle path).
--
-- Early-stage cleanup: the per-user prekey rows from 0033 have no
-- device_id and can't participate in the per-device flow, so we drop
-- them. (Acceptable: prekeys are ephemeral by design and clients
-- re-provision automatically on next identity unlock.)

-- ── devices ──────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS user_devices (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id              UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Client-generated stable id (UUID) kept in that browser/device's
    -- localStorage. Opaque to the server.
    device_id            TEXT NOT NULL,
    -- ECDSA P-256 public key used to verify this device's signed
    -- prekey signature. Per-device (each device generates its own).
    signing_public_key   TEXT NOT NULL,
    -- Optional human label ("Stefan's iPhone") for a future device-
    -- management UI. Not shown anywhere yet.
    label                TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, device_id)
);

CREATE INDEX IF NOT EXISTS idx_user_devices_user ON user_devices(user_id);

-- ── prekeys become per-device ────────────────────────────────────────
-- Wipe the single-device rows from 0033 first (see header note).
DELETE FROM signed_prekeys;
DELETE FROM one_time_prekeys;

ALTER TABLE signed_prekeys
    ADD COLUMN IF NOT EXISTS device_id TEXT NOT NULL DEFAULT '';
ALTER TABLE one_time_prekeys
    ADD COLUMN IF NOT EXISTS device_id TEXT NOT NULL DEFAULT '';

-- Replace the per-user uniqueness/index with per-device equivalents.
DROP INDEX IF EXISTS uq_signed_prekey_active;
ALTER TABLE signed_prekeys DROP CONSTRAINT IF EXISTS signed_prekeys_user_id_key_id_key;
CREATE UNIQUE INDEX IF NOT EXISTS uq_signed_prekey_active_dev
    ON signed_prekeys(user_id, device_id)
    WHERE active = TRUE;
CREATE UNIQUE INDEX IF NOT EXISTS uq_signed_prekey_dev_keyid
    ON signed_prekeys(user_id, device_id, key_id);

DROP INDEX IF EXISTS idx_otp_available;
ALTER TABLE one_time_prekeys DROP CONSTRAINT IF EXISTS one_time_prekeys_user_id_key_id_key;
CREATE UNIQUE INDEX IF NOT EXISTS uq_otp_dev_keyid
    ON one_time_prekeys(user_id, device_id, key_id);
CREATE INDEX IF NOT EXISTS idx_otp_available_dev
    ON one_time_prekeys(user_id, device_id)
    WHERE used_at IS NULL;
