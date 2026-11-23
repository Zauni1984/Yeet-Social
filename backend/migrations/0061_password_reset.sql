-- Password reset by email link ("Forgot password").
-- Only the SHA-256 of the token is stored: a database leak must not hand out
-- working reset links. One open token per user; used and expired rows are
-- removed when a new link is requested or a reset completes.
CREATE TABLE IF NOT EXISTS password_reset_tokens (
  token_hash  TEXT        PRIMARY KEY,
  user_id     UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  expires_at  TIMESTAMPTZ NOT NULL,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_password_reset_user    ON password_reset_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_password_reset_expires ON password_reset_tokens(expires_at);
