-- ====================================
-- Drop the unused user_client_grant (realm_id, user_id) index
-- ====================================
-- Every query on user_client_grant filters by (user_id, client_app_id),
-- which the table's UNIQUE constraint already indexes; nothing reads by
-- (realm_id, user_id). The index only added write amplification on the
-- /token exchange — the choke point every login entrance's codes pass
-- through. 0004 shipped with it; editing an already-applied migration
-- would fail sqlx's startup checksum validation, so the drop is a
-- migration of its own.

DROP INDEX IF EXISTS idx_user_client_grant_realm_user;

COMMENT ON COLUMN user_client_grant.realm_id IS 'Realm the grant was issued in (denormalized from account/client_app for tenant attribution)';
