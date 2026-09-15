-- Platform-level OIDC signing keys (RS256 id_token signing). One row per
-- generated RSA-2048 key; `id` doubles as the JWKS/id_token `kid`.
--   * status state machine: active (signing + published) -> retained
--     (overlap window, published only) -> retired (not published);
--   * the partial unique index enforces at most one platform-wide active key,
--     which is the concurrency guard for both startup bootstrapping and
--     rotation (the loser of a 23505 treats the winner as authoritative);
--   * private_key_pem_encrypted stores AES-256-GCM ciphertext as
--     nonce(12B) || ciphertext; the KEK is derived from [jwt] secret, so
--     rotating that secret invalidates stored keys — run one signing-key
--     rotation after changing [jwt] secret.
-- No public-key column: n/e are derived from the private key on read, so
-- the pair can never drift out of sync.
CREATE TABLE oidc_signing_key (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    status TEXT NOT NULL CHECK (status IN ('active', 'retained', 'retired')),
    private_key_pem_encrypted BYTEA NOT NULL,
    retained_until TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX oidc_signing_key_single_active_idx
    ON oidc_signing_key (status) WHERE status = 'active';

COMMENT ON TABLE oidc_signing_key IS 'Platform-level OIDC RS256 signing keys: active key signs id_tokens, retained keys stay published in JWKS through their overlap window';
COMMENT ON COLUMN oidc_signing_key.status IS 'active = signs and is published; retained = published only until retained_until; retired = no longer published';
COMMENT ON COLUMN oidc_signing_key.retained_until IS 'Overlap-window deadline for retained keys; NULL for active/retired';
