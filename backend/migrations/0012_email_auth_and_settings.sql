-- Migration: Email auth + User settings
-- Run after existing migrations

-- 1. Add email auth columns to users table
ALTER TABLE users
  ADD COLUMN IF NOT EXISTS email          TEXT UNIQUE,
  ADD COLUMN IF NOT EXISTS password_hash  TEXT,
  ADD COLUMN IF NOT EXISTS password_salt  TEXT;

-- Allow wallet_address to be NULL (email-only users)
ALTER TABLE users
  ALTER COLUMN wallet_address DROP NOT NULL;

-- Index for email lookups
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users (email)
  WHERE email IS NOT NULL;

-- 2. User settings table
CREATE TABLE IF NOT EXISTS user_settings (
  user_id              UUID        PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  currency             TEXT        NOT NULL DEFAULT 'USD',
  language             TEXT        NOT NULL DEFAULT 'en',
  show_nsfw            BOOLEAN     NOT NULL DEFAULT FALSE,
  email_notifications  BOOLEAN     NOT NULL DEFAULT TRUE,
  push_notifications   BOOLEAN     NOT NULL DEFAULT TRUE,
  auto_play_media      BOOLEAN     NOT NULL DEFAULT TRUE,
  compact_mode         BOOLEAN     NOT NULL DEFAULT FALSE,
  created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 3. Add check: user must have wallet_address OR email (or both)
ALTER TABLE users
  ADD CONSTRAINT users_must_have_identity
  CHECK (wallet_address IS NOT NULL OR email IS NOT NULL);