-- Migration: Email double-opt-in (DSGVO) + wallet ↔ email linking
-- Adds email verification tracking and a token table for confirmation links.

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS email_verified_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS email_pending     TEXT;

CREATE TABLE IF NOT EXISTS email_verification_tokens (
  token       TEXT        PRIMARY KEY,
  user_id     UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  email       TEXT        NOT NULL,
  expires_at  TIMESTAMPTZ NOT NULL,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_email_tokens_user    ON email_verification_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_email_tokens_expires ON email_verification_tokens(expires_at);
