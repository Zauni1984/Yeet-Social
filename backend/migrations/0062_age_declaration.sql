-- The service is offered to adults only (Terms of Service, section on
-- eligibility). Sign-up asks for the confirmation "I am at least 18"; this
-- column records when it was given. NULL for accounts created before the
-- rule and for wallet sign-ins, which have no sign-up form.
ALTER TABLE users ADD COLUMN IF NOT EXISTS age_declared_at TIMESTAMPTZ;
