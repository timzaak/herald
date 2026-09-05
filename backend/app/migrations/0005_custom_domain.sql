-- ====================================
-- Custom Domain Mapping
-- ====================================
-- Stores the host→realm mapping for custom login domains.
-- This table is the request-time lookup surface for host→realm resolution
-- (middleware/CORS/ask/resolve) and the single-save/status-update targets.
-- It mirrors the effective custom-domain value stored in realm_config; active
-- hostname rows live here, keyed globally unique by hostname.

CREATE TABLE custom_domain_mapping (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    realm_id text NOT NULL,
    hostname text NOT NULL UNIQUE,
    enabled boolean NOT NULL DEFAULT true,
    cname_verified boolean NOT NULL DEFAULT false,
    tls_ready boolean NOT NULL DEFAULT false,
    status_checked_at timestamptz NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX custom_domain_mapping_hostname_idx
    ON custom_domain_mapping(hostname);
CREATE INDEX custom_domain_mapping_realm_idx
    ON custom_domain_mapping(realm_id);

COMMENT ON TABLE custom_domain_mapping IS 'Host→realm mapping for active custom login domains';
COMMENT ON COLUMN custom_domain_mapping.realm_id IS 'Realm this custom domain resolves to (no FK, matches realm_config style)';
COMMENT ON COLUMN custom_domain_mapping.hostname IS 'Precise custom login hostname, normalized (lowercase, trailing dot stripped); globally unique';
COMMENT ON COLUMN custom_domain_mapping.enabled IS 'Whether this mapping is active; request-time resolution keys solely on this';
COMMENT ON COLUMN custom_domain_mapping.cname_verified IS 'Surface-only: whether CNAME currently points to Herald cname target (not part of resolution)';
COMMENT ON COLUMN custom_domain_mapping.tls_ready IS 'Surface-only: whether Caddy has issued On-Demand TLS for the hostname (not part of resolution)';
COMMENT ON COLUMN custom_domain_mapping.status_checked_at IS 'Last time CNAME/TLS status was probed';
