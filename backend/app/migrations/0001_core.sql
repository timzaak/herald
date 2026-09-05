-- ====================================
-- Herald Core Schema (baseline)
-- ====================================
-- Consolidates identity, RBAC, TOTP, API keys, OAuth config, audit events,
-- and email tables into a single baseline. Pre-launch squash: no ALTER/DROP.
-- The user_roles role-grant provenance/expiry columns and split manual/payment
-- partial unique indexes (former 0006_support_paywall) are inlined.

-- ====================================
-- UUID v7 Support (PostgreSQL 18+)
-- ====================================
-- REQUIREMENT: PostgreSQL 18 or higher
-- PostgreSQL 18+ natively supports UUID v7 via the uuidv7() function.
-- This migration will NOT work on PostgreSQL versions below 18.
-- No extension installation required.

-- ====================================
-- Core Tables
-- ====================================

-- Account table (users)
CREATE TABLE account (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    realm_id text,
    email text NOT NULL,
    username text,
    password text,
    provider_ids uuid[] DEFAULT '{}'::uuid[],
    status smallint NOT NULL DEFAULT 0,
    deleted_original_email_hash text,
    created_at timestamp with time zone DEFAULT now(),
    updated_at timestamptz DEFAULT now()
);

CREATE UNIQUE INDEX account_realm_id_email_index ON account(realm_id, email);
CREATE UNIQUE INDEX account_realm_id_username_index ON account(realm_id, username) WHERE username IS NOT NULL;
CREATE INDEX idx_account_realm_created ON account(realm_id, created_at DESC);
-- One-way hash of original email kept after soft-delete so login with the
-- original address can detect the deleted account and surface a Forbidden
-- response, without exposing the original address.
CREATE INDEX idx_account_deleted_email_hash
    ON account (realm_id, deleted_original_email_hash)
    WHERE deleted_original_email_hash IS NOT NULL;
COMMENT ON TABLE account IS 'User accounts for authentication';
COMMENT ON COLUMN account.status IS '0: wait verified, 1: normal, 2: forbid, 3: deleted';
COMMENT ON COLUMN account.username IS 'Optional username for login, can be used instead of email';
COMMENT ON COLUMN account.provider_ids IS 'Array of OAuth provider IDs linked to this account';
COMMENT ON COLUMN account.deleted_original_email_hash IS 'One-way hash of original email retained after soft-delete for deleted-account detection';

-- Profile table (user profiles)
CREATE TABLE profile (
    id uuid,
    realm_id text,
    nickname text,
    created_at timestamp with time zone DEFAULT now(),
    updated_at timestamptz DEFAULT now()
);

CREATE UNIQUE INDEX profile_id_realm_id_index ON profile(id, realm_id);
COMMENT ON TABLE profile IS 'User profile information';

-- Provider table (OAuth provider accounts)
CREATE TABLE provider (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    realm_id text NOT NULL,
    type text NOT NULL,
    open_id text NOT NULL,
    union_id text,
    email text,
    user_id uuid REFERENCES account(id) ON DELETE CASCADE,
    created_at timestamp with time zone DEFAULT now(),
    updated_at timestamptz DEFAULT now()
);

CREATE UNIQUE INDEX provider_realm_id_type_open_id_index ON provider(realm_id, type, open_id);
CREATE INDEX idx_provider_user_id ON provider(user_id);
CREATE UNIQUE INDEX idx_provider_user_id_type ON provider(user_id, type) WHERE user_id IS NOT NULL;
-- WeChat OAuth support: union_id lookup for cross-app user matching.
CREATE INDEX idx_provider_union_id
    ON provider(realm_id, union_id)
    WHERE union_id IS NOT NULL;
