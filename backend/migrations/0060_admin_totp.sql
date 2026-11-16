-- Mandatory second factor for the admin surface. Admin access is a shared
-- secret (ADMIN_SECRET), not an account, so the authenticator is a single
-- installation-wide TOTP: one row, secret encrypted like user 2FA secrets.
CREATE TABLE IF NOT EXISTS admin_totp (
    id              INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    secret_enc      TEXT,
    pending_enc     TEXT,
    enabled_at      TIMESTAMPTZ,
    recovery_hashes TEXT[] NOT NULL DEFAULT '{}'
);
INSERT INTO admin_totp (id) VALUES (1) ON CONFLICT DO NOTHING;
