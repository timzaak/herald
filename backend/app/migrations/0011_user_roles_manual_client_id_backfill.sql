-- Backfill: unify manual-grant provenance (audit run-1:
-- herald:api-admin/permission/user_roles/manual-grant-client-id-provenance-replace-revocation-gap).
-- The permission-module POST /api/permission/users/{id}/roles surface wrote
-- client_id = '' (Identity::client_id() is the empty string for the
-- Identity::User callers that reach it), diverging from the
-- 'admin-web-console' provenance every other admin surface writes
-- (users-module PUT/create-user, realm bootstrap). client_id is
-- provenance-only for manual rows — the partial unique index
-- idx_user_roles_principal_role_manual ignores it — so the rewrite cannot
-- collide: at most one manual row per (realm_id, principal_type,
-- principal_id, role_id) exists. Idempotent: rows already carrying a
-- provenance value are untouched.
UPDATE user_roles
SET client_id = 'admin-web-console'
WHERE source = 'manual' AND COALESCE(client_id, '') = '';