COMMENT ON TABLE provider IS 'OAuth provider account associations';
COMMENT ON COLUMN provider.type IS 'OAuth provider type (google, github, facebook, apple)';
COMMENT ON COLUMN provider.open_id IS 'OpenID from the OAuth provider';
COMMENT ON COLUMN provider.union_id IS 'Union ID for cross-provider identity matching';
COMMENT ON COLUMN provider.user_id IS 'Reference to the account (user) that owns this OAuth provider';

-- Realm table (tenant/realm)
CREATE TABLE realm (
    id text PRIMARY KEY DEFAULT uuidv7(),
    name text NOT NULL,
    description text,
    created_at timestamp with time zone DEFAULT now(),
    updated_at timestamptz DEFAULT now()
);

COMMENT ON TABLE realm IS 'Multi-tenant realms for isolation';

-- Client application table
CREATE TABLE client_app (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    realm_id text NOT NULL,
    client_id text NOT NULL,
    name text NOT NULL,
    description text,
    redirect_uris jsonb DEFAULT '[]'::jsonb NOT NULL,
    allowed_origins jsonb DEFAULT '[]'::jsonb NOT NULL,
    email_verify_return_url text,
    password_reset_return_url text,
    browser_refresh_absolute_ttl_seconds integer DEFAULT 2592000 NOT NULL,
    is_first_party boolean DEFAULT false NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    icon_url text,
    client_secret text,
    device_code_grant_enabled boolean NOT NULL DEFAULT false,
    -- Turnstile (Cloudflare human-verification), delegated to the Client App.
    -- `turnstile_enabled` defaults to false so existing Client
    -- Apps keep the "not configured -> skip" behaviour until enabled.
    turnstile_enabled boolean NOT NULL DEFAULT false,
    turnstile_site_key text,
    turnstile_secret_key text,
    created_at timestamp with time zone DEFAULT now(),
    updated_at timestamptz DEFAULT now(),
    CONSTRAINT client_app_realm_client_idx UNIQUE (realm_id, client_id),
    CONSTRAINT chk_client_app_allowed_origins_array
        CHECK (jsonb_typeof(allowed_origins) = 'array'),
    CONSTRAINT chk_client_app_browser_refresh_absolute_ttl
        CHECK (browser_refresh_absolute_ttl_seconds BETWEEN 86400 AND 7776000),
    CONSTRAINT chk_client_app_first_party_reserved
        CHECK (
            NOT is_first_party
            OR client_id IN ('admin-web-console', 'user-account-center')
        )
);

CREATE INDEX client_app_id_realm_id_index ON client_app(id, realm_id);
CREATE INDEX idx_client_app_realm_enabled ON client_app(realm_id, enabled);
COMMENT ON TABLE client_app IS 'OAuth client applications';
COMMENT ON COLUMN client_app.turnstile_enabled IS
    'Whether Cloudflare Turnstile human-verification is enforced for this Client App';
COMMENT ON COLUMN client_app.turnstile_site_key IS
    'Cloudflare Turnstile site key (public) shown to the client widget; NULL when Turnstile is disabled';
COMMENT ON COLUMN client_app.turnstile_secret_key IS
    'Cloudflare Turnstile secret key (server-side, sensitive); NULL when Turnstile is disabled';
COMMENT ON COLUMN client_app.id IS 'Internal UUID primary key (UUID v7)';
COMMENT ON COLUMN client_app.client_id IS 'External client identifier for API usage (alphanumeric, 3-36 chars)';
COMMENT ON COLUMN client_app.redirect_uris IS 'Redirect URI whitelist, JSON array format, must contain at least one valid HTTPS address (HTTP allowed in dev)';
COMMENT ON COLUMN client_app.allowed_origins IS 'Normalized exact origins allowed to call Herald APIs';
COMMENT ON COLUMN client_app.email_verify_return_url IS 'Pre-registered return URL for email verification results';
COMMENT ON COLUMN client_app.password_reset_return_url IS 'Pre-registered return URL for password reset results';
COMMENT ON COLUMN client_app.browser_refresh_absolute_ttl_seconds IS 'Absolute browser refresh token family lifetime in seconds (1-90 days)';
COMMENT ON COLUMN client_app.is_first_party IS 'Internal trust marker reserved for built-in Herald browser clients';
COMMENT ON COLUMN client_app.enabled IS 'Whether the client app is enabled, OAuth authorization cannot be completed when disabled';
COMMENT ON COLUMN client_app.icon_url IS 'App icon URL, optional';
COMMENT ON COLUMN client_app.client_secret IS 'OAuth client secret, UUID auto-generated on creation';
COMMENT ON INDEX client_app_realm_client_idx IS 'Unique constraint on (realm_id, client_id) for external lookups';
COMMENT ON INDEX idx_client_app_realm_enabled IS 'Index for filtering enabled client apps by realm';

