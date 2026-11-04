-- phone-login: phone as a parallel login identifier (E.164, DEC-phone-login-004).
-- Uniqueness is per realm on the normalized form; NULL (unbound) rows are
-- excluded by the partial index, mirroring the username precedent (0001 L46).
ALTER TABLE account ADD COLUMN phone TEXT;
CREATE UNIQUE INDEX account_realm_id_phone_index ON account(realm_id, phone) WHERE phone IS NOT NULL;
