-- 0026_indexer_state.sql
--
-- Persists per-indexer scan checkpoints so the Transfer-event indexer
-- and any future on-chain watchers resume after a backend restart
-- instead of re-scanning from chain genesis (which would be expensive)
-- or skipping the gap since the last poll (which would silently drop
-- tip notifications).

CREATE TABLE IF NOT EXISTS indexer_state (
    indexer_key TEXT PRIMARY KEY,
    last_block  BIGINT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