-- ====================================
-- RBAC Tables (Role-Based Access Control)
-- ====================================

-- Roles table
CREATE TABLE roles (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    name TEXT NOT NULL,
    description text,
    realm_id text NOT NULL,
    client_id text NOT NULL,
    is_builtin BOOLEAN NOT NULL DEFAULT FALSE,
    created_at timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT roles_unique_name_realm_client UNIQUE (name, realm_id, client_id)
);

CREATE INDEX idx_roles_realm_client ON roles(realm_id, client_id);
CREATE INDEX idx_roles_is_builtin ON roles(is_builtin);
COMMENT ON TABLE roles IS 'User roles for access control';
COMMENT ON COLUMN roles.is_builtin IS 'TRUE indicates system built-in role, cannot be deleted or renamed';
COMMENT ON INDEX idx_roles_is_builtin IS 'Index on roles.is_builtin for filtering built-in roles';

-- Permissions table
CREATE TABLE permissions (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    name TEXT NOT NULL,
    description text,
    realm_id text NOT NULL,
    resource text NOT NULL DEFAULT '',
    action text NOT NULL DEFAULT '',
    is_builtin BOOLEAN NOT NULL DEFAULT FALSE,
    created_at timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT permissions_unique_name_realm_id UNIQUE (name, realm_id),
    CONSTRAINT chk_permission_format CHECK (resource <> '' AND action <> '')
);

CREATE INDEX idx_permissions_realm_id ON permissions(realm_id);
CREATE INDEX idx_permissions_resource ON permissions(resource);
CREATE INDEX idx_permissions_action ON permissions(action);
CREATE INDEX idx_permissions_resource_action ON permissions(resource, action);
CREATE INDEX idx_permissions_is_builtin ON permissions(is_builtin);
COMMENT ON TABLE permissions IS 'Permissions for RBAC at Realm level (no client_id)';
COMMENT ON COLUMN permissions.resource IS 'Resource type (e.g., "users", "profile", "clients")';
COMMENT ON COLUMN permissions.action IS 'Action type (e.g., "manage", "view", "update")';
COMMENT ON COLUMN permissions.is_builtin IS 'TRUE indicates system built-in permission, cannot be deleted';
COMMENT ON INDEX idx_permissions_resource IS 'Index on permissions.resource for filtering by resource';
COMMENT ON INDEX idx_permissions_action IS 'Index on permissions.action for filtering by action';
COMMENT ON INDEX idx_permissions_resource_action IS 'Index on permissions.resource and action for filtering by both';
COMMENT ON INDEX idx_permissions_is_builtin IS 'Index on permissions.is_builtin for filtering built-in permissions';

-- Role-Permission associations
CREATE TABLE role_permissions (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    role_id uuid NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id uuid NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    created_at timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT unique_role_permission UNIQUE (role_id, permission_id)
);

CREATE INDEX idx_role_permissions_role_id ON role_permissions(role_id);
CREATE INDEX idx_role_permissions_permission_id ON role_permissions(permission_id);
COMMENT ON TABLE role_permissions IS 'Many-to-many relationship between roles and permissions';

