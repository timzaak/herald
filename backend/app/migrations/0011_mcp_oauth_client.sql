-- MCP OAuth client (DEC-mcp-server-003/005/006): the /mcp endpoint moves
-- from Client API Key auth to Herald-OAuth user credentials. This migration
-- adds the per-realm built-in public MCP client ('herald-mcp') and the
-- mcp_token_generation column that makes "disable kills every MCP
-- credential, re-enable does not resurrect them" survivable across a Redis
-- revocation failure (DEC-mcp-server-006).
--
-- Deploy order: run this SQL (additive column + seed) BEFORE upgrading the
-- application nodes, inside the maintenance window that upgrades all
-- OAuth/MCP nodes at once. Old and new nodes must never serve simultaneously
-- (old nodes neither know the column nor validate the MCP audience).

ALTER TABLE client_app
    ADD COLUMN mcp_token_generation BIGINT NOT NULL DEFAULT 0
    CHECK (mcp_token_generation >= 0);

-- Seed one built-in MCP client per existing realm. A realm that already
-- occupies the reserved id is a hard error (rollback): the row cannot be
-- "adopted" — an ordinary client app with the same id is not the MCP public
-- client and must not silently become one.
DO $$
DECLARE
    realm_row RECORD;
BEGIN
    FOR realm_row IN SELECT id FROM realm LOOP
        BEGIN
            INSERT INTO client_app (
                id, realm_id, client_id, name, description,
                redirect_uris, allowed_origins,
                email_verify_return_url, password_reset_return_url,
                browser_refresh_absolute_ttl_seconds,
                is_first_party, enabled, icon_url, client_secret,
                device_code_grant_enabled,
                turnstile_enabled, turnstile_site_key, turnstile_secret_key,
                mcp_token_generation, created_at, updated_at
            ) VALUES (
                uuidv7(), realm_row.id, 'herald-mcp', 'Herald MCP',
                'Built-in read-only AI agent access client',
                '["http://127.0.0.1/callback","http://localhost/callback","http://[::1]/callback"]'::jsonb,
                '[]'::jsonb,
                NULL, NULL,
                2592000,
                false, true, NULL, NULL,
                false,
                false, NULL, NULL,
                0, NOW(), NOW()
            );
        EXCEPTION WHEN unique_violation THEN
            RAISE EXCEPTION
                'realm % already has a client app named herald-mcp; resolve the conflict before migrating',
                realm_row.id;
        END;
    END LOOP;
END $$;

COMMENT ON COLUMN client_app.mcp_token_generation IS 'MCP disable generation (DEC-mcp-server-006): bumped by the atomic disable UPDATE on the built-in MCP client; MCP credentials carry the generation they were issued under and are rejected once stale. Always 0 for other clients.';
