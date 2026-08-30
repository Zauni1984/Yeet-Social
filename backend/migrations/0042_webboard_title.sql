-- User-composable webboards: users curate their own list of RSS/Atom feeds
-- (stored in the previously orphaned webboard_connections table). Add an
-- optional display title so a saved feed can be labelled by the user; the host
-- (domain) is the fallback label and the per-user uniqueness key.
ALTER TABLE webboard_connections ADD COLUMN IF NOT EXISTS title TEXT;