-- User-Role associations
-- Pre-launch squash: the role-grant provenance/expiry columns
-- (source/source_id/expires_at) and the split manual/payment partial unique
-- indexes (which admit the same role from multiple sources) are
-- inlined from the former 0006_support_paywall migration. No ALTER/DROP.
CREATE TABLE user_roles (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    user_id UUID,
    role_id UUID NOT NULL,
    realm_id TEXT NOT NULL,
    client_id TEXT,
    principal_type TEXT NOT NULL DEFAULT 'user',
    principal_id TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'manual',
    source_id TEXT NULL,
    expires_at TIMESTAMPTZ NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    CONSTRAINT fk_user_roles_role FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE,
    CONSTRAINT fk_user_roles_realm FOREIGN KEY (realm_id) REFERENCES realm(id) ON DELETE CASCADE
);

CREATE INDEX idx_user_roles_user_id ON user_roles(user_id);
CREATE INDEX idx_user_roles_role_id ON user_roles(role_id);
CREATE INDEX idx_user_roles_realm_id ON user_roles(realm_id);
-- Principal-role uniqueness is split across manual and payment sources so the
-- same role can coexist from manual + payment grants (PRD §4.2). source_id is
-- always NULL for manual grants (excluded from the key); for payment grants it
-- is the payment origin (attempt_id / subscription_id), so distinct purchases
-- coexist and revoking one source leaves the other intact (grant/revoke/
-- idempotency code agrees on this key — BE-D03).
CREATE UNIQUE INDEX idx_user_roles_principal_role_manual
    ON user_roles(realm_id, principal_type, principal_id, role_id)
    WHERE source = 'manual';
CREATE UNIQUE INDEX idx_user_roles_principal_role_payment
    ON user_roles(realm_id, principal_type, principal_id, role_id, source_id)
    WHERE source = 'payment';
-- Revoke-lookup partial index: used by the payment-role revocation path to find
-- rows by payment origin.
CREATE INDEX idx_user_roles_source
    ON user_roles(source, source_id) WHERE source = 'payment';
COMMENT ON TABLE user_roles IS 'Many-to-many relationship between principals (users, api_keys, clients) and roles';
COMMENT ON COLUMN user_roles.user_id IS 'UUID of the user (nullable for non-user principals)';
COMMENT ON COLUMN user_roles.role_id IS 'UUID of the role (references roles.id)';
COMMENT ON COLUMN user_roles.realm_id IS 'Realm ID for multi-tenant isolation';
COMMENT ON COLUMN user_roles.client_id IS 'Client app identifier (nullable for non-user principals)';
COMMENT ON COLUMN user_roles.principal_type IS 'Type of principal: "user", "api_key", or "client"';
COMMENT ON COLUMN user_roles.principal_id IS 'ID of the principal (user_id for users, client_api_keys.id for API keys)';
COMMENT ON COLUMN user_roles.source IS
    'Grant origin: ''manual'' (hand-assigned) or ''payment'' (granted on payment success).';
COMMENT ON COLUMN user_roles.source_id IS
    'Payment origin identifier. one_time = payment attempt_id; subscription = subscription_id. NULL for manual grants.';
COMMENT ON COLUMN user_roles.expires_at IS
    'INFORMATIONAL/PROVENANCE ONLY: the subscription billing period end aligned '
    'at grant time. Not an authz TTL — there is NO background sweep that removes '
    'rows once this timestamp passes. Role removal is event-driven (ImmediateCancel '
    'webhook → revoke_roles_by_payment_source). NULL for manual or permanent '
    'one-time grants. Kept for observability/audit of when the grant was meant to lapse.';

-- Role Policies table (RBAC policies for roles)
CREATE TABLE role_policies (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    role_id UUID NOT NULL,
    realm_id TEXT NOT NULL,
    resource TEXT NOT NULL,
    action TEXT NOT NULL,
    effect BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    CONSTRAINT fk_role_policies_role FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE,
    CONSTRAINT fk_role_policies_realm FOREIGN KEY (realm_id) REFERENCES realm(id) ON DELETE CASCADE,
    CONSTRAINT uq_role_policies UNIQUE (role_id, resource, action)
);

