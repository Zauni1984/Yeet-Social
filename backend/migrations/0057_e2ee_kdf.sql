-- MSG-H5: the E2EE master key is no longer derived from the login password
-- (which the server sees at login) but from a dedicated message passphrase.
-- `e2ee_kdf` records how the stored wrapped private key was derived so the
-- client knows what to ask for:
--   login-v1      legacy: PBKDF2 over the login password (or wallet signature)
--   passphrase-v2 PBKDF2 over the dedicated message passphrase
--   wallet-v1     wallet signature (never leaves the device)
ALTER TABLE users ADD COLUMN IF NOT EXISTS e2ee_kdf TEXT NOT NULL DEFAULT 'login-v1';
