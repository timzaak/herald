-- webhook-event-push: outbound webhook subscriptions + delivery queue.
-- Mirrors the payment_event sweep/lease pattern (0002) upgraded with
-- per-attempt exponential backoff.

CREATE TABLE webhook_subscription (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    realm_id TEXT NOT NULL,
    endpoint_url TEXT NOT NULL,
    secret TEXT NOT NULL,
    event_categories TEXT[] NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    paused_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_webhook_subscription_realm_url UNIQUE (realm_id, endpoint_url),
    CONSTRAINT chk_webhook_subscription_status
        CHECK (status IN ('active','paused','disabled')),
    CONSTRAINT chk_webhook_subscription_categories
        CHECK (event_categories <@ ARRAY['user_lifecycle','role_events','subscription_lifecycle']
               AND cardinality(event_categories) >= 1)
);
CREATE INDEX idx_webhook_subscription_realm_status
    ON webhook_subscription(realm_id, status);

CREATE TABLE outbound_event (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    realm_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_outbound_event_type CHECK (event_type IN (
        'user.created','user.disabled','user.deleted',
        'role.granted','role.revoked',
        'subscription.activated','subscription.renewed',
        'subscription.expired','subscription.refunded'))
);

CREATE TABLE outbound_delivery (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    event_id UUID NOT NULL REFERENCES outbound_event(id) ON DELETE CASCADE,
    subscription_id UUID NOT NULL REFERENCES webhook_subscription(id) ON DELETE CASCADE,
    realm_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at TIMESTAMPTZ,
    processing_started_at TIMESTAMPTZ,
    last_attempt_at TIMESTAMPTZ,
    last_error TEXT,
    delivered_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_outbound_delivery_status
        CHECK (status IN ('pending','delivered','dropped'))
);
CREATE INDEX idx_outbound_delivery_sweep
    ON outbound_delivery(next_retry_at) WHERE status = 'pending';
CREATE INDEX idx_outbound_delivery_subscription ON outbound_delivery(subscription_id);
CREATE INDEX idx_outbound_delivery_event ON outbound_delivery(event_id);

-- Seed builtin webhooks permissions for every existing realm and attach
-- them to the builtin realm-admin role. rbac_init only runs at realm
-- creation; existing realms never re-initialize, so the migration is the
-- sole delivery path for them.
INSERT INTO permissions (id, name, description, realm_id, resource, action, is_builtin)
SELECT uuidv7(), 'webhooks.view', 'View webhook subscriptions',
       r.id, 'webhooks', 'view', true
FROM realm r
ON CONFLICT (name, realm_id) DO NOTHING;

INSERT INTO permissions (id, name, description, realm_id, resource, action, is_builtin)
SELECT uuidv7(), 'webhooks.manage', 'Manage webhook subscriptions',
       r.id, 'webhooks', 'manage', true
FROM realm r
ON CONFLICT (name, realm_id) DO NOTHING;

INSERT INTO role_permissions (id, role_id, permission_id)
SELECT uuidv7(), ro.id, p.id
FROM roles ro
JOIN permissions p
  ON p.realm_id = ro.realm_id AND p.name IN ('webhooks.view', 'webhooks.manage')
WHERE ro.name = 'realm-admin' AND ro.is_builtin = true
ON CONFLICT (role_id, permission_id) DO NOTHING;
