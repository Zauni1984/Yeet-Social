-- Backfill: NFT posts created before the create_post fix were stored with
-- is_permanent = FALSE even though they got a ~100-year expiry (so they
-- linger in the global feed forever) and were meant to be permanent. They
-- never appeared in the per-user permanent list because that query filters
-- is_permanent = TRUE. Reconcile the column with the intent: every NFT post
-- is permanent.
UPDATE posts
   SET is_permanent = TRUE
 WHERE is_nft = TRUE
   AND is_permanent = FALSE;
