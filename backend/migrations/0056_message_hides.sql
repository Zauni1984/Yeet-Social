-- 0056: "delete for me" and server-side DM retention (audit MSG-M1 / MSG-M8).
--
-- Before: DELETE /messages/:id blanked the ciphertext for BOTH parties with
-- no time window (it was "delete for everyone" in disguise), and the per-user
-- retention setting (1/7/30 days) was only a client-side display filter.
--
-- Now: a per-viewer hide row removes a message from that viewer's view only;
-- the hourly cleanup job inserts hides once a message is older than the
-- member's retention and hard-deletes a message once every member hides it.
CREATE TABLE IF NOT EXISTS message_hides (
    message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    hidden_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (message_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_message_hides_user ON message_hides(user_id);
