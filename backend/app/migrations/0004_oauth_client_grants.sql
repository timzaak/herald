-- ====================================
-- OAuth Client Grants (third-party authorization consent memory)
-- ====================================
-- Remembers that a user has authorized a third-party Client App through the
-- Authorization Code + PKCE flow. `/authorize` no longer assumes an
-- unauthenticated browser: authenticated users with an existing grant get a
-- code without re-entering credentials, while authenticated users without
-- one see the consent page. A row is written when a code is successfully
-- exchanged at `/token` (the single choke point every login entrance's codes
-- pass through), so password/TOTP/passkey/LDAP/social completions all count
-- as the user's authorization of that client — matching the pre-consent-page
-- semantics where completing login on the continuation page minted the code.

CREATE TABLE user_client_grant (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    realm_id text NOT NULL,
    user_id uuid NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    client_app_id uuid NOT NULL REFERENCES client_app(id) ON DELETE CASCADE,
    granted_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT user_client_grant_user_client_unique UNIQUE (user_id, client_app_id)
);

CREATE INDEX idx_user_client_grant_realm_user ON user_client_grant(realm_id, user_id);

COMMENT ON TABLE user_client_grant IS 'Per-user memory of third-party OAuth client authorizations; presence lets /authorize complete silently for authenticated users';
COMMENT ON COLUMN user_client_grant.realm_id IS 'Realm the grant was issued in (denormalized from account/client_app for realm-scoped lookups)';
COMMENT ON COLUMN user_client_grant.user_id IS 'User who authorized the client (FK account.id, soft-deleted accounts keep rows but can no longer authenticate)';
COMMENT ON COLUMN user_client_grant.client_app_id IS 'Authorized Client App (FK client_app.id)';
COMMENT ON INDEX user_client_grant_user_client_unique IS 'One grant per user per client; re-authorization is INSERT ... ON CONFLICT DO NOTHING';
