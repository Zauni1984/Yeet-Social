-- Migration: Cover / banner image on users
ALTER TABLE users
  ADD COLUMN IF NOT EXISTS cover_url TEXT;
