-- Two-factor authentication (TOTP / authenticator app).
--   totp_secret_enc    active secret, AES-256-GCM (nonce||ct, base64) under a
--                      key derived from JWT_SECRET; never stored in clear.
--   totp_pending_enc   provisional secret during setup, before the first code
--                      is verified; cleared on activate or on a new setup.
--   totp_enabled_at    non-null once 2FA is active.
--   totp_recovery_hashes  SHA-256 hex of one-time recovery codes; a used code
--                      is removed from the array.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS totp_secret_enc      TEXT,
    ADD COLUMN IF NOT EXISTS totp_pending_enc     TEXT,
    ADD COLUMN IF NOT EXISTS totp_enabled_at      TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS totp_recovery_hashes TEXT[] NOT NULL DEFAULT '{}';