CREATE INDEX idx_role_policies_role_id ON role_policies(role_id);
CREATE INDEX idx_role_policies_realm_id ON role_policies(realm_id);
CREATE INDEX idx_role_policies_resource_action ON role_policies(resource, action);
COMMENT ON TABLE role_policies IS 'RBAC policies defining what resources/actions roles can access';
COMMENT ON COLUMN role_policies.resource IS 'Resource identifier (e.g., "users", "clients")';
COMMENT ON COLUMN role_policies.action IS 'Action identifier (e.g., "view", "manage")';
COMMENT ON COLUMN role_policies.effect IS 'True for allow, false for deny';

-- ====================================
-- Authentication Tables (TOTP)
-- ====================================

-- User TOTP configuration table
CREATE TABLE user_totp_config (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    user_id uuid NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    realm_id text NOT NULL,
    secret_hash text NOT NULL,
    key_version integer DEFAULT 1,
    enabled boolean NOT NULL DEFAULT false,
    verified_at timestamptz,
    last_used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX uniq_user_totp_config_user_id ON user_totp_config(user_id);
CREATE INDEX idx_user_totp_config_realm_id ON user_totp_config(realm_id);
CREATE INDEX idx_user_totp_config_enabled ON user_totp_config(enabled);
CREATE INDEX idx_user_totp_config_key_version ON user_totp_config(key_version);
CREATE INDEX idx_user_totp_config_realm_version ON user_totp_config(realm_id, key_version);
COMMENT ON TABLE user_totp_config IS 'User TOTP configuration for two-factor authentication';
COMMENT ON COLUMN user_totp_config.secret_hash IS 'AES-256-GCM encrypted TOTP secret key';
COMMENT ON COLUMN user_totp_config.key_version IS 'Version of realm TOTP key used for encryption (reserved for future rotation, currently fixed at 1)';
COMMENT ON COLUMN user_totp_config.enabled IS 'Whether TOTP is enabled for this user';
COMMENT ON COLUMN user_totp_config.verified_at IS 'When the user verified TOTP setup';
COMMENT ON INDEX idx_user_totp_config_key_version IS 'Index on key_version for future key rotation support';
COMMENT ON INDEX idx_user_totp_config_realm_version IS 'Composite index on realm_id and key_version for future key rotation support';

-- User TOTP backup codes table
CREATE TABLE user_totp_backup_codes (
    id bigserial PRIMARY KEY,
    user_totp_config_id uuid NOT NULL REFERENCES user_totp_config(id) ON DELETE CASCADE,
    code_hash text NOT NULL,
    used boolean NOT NULL DEFAULT false,
    used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_user_totp_backup_codes_config_id ON user_totp_backup_codes(user_totp_config_id);
CREATE INDEX idx_user_totp_backup_codes_used ON user_totp_backup_codes(used);
COMMENT ON TABLE user_totp_backup_codes IS 'Backup recovery codes for TOTP';
COMMENT ON COLUMN user_totp_backup_codes.code_hash IS 'bcrypt hashed backup code';
COMMENT ON COLUMN user_totp_backup_codes.used IS 'Whether this backup code has been used';

-- ====================================
-- API Keys Table
-- ====================================

-- Client API Keys table for client app API access
CREATE TABLE client_api_keys (
    id TEXT PRIMARY KEY DEFAULT uuidv7()::text,
    name TEXT NOT NULL,
    api_key_hash TEXT UNIQUE NOT NULL,
    realm_id TEXT NOT NULL,
    client_app_id UUID REFERENCES client_app(id) ON DELETE RESTRICT,
    enabled BOOLEAN DEFAULT true,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ,
    CONSTRAINT fk_client_api_keys_realm FOREIGN KEY (realm_id)
        REFERENCES realm(id) ON DELETE CASCADE
);

CREATE INDEX idx_client_api_keys_realm ON client_api_keys(realm_id);
CREATE INDEX idx_client_api_keys_enabled ON client_api_keys(enabled);
CREATE INDEX idx_client_api_keys_client_app_id ON client_api_keys(client_app_id);
CREATE INDEX idx_client_api_keys_app_realm ON client_api_keys(client_app_id, realm_id);
COMMENT ON TABLE client_api_keys IS 'API keys for client app programmatic access';
COMMENT ON COLUMN client_api_keys.api_key_hash IS 'Hashed API key for secure storage';
COMMENT ON COLUMN client_api_keys.client_app_id IS 'Client App this API key belongs to (1:1 relationship)';

-- ====================================
-- OAuth Configuration Tables
-- ====================================

-- OAuth provider-specific configuration table
CREATE TABLE oauth_provider_config (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    realm_id text NOT NULL,
    provider_type text NOT NULL,
    client_id text NOT NULL,
    client_secret text NOT NULL,
    scopes text[] DEFAULT '{}',
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamp with time zone DEFAULT now(),
    updated_at timestamptz DEFAULT now()
);

CREATE UNIQUE INDEX oauth_provider_config_realm_provider_idx ON oauth_provider_config(realm_id, provider_type);
CREATE INDEX oauth_provider_config_realm_idx ON oauth_provider_config(realm_id);
CREATE INDEX oauth_provider_config_enabled_idx ON oauth_provider_config(realm_id) WHERE enabled = true;
COMMENT ON TABLE oauth_provider_config IS 'OAuth provider configurations per realm';
COMMENT ON COLUMN oauth_provider_config.realm_id IS 'Realm identifier';
COMMENT ON COLUMN oauth_provider_config.provider_type IS 'OAuth provider type (google, github, facebook, apple)';
COMMENT ON COLUMN oauth_provider_config.client_id IS 'OAuth client ID from provider';
COMMENT ON COLUMN oauth_provider_config.client_secret IS 'OAuth client secret from provider';
COMMENT ON COLUMN oauth_provider_config.scopes IS 'OAuth scopes to request';
COMMENT ON COLUMN oauth_provider_config.enabled IS 'Whether this provider is enabled';

-- ====================================
-- General Configuration Table
-- ====================================

-- Realm generic configuration table (supports multiple config types)
CREATE TABLE realm_config (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    realm_id text NOT NULL,
    config_type text NOT NULL,
    config_key text NOT NULL,
    config_value text NOT NULL,
    is_secret boolean NOT NULL DEFAULT false,
    enabled boolean NOT NULL DEFAULT true,
    metadata jsonb DEFAULT '{}',
    created_at timestamp with time zone DEFAULT now(),
    updated_at timestamptz DEFAULT now()
);

CREATE UNIQUE INDEX realm_config_realm_type_key_idx ON realm_config(realm_id, config_type, config_key);
CREATE INDEX realm_config_realm_idx ON realm_config(realm_id);
CREATE INDEX realm_config_type_idx ON realm_config(config_type);
CREATE INDEX realm_config_type_key_idx ON realm_config(config_type, config_key);
COMMENT ON TABLE realm_config IS 'Realm generic configuration table, supports multiple config types';
COMMENT ON COLUMN realm_config.config_type IS 'Configuration type: oauth, turnstile, email, sms, etc';
COMMENT ON COLUMN realm_config.config_key IS 'Configuration key, e.g., "provider_id" for oauth, "secret" for turnstile';
COMMENT ON COLUMN realm_config.is_secret IS 'Whether this is sensitive information (should be encrypted)';
COMMENT ON COLUMN realm_config.metadata IS 'Additional JSON metadata for structured configuration';

-- ====================================
-- Email Tables
-- ====================================

-- Email templates
CREATE TABLE email_templates (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    realm_id text,
    type text NOT NULL,
    content text NOT NULL,
    created_at timestamptz DEFAULT now(),
    updated_at timestamp DEFAULT now()
);

-- One effective template per scope and type keeps locale/realm fallback deterministic.
CREATE UNIQUE INDEX uq_email_templates_realm_type
    ON email_templates (realm_id, type)
    WHERE realm_id IS NOT NULL;
CREATE UNIQUE INDEX uq_email_templates_global_type
    ON email_templates (type)
    WHERE realm_id IS NULL;

COMMENT ON TABLE email_templates IS 'Email templates for different types of emails';
COMMENT ON COLUMN email_templates.type IS
    'Template kind with optional locale suffix, e.g. verify_email or verify_email:zh-CN';
COMMENT ON COLUMN email_templates.content IS
    'JSON object with subject, text and html fields; allowed variables: brand_name and action_url';

-- Email verification codes
CREATE TABLE email_verification_code (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    email text NOT NULL,
    type text NOT NULL,
    verification_code text NOT NULL,
    created_at timestamptz DEFAULT now()
);

COMMENT ON TABLE email_verification_code IS 'Verification codes for email confirmation';

-- ====================================
-- Audit Events Table
-- ====================================
-- Append-only audit log for security and compliance.
-- No foreign keys (audit table doesn't reference business tables).
-- No updated_at column (audit events are immutable).

CREATE TABLE audit_events (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    realm_id TEXT NOT NULL,
    category TEXT NOT NULL,
    action TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    actor_type TEXT,
    actor_name TEXT,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    target_name TEXT,
    result TEXT NOT NULL,
    details JSONB,
    ip_address TEXT,
    user_agent TEXT,
    trace_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- List query primary index (realm + time descending)
CREATE INDEX idx_audit_events_realm_created ON audit_events(realm_id, created_at DESC);

-- Filter by event type
CREATE INDEX idx_audit_events_realm_category_action ON audit_events(realm_id, category, action);

-- Filter by actor
CREATE INDEX idx_audit_events_realm_actor ON audit_events(realm_id, actor_id);

-- Filter by IP address (security forensics)
CREATE INDEX idx_audit_events_realm_ip ON audit_events(realm_id, ip_address);

COMMENT ON TABLE audit_events IS 'Append-only audit event log for security and compliance';
COMMENT ON COLUMN audit_events.category IS 'Event category: user_management, rbac, realm_management, auth';
COMMENT ON COLUMN audit_events.action IS 'Specific operation within category';
COMMENT ON COLUMN audit_events.actor_type IS 'Actor classification: user, admin, system';
COMMENT ON COLUMN audit_events.result IS 'Operation result: success, failure';
COMMENT ON COLUMN audit_events.details IS 'Extensible JSONB payload (change diffs, failure reasons, etc.)';

-- ====================================
-- Initial Data
-- ====================================
-- NOTE: RBAC initialization (roles, permissions) should be done via RealmInitializationService
-- when creating realms, not in database migrations.

-- Insert default admin realm
INSERT INTO realm (id, name)
VALUES ('admin', 'Admin');

-- Insert default admin client app
INSERT INTO client_app (id, realm_id, client_id, name, is_first_party)
VALUES (uuidv7(), 'admin', 'admin-web-console', 'Admin Client App', true);

-- Insert default personal account-center client app
INSERT INTO client_app (id, realm_id, client_id, name, is_first_party)
VALUES (uuidv7(), 'admin', 'user-account-center', 'User Account Center', true);

-- Insert built-in API Key Client App for the admin realm
INSERT INTO client_app (id, realm_id, client_id, name, description, enabled, redirect_uris)
VALUES (uuidv7(), 'admin', 'admin-api-client', 'API Key Client', 'Built-in client for API key authentication', true, '[]'::jsonb)
ON CONFLICT (realm_id, client_id) DO NOTHING;
