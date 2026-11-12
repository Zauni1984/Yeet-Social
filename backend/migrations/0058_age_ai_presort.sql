-- AI pre-sorting hint for the 18+ review queue (on-server age estimate,
-- only with the user's explicit consent). A hint for the reviewer, never a
-- decision; the probabilities are cleared when the blobs are purged.
ALTER TABLE age_verification_cases
    ADD COLUMN IF NOT EXISTS ai_consent_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS ai_verdict    TEXT,
    ADD COLUMN IF NOT EXISTS ai_adult_prob REAL,
    ADD COLUMN IF NOT EXISTS ai_child_prob REAL,
    ADD COLUMN IF NOT EXISTS ai_age_bucket TEXT,
    ADD COLUMN IF NOT EXISTS ai_face_score REAL,
    ADD COLUMN IF NOT EXISTS ai_faces      INTEGER,
    ADD COLUMN IF NOT EXISTS ai_model      TEXT,
    ADD COLUMN IF NOT EXISTS ai_ran_at     TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS ai_error      TEXT;
