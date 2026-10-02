-- F7 — consumer consent for pay-per-view purchases (docs/mica/04 §B2,
-- docs/rechtstexte.md). Digital content not supplied on a tangible medium:
-- the buyer must expressly request immediate performance and acknowledge
-- that the right of withdrawal is lost (§ 356 Abs. 5 BGB / Art. 16(m)
-- Directive 2011/83/EU). We record which notice version was accepted,
-- when, and in which language, on the unlock row itself.
ALTER TABLE ppv_unlocks
    ADD COLUMN IF NOT EXISTS consent_version TEXT,
    ADD COLUMN IF NOT EXISTS consent_at      TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS consent_lang    TEXT;
