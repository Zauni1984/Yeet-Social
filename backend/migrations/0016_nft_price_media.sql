-- NFT and media enhancements
ALTER TABLE posts
    ADD COLUMN IF NOT EXISTS nft_price_yeet  NUMERIC(18,4),
    ADD COLUMN IF NOT EXISTS is_permanent    BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS media_url       TEXT;

-- is_nft already exists, nft_price_yeet is new
-- is_permanent = TRUE means post doesn't expire after 24h
-- Update expires_at logic: if is_permanent, set expires_at far in future
