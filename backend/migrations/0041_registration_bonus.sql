-- One-time signup bonus (tokenomics): 1000 points for the first 100k users who
-- complete BOTH email double-opt-in AND KYC/age verification.
--
-- The timestamp column does double duty:
--   * idempotency — a user with a non-NULL value has already been paid, so the
--     grant can be retried safely from either completion hook (email-verify or
--     KYC-approve), whichever finishes last;
--   * the first-N cap — COUNT(*) of non-NULL rows is how many bonuses have been
--     handed out, checked under an advisory lock before each grant.
ALTER TABLE users ADD COLUMN IF NOT EXISTS registration_bonus_granted_at TIMESTAMPTZ;

-- Partial index keeps the recipient COUNT(*) cheap even at 100k+ users.
CREATE INDEX IF NOT EXISTS idx_users_reg_bonus
  ON users (registration_bonus_granted_at)
  WHERE registration_bonus_granted_at IS NOT NULL;
