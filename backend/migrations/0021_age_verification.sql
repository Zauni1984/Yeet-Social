-- Migration: Per-account age verification (syncs across devices)
ALTER TABLE users
  ADD COLUMN IF NOT EXISTS age_verified_at TIMESTAMPTZ;
