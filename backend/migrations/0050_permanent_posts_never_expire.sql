-- Permanent posts never expire.
--
-- Early versions created permanent posts with the regular 24-hour
-- expires_at, and the expiry sweep (cleanup_expired_posts) later
-- soft-deleted them — the author's "Permanent Posts" list shrank over time.
-- The sweep sets deleted_at only once expires_at has passed, so
-- deleted_at >= expires_at identifies exactly the rows it took (an author
-- deleting a post does so while expires_at is still in the future).

-- 1. Bring back permanent/NFT posts the sweep soft-deleted.
UPDATE posts
   SET deleted_at = NULL
 WHERE (is_permanent = TRUE OR is_nft = TRUE)
   AND deleted_at IS NOT NULL
   AND is_removed = FALSE
   AND deleted_at >= expires_at;

-- 2. Give every permanent/NFT post the far-future expiry it should have.
UPDATE posts
   SET expires_at = NOW() + INTERVAL '100 years'
 WHERE (is_permanent = TRUE OR is_nft = TRUE)
   AND expires_at < NOW() + INTERVAL '50 years';

-- 3. The sweep never touches permanent or NFT posts again, whatever their
--    expires_at says.
CREATE OR REPLACE FUNCTION cleanup_expired_posts()
RETURNS INTEGER AS $$
DECLARE deleted_count INTEGER;
BEGIN
    UPDATE posts
    SET deleted_at = NOW()
    WHERE expires_at < NOW()
      AND is_nft = FALSE
      AND is_permanent = FALSE
      AND deleted_at IS NULL;

    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$ LANGUAGE plpgsql;
