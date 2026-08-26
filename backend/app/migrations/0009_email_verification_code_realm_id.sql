-- ====================================
-- Realm-scoped email verification codes
-- ====================================
-- email_verification_code had no realm linkage and every repository lookup
-- (verify / consume / get_email_by_code) matched globally on the bare code or
-- email+type. A code issued in one realm could therefore be matched, read, or
-- consumed via another realm's request path for the same email.
--
-- Existing rows are ephemeral verification codes with a 30-minute TTL
-- (EMAIL_VERIFICATION_CODE_TTL_SECONDS); the same email may exist in multiple
-- realms so they cannot be attributed reliably. Delete them instead of
-- backfilling: any live code is at most one re-request away from reissue.

DELETE FROM email_verification_code;

ALTER TABLE email_verification_code
    ADD COLUMN realm_id text NOT NULL;

-- Codes are looked up by (realm_id, email, type) on issue/verify and by
-- (realm_id, verification_code) on consume.
CREATE INDEX idx_email_verification_code_realm_email_type
    ON email_verification_code(realm_id, email, type);
CREATE INDEX idx_email_verification_code_realm_code
    ON email_verification_code(realm_id, verification_code);
