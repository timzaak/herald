-- ====================================
-- Herald Billing / Points / Invoice Schema (baseline)
-- ====================================
-- Consolidates subscription history, points (wallets, transactions, ledger,
-- allocations, revocations, grant schedules/records, quota entitlements),
-- unified purchase / payment attempts, provider entitlement mappings,
-- idempotency, and invoice/credit-note schema into one baseline.
-- Pre-launch squash: all ALTER/DROP folded into final-state CREATE TABLE.
--   - points_credit_ledger.effective_at + its CHECK + bucket-avail partial
--     index are inline
--   - points_grant_records.ledger_id FK is inline
--   - points_distribution_rules / points_distribution_events /
--     payment_attempt_point_rules and the rule/event attribution columns on
--     points_credit_ledger / points_transactions / points_quota_entitlements /
--     points_grant_schedules are inline (multi-wallet grant rules model)
--   - payment_attempts.target_type CHECK is the FINAL 'entitlement_mapping'
--     form (the obsolete 'subscription_entitlement' / 'points_package'
--     values and the dropped points_packages family never existed)
--   - subscription.billing_type + chk_subscription_billing_type (former 0011)
--     are inline
--   - payment_event.next_retry_at (former 0006) is inline
--   - provider_entitlement_mappings.granted_role_ids (former 0006) and
--     service_duration_days + chk_pem_service_duration_days (former 0011) are
--     inline; chk_pem_billing_type is the FINAL ('recurring','one_time',
--     'non_renewing') value set (former 0011) and chk_pem_payment_provider is
--     the FINAL ('stripe','creem','apple','google','wechat') value set
--     (former 0010 + wechat pay provider)
--   - payment_attempts.is_one_time_role + idx_payment_attempts_one_time_role
--     (former 0006) are inline; chk_payment_attempt_provider is the FINAL
--     ('stripe','creem','apple','google','wechat') value set (former 0010 +
--     wechat pay provider)
--   - payment_event idempotency UNIQUE is realm-scoped (realm_id,
--     external_event_id, payment_provider): two realms sharing one provider
--     account receive the same event id and must both fulfill
--   - idx_points_quota_entitlements_bucket_id is inline (bucket-delete
--     reference probe)
--
-- Available balance is exclusively a derived SUM over points_credit_ledger
-- (same predicate as consumption); there is no stored/derived dual-track.
-- The 4 lifetime analytics columns on points_wallets remain stored. Only
-- points_transactions.balance_after (+typed snapshots) is retained as the
-- real post-mutation derived balance.
-- Tables are ordered by FK dependency (referenced table before referencer).

-- ====================================
-- Credit Buckets
-- ====================================
CREATE TABLE credit_buckets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    realm_id TEXT NOT NULL,
    bucket_key TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    display_order INTEGER NOT NULL DEFAULT 0,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_credit_buckets_realm_key UNIQUE (realm_id, bucket_key),
    CONSTRAINT chk_credit_buckets_key CHECK (bucket_key ~ '^[a-z0-9-]{1,64}$')
);

CREATE INDEX idx_credit_buckets_realm_id ON credit_buckets(realm_id);
CREATE INDEX idx_credit_buckets_enabled ON credit_buckets(realm_id, enabled);

CREATE TABLE credit_bucket_client_apps (
    bucket_id UUID NOT NULL REFERENCES credit_buckets(id) ON DELETE CASCADE,
    client_app_id UUID NOT NULL REFERENCES client_app(id) ON DELETE CASCADE,
    realm_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (bucket_id, client_app_id)
);

CREATE INDEX idx_credit_bucket_client_apps_client_app
    ON credit_bucket_client_apps(client_app_id);
CREATE INDEX idx_credit_bucket_client_apps_realm
    ON credit_bucket_client_apps(realm_id);

COMMENT ON TABLE credit_buckets IS 'Realm-level credit bucket catalog for points pool isolation';
COMMENT ON TABLE credit_bucket_client_apps IS 'Many-to-many coverage set from credit buckets to client apps';

-- ====================================
-- Subscription
-- ====================================
CREATE TABLE subscription (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    realm_id TEXT NOT NULL,
    user_id UUID NOT NULL REFERENCES account(id) ON DELETE RESTRICT,
    external_subscription_id TEXT NOT NULL,
    external_product_id TEXT NOT NULL,
    external_price_id TEXT,
    payment_provider text NOT NULL DEFAULT 'creem',
    status text NOT NULL,
    entitlement_key TEXT NOT NULL DEFAULT '',
    provider_metadata JSONB,
    synced_at TIMESTAMPTZ,
    current_period_start TIMESTAMPTZ,
    current_period_end TIMESTAMPTZ,
    cancel_at_period_end BOOLEAN DEFAULT false,
    client_app_id UUID UNIQUE,
    cancel_at TIMESTAMPTZ,
    -- billing_type is the snapshot of the mapping billing type taken at
    -- fulfillment time (DEC-pay_model-007), so reconciliation/views/api-ext can
    -- filter without joining the mapping. Only subscription-shape billing types
    -- land here (recurring / non_renewing); one_time purchases never create a
    -- subscription row. NOT NULL directly (no existing rows; DEC-pay_model-008).
    billing_type TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_subscription_client_app UNIQUE (client_app_id),
    CONSTRAINT chk_subscription_billing_type
        CHECK (billing_type IN ('recurring', 'non_renewing'))
);

CREATE INDEX idx_subscription_realm_id ON subscription(realm_id);
CREATE INDEX idx_subscription_external_provider
    ON subscription(external_subscription_id, payment_provider);
CREATE INDEX idx_subscription_status ON subscription(status);
CREATE INDEX idx_subscription_entitlement_key ON subscription(entitlement_key);
CREATE INDEX idx_subscription_client_app_id ON subscription(client_app_id);
CREATE INDEX idx_subscription_user_id ON subscription(user_id);
CREATE INDEX idx_subscription_realm_user_id ON subscription(realm_id, user_id);
COMMENT ON TABLE subscription IS 'Client app subscriptions mapped to entitlement keys';
COMMENT ON COLUMN subscription.billing_type IS
    'Billing type snapshot from the entitlement mapping at fulfillment time (DEC-pay_model-007): recurring or non_renewing';

-- ====================================
-- Subscription History
-- ====================================
CREATE TABLE subscription_history (
    id TEXT PRIMARY KEY,
    subscription_id UUID NOT NULL,
    event_type text NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    actor TEXT,
    changes JSONB,
    previous_state JSONB,
    new_state JSONB,
    realm_id TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    CONSTRAINT fk_subscription_history_subscription
        FOREIGN KEY (subscription_id) REFERENCES subscription(id) ON DELETE CASCADE
);

CREATE INDEX idx_subscription_history_subscription_id
    ON subscription_history(subscription_id);
CREATE INDEX idx_subscription_history_realm_id
    ON subscription_history(realm_id);
CREATE INDEX idx_subscription_history_timestamp
    ON subscription_history(timestamp);
CREATE INDEX idx_subscription_history_event_type
    ON subscription_history(event_type);
CREATE INDEX idx_subscription_history_realm_timestamp
    ON subscription_history(realm_id, timestamp DESC);

COMMENT ON TABLE subscription_history IS 'Audit trail of subscription changes including upgrades, downgrades, cancellations, and other events';
COMMENT ON COLUMN subscription_history.id IS 'Unique event identifier (UUID v7)';
COMMENT ON COLUMN subscription_history.subscription_id IS 'Reference to the subscription that changed';
COMMENT ON COLUMN subscription_history.event_type IS 'Type of event: created, upgraded, downgraded, canceled, expired, renewed, reactivated, billing_period_changed';
COMMENT ON COLUMN subscription_history.timestamp IS 'When the change occurred';
COMMENT ON COLUMN subscription_history.actor IS 'Who performed the change (user ID, system, or webhook)';
COMMENT ON COLUMN subscription_history.changes IS 'Detailed change information (JSON)';
COMMENT ON COLUMN subscription_history.previous_state IS 'Subscription state before the change (JSON)';
COMMENT ON COLUMN subscription_history.new_state IS 'Subscription state after the change (JSON)';
COMMENT ON COLUMN subscription_history.realm_id IS 'Realm ID for permission isolation';
COMMENT ON COLUMN subscription_history.created_at IS 'When this history record was created';

-- ====================================
-- Payment Event
-- ====================================
CREATE TABLE payment_event (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    realm_id TEXT NOT NULL,
    external_event_id TEXT NOT NULL,
    payment_provider text NOT NULL DEFAULT 'creem',
    event_type text NOT NULL,
    subscription_id UUID,
    payload JSONB,
    processed BOOLEAN DEFAULT false,
    processing_started_at TIMESTAMPTZ,
    next_retry_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT payment_event_unique_external_provider
        UNIQUE (realm_id, external_event_id, payment_provider)
);

CREATE INDEX idx_payment_event_realm_id ON payment_event(realm_id);
CREATE INDEX idx_payment_event_event_type ON payment_event(event_type);
CREATE INDEX idx_payment_event_processed ON payment_event(processed);
CREATE INDEX idx_payment_event_provider ON payment_event(payment_provider);
COMMENT ON TABLE payment_event IS 'Payment events from multiple providers (Creem, Stripe, etc.)';
COMMENT ON COLUMN payment_event.external_event_id IS 'External event ID from payment provider (unique per realm and provider: realms sharing one provider account receive the same event id)';
COMMENT ON COLUMN payment_event.payment_provider IS 'Payment provider type (creem, stripe, etc.)';
COMMENT ON COLUMN payment_event.processing_started_at IS 'When webhook processing last claimed the event for execution; null means idle';
COMMENT ON COLUMN payment_event.next_retry_at IS
    'Backoff-scheduled retry time for the processed=false sweep job (PaymentEventRetryJob). NULL = eligible for immediate retry.';

-- ====================================
-- Points Wallets
-- ====================================
CREATE TABLE points_wallets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    realm_id TEXT NOT NULL,
    bucket_id UUID NOT NULL REFERENCES credit_buckets(id) ON DELETE RESTRICT,
    total_recharged BIGINT NOT NULL DEFAULT 0 CHECK (total_recharged >= 0),
    total_consumed BIGINT NOT NULL DEFAULT 0 CHECK (total_consumed >= 0),
    total_topup_granted BIGINT NOT NULL DEFAULT 0 CHECK (total_topup_granted >= 0),
    total_subscription_granted BIGINT NOT NULL DEFAULT 0 CHECK (total_subscription_granted >= 0),
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'frozen', 'closed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uk_points_wallets_user_bucket UNIQUE (realm_id, user_id, bucket_id)
);

CREATE INDEX idx_points_wallets_user_id ON points_wallets(user_id);
CREATE INDEX idx_points_wallets_realm_id ON points_wallets(realm_id);
CREATE INDEX idx_points_wallets_bucket_id ON points_wallets(bucket_id);
CREATE INDEX idx_points_wallets_status ON points_wallets(status);

COMMENT ON TABLE points_wallets IS 'User-level points wallets tracking lifetime analytics (recharges/consumption). Available balance is derived from points_credit_ledger; no Stored balance columns.';
COMMENT ON COLUMN points_wallets.id IS 'Unique wallet identifier';
COMMENT ON COLUMN points_wallets.user_id IS 'Reference to user who owns this wallet';
COMMENT ON COLUMN points_wallets.realm_id IS 'Realm ID for permission isolation';
COMMENT ON COLUMN points_wallets.bucket_id IS 'Credit bucket this wallet belongs to';
COMMENT ON COLUMN points_wallets.total_recharged IS 'Total points ever recharged (lifetime analytics; paid topup + subscription grants)';
COMMENT ON COLUMN points_wallets.total_consumed IS 'Total points ever consumed (lifetime analytics)';
COMMENT ON COLUMN points_wallets.total_topup_granted IS 'Total purchased points ever granted (lifetime analytics)';
COMMENT ON COLUMN points_wallets.total_subscription_granted IS 'Total subscription points ever granted (lifetime analytics)';
COMMENT ON COLUMN points_wallets.status IS 'Wallet status: active (normal operations), frozen (temporarily disabled), closed (permanently disabled)';

-- ====================================
-- Points Credit Ledger
-- ====================================
-- Source of truth for all points credits. effective_at gates when a grant
-- enters the available set: NULL = immediately available, non-null = enters
-- at/after that time. Consumption selection AND derived balance both gate on
-- (effective_at IS NULL OR effective_at <= NOW()), as does the bucket-avail
-- partial index below.
CREATE TABLE points_credit_ledger (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    realm_id TEXT NOT NULL,
    bucket_id UUID NOT NULL REFERENCES credit_buckets(id) ON DELETE RESTRICT,
    credit_type text NOT NULL CHECK (credit_type IN (
        'topup_credit',
        'subscription_credit',
        'registration_credit',
        'free_periodic_credit',
        'granted_credit'
    )),
    source_type text NOT NULL CHECK (source_type IN (
        'subscription_initial',
        'subscription_renewal',
        'subscription_upgrade',
        'subscription_downgrade',
        'topup',
        'system_grant',
        'registration',
        'free_periodic_grant',
        'admin_grant',
        'sdk_grant'
    )),
    source_id TEXT NOT NULL,
    granted_amount BIGINT NOT NULL CHECK (granted_amount > 0),
    used_amount BIGINT NOT NULL DEFAULT 0 CHECK (used_amount >= 0),
    revoked_amount BIGINT NOT NULL DEFAULT 0 CHECK (revoked_amount >= 0),
    remaining_amount BIGINT NOT NULL GENERATED ALWAYS AS (
        granted_amount - used_amount - revoked_amount
    ) STORED CHECK (remaining_amount >= 0),
    expires_at TIMESTAMPTZ,
    effective_at TIMESTAMPTZ,
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked', 'expired', 'fully_used')),
    -- Distribution attribution (multi-wallet grant rules). Both NULL = direct
    -- write (admin/sdk grant, demo/test-only internal quota); both NOT NULL =
    -- rule-executed grant. FK constraints are added after the referenced
    -- tables are created (see "Distribution attribution constraints").
    distribution_event_id UUID,
    distribution_rule_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT points_credit_ledger_effective_before_expires
        CHECK (effective_at IS NULL OR expires_at IS NULL OR effective_at <= expires_at),
    CONSTRAINT points_credit_ledger_attribution_pair
        CHECK ((distribution_event_id IS NULL) = (distribution_rule_id IS NULL))
);

CREATE INDEX idx_points_credit_ledger_user_id ON points_credit_ledger(user_id);
CREATE INDEX idx_points_credit_ledger_realm_id ON points_credit_ledger(realm_id);
CREATE INDEX idx_points_credit_ledger_bucket_id ON points_credit_ledger(bucket_id);
CREATE INDEX idx_points_credit_ledger_credit_type ON points_credit_ledger(credit_type);
CREATE INDEX idx_points_credit_ledger_status ON points_credit_ledger(status);
CREATE INDEX idx_points_credit_ledger_expires_at ON points_credit_ledger(expires_at);
CREATE INDEX idx_points_credit_ledger_user_credit_status
    ON points_credit_ledger(user_id, credit_type, status);
CREATE INDEX idx_points_credit_ledger_bucket_expiration
    ON points_credit_ledger(realm_id, user_id, bucket_id, expires_at);
CREATE INDEX idx_points_credit_ledger_created_at ON points_credit_ledger(created_at ASC);
-- Partial covering index for the shared predicate of derived SUM
-- (compute_available_balance / compute_bucket_available_balances) and
-- consumption selection. INCLUDE carries the covering columns so both SUM
-- and selection are index-only scans.
CREATE INDEX idx_points_credit_ledger_bucket_avail
    ON points_credit_ledger (realm_id, user_id, bucket_id, status)
    INCLUDE (remaining_amount, effective_at, expires_at, credit_type)
    WHERE status = 'active';

COMMENT ON TABLE points_credit_ledger IS 'Source of truth for all points credits, tracking grants, usage, and revocation by type';
COMMENT ON COLUMN points_credit_ledger.bucket_id IS 'Credit bucket that owns this credit grant';
COMMENT ON COLUMN points_credit_ledger.credit_type IS 'Type of credit: topup_credit, subscription_credit, registration_credit, or free_periodic_credit';
COMMENT ON COLUMN points_credit_ledger.source_type IS 'Source of credit: subscription_initial/renewal/upgrade, topup, system_grant, registration, or free_periodic_grant';
COMMENT ON COLUMN points_credit_ledger.remaining_amount IS 'Computed field: granted_amount - used_amount - revoked_amount';
COMMENT ON COLUMN points_credit_ledger.effective_at IS 'Expected effective time; NULL = immediately available, non-null = enters available set only at/after this time (consumption selection + derived balance predicate gating)';
COMMENT ON COLUMN points_credit_ledger.distribution_event_id IS 'Distribution event that produced this credit row; NULL for direct admin/sdk/internal writes (paired with distribution_rule_id)';
COMMENT ON COLUMN points_credit_ledger.distribution_rule_id IS 'Distribution rule that produced this credit row; NULL for direct admin/sdk/internal writes (paired with distribution_event_id)';
-- At most one attribution-bearing ledger row per (event, rule).
CREATE UNIQUE INDEX idx_points_credit_ledger_event_rule
    ON points_credit_ledger (distribution_event_id, distribution_rule_id)
    WHERE distribution_rule_id IS NOT NULL;

-- ====================================
-- Points Transactions
-- ====================================
CREATE TABLE points_transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    wallet_id UUID NOT NULL REFERENCES points_wallets(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    realm_id TEXT NOT NULL,
    bucket_id UUID NOT NULL REFERENCES credit_buckets(id) ON DELETE RESTRICT,
    type text NOT NULL CHECK (type IN (
        'recharge',
        'consume',
        'subscription_grant',
        'subscription_renewal',
        'subscription_upgrade',
        'subscription_downgrade',
        'registration_grant',
        'free_periodic_grant',
        'refund_revoke',
        'expire_revoke',
        'cancel_revoke',
        'idempotency_record',
        'expiration',
        'refund',
        'grant'
    )),
    amount BIGINT NOT NULL,
    balance_after BIGINT NOT NULL CHECK (balance_after >= 0),
    topup_balance_after BIGINT CHECK (topup_balance_after >= 0),
    subscription_balance_after BIGINT CHECK (subscription_balance_after >= 0),
    credit_type text CHECK (credit_type IN (
        'topup_credit',
        'subscription_credit',
        'registration_credit',
        'free_periodic_credit',
        'granted_credit'
    )),
    description TEXT,
    client_app_id UUID REFERENCES client_app(id) ON DELETE SET NULL,
    subscription_id UUID REFERENCES subscription(id) ON DELETE SET NULL,
    external_ref_id TEXT,
    correlation_id TEXT,
    expires_at TIMESTAMPTZ,
    -- Distribution attribution (see points_credit_ledger pair rule).
    distribution_event_id UUID,
    distribution_rule_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT points_transactions_attribution_pair
        CHECK ((distribution_event_id IS NULL) = (distribution_rule_id IS NULL))
);

CREATE INDEX idx_points_transactions_wallet_id ON points_transactions(wallet_id);
CREATE INDEX idx_points_transactions_user_id ON points_transactions(user_id);
CREATE INDEX idx_points_transactions_realm_id ON points_transactions(realm_id);
CREATE INDEX idx_points_transactions_bucket_id ON points_transactions(bucket_id);
CREATE INDEX idx_points_transactions_type ON points_transactions(type);
CREATE INDEX idx_points_transactions_created_at ON points_transactions(created_at DESC);
CREATE INDEX idx_points_transactions_client_app_id ON points_transactions(client_app_id);
CREATE INDEX idx_points_transactions_subscription_id ON points_transactions(subscription_id);
CREATE INDEX idx_points_transactions_realm_created ON points_transactions(realm_id, created_at DESC);
CREATE INDEX idx_points_transactions_user_created ON points_transactions(user_id, created_at DESC);
CREATE INDEX idx_points_transactions_expires_at
    ON points_transactions(expires_at)
    WHERE expires_at IS NOT NULL;
CREATE UNIQUE INDEX idx_transactions_external_ref
    ON points_transactions(user_id, external_ref_id)
    WHERE external_ref_id IS NOT NULL;
CREATE INDEX idx_transactions_correlation_id
    ON points_transactions(correlation_id)
    WHERE correlation_id IS NOT NULL;
-- Window-aggregation covering index: existing indexes do not filter by
-- credit_type, so window SUM(amount) WHERE type='consume' could not use an
-- index range scan by credit_type. This partial covering index makes the
-- window aggregation an index-only scan.
CREATE INDEX idx_points_transactions_window_agg
    ON points_transactions (user_id, bucket_id, credit_type, created_at DESC)
    INCLUDE (amount)
    WHERE type = 'consume';

COMMENT ON TABLE points_transactions IS 'Transaction history for all points movements';
COMMENT ON COLUMN points_transactions.bucket_id IS 'Credit bucket this transaction belongs to; cross-bucket consumption writes one row per bucket (always NOT NULL, see A6)';
COMMENT ON COLUMN points_transactions.credit_type IS 'Type of credit affected: topup_credit, subscription_credit, registration_credit, or free_periodic_credit';
COMMENT ON COLUMN points_transactions.type IS 'Transaction type for recharge, consumption, grant, revocation, expiration, refund, and idempotency records';
COMMENT ON COLUMN points_transactions.topup_balance_after IS 'Topup credit balance after this transaction';
COMMENT ON COLUMN points_transactions.subscription_balance_after IS 'Subscription credit balance after this transaction';
COMMENT ON COLUMN points_transactions.correlation_id IS 'Cross-bucket consumption grouping key (nullable, non-unique); shared by the N transactions of a single multi-bucket consume. external_ref_id remains unique and is not used for grouping';
COMMENT ON COLUMN points_transactions.expires_at IS 'Expiration time for time-limited points (NULL = permanent points)';
COMMENT ON COLUMN points_transactions.updated_at IS 'When transaction was last updated';
COMMENT ON INDEX idx_transactions_external_ref IS 'Idempotency constraint for webhook event processing based on user_id + external_ref_id';

-- ====================================
-- Points Consumption Allocations
-- ====================================
CREATE TABLE points_consumption_allocations (
    id UUID PRIMARY KEY,
    transaction_id UUID NOT NULL,
    ledger_id UUID NOT NULL,
    wallet_id UUID NOT NULL REFERENCES points_wallets(id) ON DELETE CASCADE,
    user_id UUID NOT NULL,
    realm_id TEXT NOT NULL,
    bucket_id UUID NOT NULL REFERENCES credit_buckets(id) ON DELETE RESTRICT,
    allocated_amount BIGINT NOT NULL CHECK (allocated_amount > 0),
    ledger_remaining_after BIGINT NOT NULL CHECK (ledger_remaining_after >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_points_consumption_allocations_transaction_id
    ON points_consumption_allocations(transaction_id);
CREATE INDEX idx_points_consumption_allocations_ledger_id
    ON points_consumption_allocations(ledger_id);
CREATE INDEX idx_points_consumption_allocations_wallet_id
    ON points_consumption_allocations(wallet_id);
CREATE INDEX idx_points_consumption_allocations_user_id
    ON points_consumption_allocations(user_id);
CREATE INDEX idx_points_consumption_allocations_realm_id
    ON points_consumption_allocations(realm_id);
CREATE INDEX idx_points_consumption_allocations_bucket_id
    ON points_consumption_allocations(bucket_id);

COMMENT ON TABLE points_consumption_allocations IS 'Allocation records showing how each consumption transaction splits across ledger entries';

-- ====================================
-- Points Revocation Records
-- ====================================
CREATE TABLE points_revocation_records (
    id UUID PRIMARY KEY,
    ledger_id UUID NOT NULL,
    user_id UUID NOT NULL,
    realm_id TEXT NOT NULL,
    revocation_type text NOT NULL CHECK (revocation_type IN (
        'refund_revoke',
        'expire_revoke',
        'cancel_revoke',
        'upgrade_revoke'
    )),
    revoked_amount BIGINT NOT NULL CHECK (revoked_amount > 0),
    reason TEXT NOT NULL,
    reference_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_points_revocation_records_ledger_id
    ON points_revocation_records(ledger_id);
CREATE INDEX idx_points_revocation_records_user_id
    ON points_revocation_records(user_id);
CREATE INDEX idx_points_revocation_records_revocation_type
    ON points_revocation_records(revocation_type);
CREATE INDEX idx_points_revocation_records_realm_id
    ON points_revocation_records(realm_id);

COMMENT ON TABLE points_revocation_records IS 'Records of all points revocation operations';
COMMENT ON COLUMN points_revocation_records.revocation_type IS 'Type of revocation: refund_revoke, expire_revoke, cancel_revoke, or upgrade_revoke';

-- ====================================
-- Idempotency Keys
-- ====================================
CREATE TABLE idempotency_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    realm_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processing', 'completed', 'failed')),
    request_data JSONB NOT NULL DEFAULT '{}',
    response_data JSONB,
    transaction_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX uk_idempotency_keys ON idempotency_keys(realm_id, idempotency_key);
CREATE INDEX idx_idempotency_expires ON idempotency_keys(expires_at);
CREATE INDEX idx_idempotency_transaction ON idempotency_keys(transaction_id);

COMMENT ON TABLE idempotency_keys IS 'Idempotency keys for points consumption to prevent duplicate charges';

CREATE TABLE points_grant_schedules (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    realm_id TEXT NOT NULL,
    bucket_id UUID NOT NULL REFERENCES credit_buckets(id) ON DELETE RESTRICT,
    subscription_id UUID,
    entitlement_key TEXT NOT NULL DEFAULT '',
    grant_period_type text NOT NULL CHECK (grant_period_type IN ('once', 'daily', 'weekly', 'monthly')),
    base_time TIMESTAMPTZ NOT NULL,
    next_grant_time TIMESTAMPTZ NOT NULL,
    points_per_period BIGINT NOT NULL CHECK (points_per_period >= 0),
    validity_days BIGINT NOT NULL CHECK (validity_days >= 0),
    granted_periods BIGINT NOT NULL DEFAULT 0 CHECK (granted_periods >= 0),
    max_periods BIGINT CHECK (max_periods > 0),
    active BOOLEAN NOT NULL DEFAULT TRUE,
    -- A schedule is always created by a free-periodic fixed distribution rule;
    -- both references are NOT NULL. FK constraints added after referenced tables.
    distribution_event_id UUID NOT NULL,
    distribution_rule_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_points_grant_schedules_user_rule
        UNIQUE (realm_id, user_id, distribution_rule_id)
);

COMMENT ON TABLE points_grant_schedules IS 'Automatic points granting schedules for free users and subscriptions';
COMMENT ON COLUMN points_grant_schedules.distribution_event_id IS 'Distribution event that created this schedule (free-periodic fixed rule first execution)';
COMMENT ON COLUMN points_grant_schedules.distribution_rule_id IS 'Distribution rule this schedule fulfils; one schedule per user per free-periodic fixed rule';

CREATE INDEX idx_points_grant_schedules_next_grant_time
    ON points_grant_schedules(next_grant_time)
    WHERE active = TRUE;
CREATE INDEX idx_points_grant_schedules_user_id
    ON points_grant_schedules(user_id);
CREATE INDEX idx_points_grant_schedules_bucket_id
    ON points_grant_schedules(bucket_id);
CREATE INDEX idx_points_grant_schedules_subscription_id
    ON points_grant_schedules(subscription_id);

COMMENT ON TABLE points_grant_schedules IS 'Automatic points granting schedules for free users and subscriptions';

CREATE TABLE points_grant_records (
    id UUID PRIMARY KEY,
    schedule_id UUID NOT NULL,
    user_id UUID NOT NULL,
    realm_id TEXT NOT NULL,
    -- Bridges the (schedule_id, period_number) idempotency key (which lives only
    -- in points_grant_records) to a unique ledger row in points_credit_ledger
    -- (which has no schedule_id/period_number columns), for row-level reclaim.
    -- NOT NULL: all writes come from pregrant_next_period_atomic which inserts
    -- the ledger row first, fetches its id, then writes this record.
    ledger_id UUID NOT NULL REFERENCES points_credit_ledger(id) ON DELETE RESTRICT,
    period_number BIGINT NOT NULL CHECK (period_number > 0),
    granted_amount BIGINT NOT NULL CHECK (granted_amount > 0),
    grant_time TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX uk_points_grant_records_schedule_period
    ON points_grant_records(schedule_id, period_number);
CREATE INDEX idx_points_grant_records_user_id
    ON points_grant_records(user_id);
CREATE INDEX idx_points_grant_records_grant_time
    ON points_grant_records(grant_time);

COMMENT ON TABLE points_grant_records IS 'History of points grants for each schedule';
COMMENT ON COLUMN points_grant_records.ledger_id IS 'FK to points_credit_ledger(id); bridges (schedule_id, period_number) idempotency key to a unique ledger row for row-level reclaim positioning';

-- ====================================
-- Points Quota Entitlements
-- ====================================
-- Window-based quota entitlement (replaces per-period ledger issuance for
-- subscription_credit / free_periodic_credit). quota_windows is a snapshot
-- [{windowSeconds, limit, key}] captured at grant time. Idempotency via
-- UNIQUE(realm_id, user_id, bucket_id, credit_type, idempotency_key) keyed
-- by subscription period / webhook event.
CREATE TABLE points_quota_entitlements (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    realm_id TEXT NOT NULL,
    bucket_id UUID NOT NULL REFERENCES credit_buckets(id) ON DELETE RESTRICT,
    credit_type TEXT NOT NULL CHECK (credit_type IN ('subscription_credit', 'free_periodic_credit')),
    source_type TEXT NOT NULL CHECK (source_type IN ('subscription_initial', 'subscription_renewal', 'subscription_upgrade', 'free_periodic_grant')),
    source_id TEXT NOT NULL,
    quota_windows JSONB NOT NULL,
    effective_from TIMESTAMPTZ NOT NULL,
    effective_until TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked', 'expired')),
    idempotency_key TEXT NOT NULL,
    -- Distribution attribution (see points_credit_ledger pair rule).
    distribution_event_id UUID,
    distribution_rule_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT points_quota_entitlements_attribution_pair
        CHECK ((distribution_event_id IS NULL) = (distribution_rule_id IS NULL))
);

-- Direct-write rows (demo/test-only internal quota, DEC-013) keep the legacy
-- (realm, user, bucket, credit_type, idempotency_key) idempotency uniqueness;
-- rule-attributed rows are deduplicated by (event, rule) instead.
CREATE UNIQUE INDEX uq_points_quota_entitlements_idem
    ON points_quota_entitlements (realm_id, user_id, bucket_id, credit_type, idempotency_key)
    WHERE distribution_rule_id IS NULL;
-- At most one rule-attributed entitlement per (event, rule).
CREATE UNIQUE INDEX idx_points_quota_entitlements_event_rule
    ON points_quota_entitlements (distribution_event_id, distribution_rule_id)
    WHERE distribution_rule_id IS NOT NULL;

-- Consumption / balance read path: locate active entitlements for (user, bucket, credit_type).
CREATE INDEX idx_points_quota_entitlements_user_bucket_type_status
    ON points_quota_entitlements (user_id, bucket_id, credit_type, status);

-- Expiration sweep: active rows whose effective_until has passed.
CREATE INDEX idx_points_quota_entitlements_effective_until_active
    ON points_quota_entitlements (effective_until)
    WHERE status = 'active';

-- Bucket-delete reference probe: delete_credit_bucket checks EXISTS on
-- bucket_id here (alongside points_distribution_rules); every other points_*
-- table referencing a bucket carries a bucket_id index, without which this
-- probe degrades to a sequential scan.
CREATE INDEX idx_points_quota_entitlements_bucket_id
    ON points_quota_entitlements(bucket_id);

COMMENT ON TABLE points_quota_entitlements IS 'Window-based quota entitlements for subscription_credit / free_periodic_credit (replaces per-period ledger issuance)';
COMMENT ON COLUMN points_quota_entitlements.quota_windows IS 'Snapshot of [{windowSeconds, limit, key}] captured at grant time (A2)';
COMMENT ON COLUMN points_quota_entitlements.source_id IS 'subscription_id or registration/free source identifier';
COMMENT ON COLUMN points_quota_entitlements.idempotency_key IS 'Business idempotency key (subscription period / webhook event)';
COMMENT ON COLUMN points_quota_entitlements.distribution_event_id IS 'Distribution event that produced this entitlement; NULL for direct internal quota writes (paired with distribution_rule_id)';
COMMENT ON COLUMN points_quota_entitlements.distribution_rule_id IS 'Distribution rule that produced this entitlement; NULL for direct internal quota writes (paired with distribution_event_id)';

-- ====================================
-- Provider Entitlement Mappings
-- ====================================
-- Maps payment provider products to Herald entitlement keys with points
-- strategy config. quota_windows non-NULL switches grant to the window model.
CREATE TABLE provider_entitlement_mappings (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    realm_id TEXT NOT NULL,
    payment_provider TEXT NOT NULL,
    external_product_id TEXT NOT NULL,
    external_price_id TEXT,
    entitlement_key TEXT NOT NULL,
    billing_type TEXT,
    billing_period TEXT,
    enabled BOOLEAN NOT NULL DEFAULT false,
    provider_product_info JSONB,
    synced_at TIMESTAMPTZ,
    granted_role_ids UUID[] NOT NULL DEFAULT '{}'::uuid[],
    service_duration_days INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_pem_realm_provider_product_price UNIQUE NULLS NOT DISTINCT (realm_id, payment_provider, external_product_id, external_price_id),
    CONSTRAINT chk_pem_entitlement_key CHECK (entitlement_key ~ '^[a-z0-9-]{1,64}$'),
    CONSTRAINT chk_pem_billing_type CHECK (billing_type IS NULL OR billing_type IN ('recurring', 'one_time', 'non_renewing')),
    CONSTRAINT chk_pem_payment_provider CHECK (payment_provider IN ('stripe', 'creem', 'apple', 'google', 'wechat')),
    CONSTRAINT chk_pem_service_duration_days
        CHECK (
            (billing_type IS DISTINCT FROM 'non_renewing')
            OR (service_duration_days IS NOT NULL AND service_duration_days >= 1)
        )
);

CREATE INDEX idx_pem_realm_id ON provider_entitlement_mappings(realm_id);
CREATE INDEX idx_pem_realm_provider ON provider_entitlement_mappings(realm_id, payment_provider);
CREATE INDEX idx_pem_entitlement_key ON provider_entitlement_mappings(entitlement_key);

COMMENT ON TABLE provider_entitlement_mappings IS 'Maps payment provider products to Herald entitlement keys; points distribution is configured via points_distribution_rules';
COMMENT ON COLUMN provider_entitlement_mappings.entitlement_key IS 'Herald entitlement identifier, matching [a-z0-9-]{1,64}';
COMMENT ON COLUMN provider_entitlement_mappings.billing_type IS 'recurring, one_time or non_renewing';
COMMENT ON COLUMN provider_entitlement_mappings.payment_provider IS 'Payment provider: stripe, creem, apple, google, wechat';
COMMENT ON COLUMN provider_entitlement_mappings.provider_product_info IS 'Cached provider product info (name, price, currency, etc.)';
COMMENT ON COLUMN provider_entitlement_mappings.granted_role_ids IS
    'Role IDs auto-granted on payment success (paywall). Empty = no role grant.';
COMMENT ON COLUMN provider_entitlement_mappings.service_duration_days IS
    'Fixed service-period length in days; required (>=1) when billing_type = non_renewing, NULL otherwise (DEC-pay_model-005)';

-- ====================================
-- Payment Attempts
-- ====================================
-- Unified payment attempt tracking for initiator-based payment platforms
-- (Stripe, Creem). target_type is constrained to 'entitlement_mapping'
-- (the only valid purchasable target in the current model).
CREATE TABLE payment_attempts (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    realm_id text NOT NULL,
    user_id uuid NOT NULL,
    payment_provider text NOT NULL,
    target_type text NOT NULL,
    target_id uuid NOT NULL,
    amount bigint NOT NULL CHECK(amount > 0),
    currency text NOT NULL,
    status text NOT NULL,
    provider_reference text,
    provider_status text,
    metadata jsonb,
    expires_at timestamptz NOT NULL,
    completed_at timestamptz,
    is_one_time_role BOOLEAN NOT NULL DEFAULT FALSE,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT chk_payment_attempt_provider CHECK (payment_provider IN ('stripe', 'creem', 'apple', 'google', 'wechat')),
    CONSTRAINT chk_target_type CHECK (target_type = 'entitlement_mapping'),
    CONSTRAINT chk_status CHECK (status IN ('Pending', 'RequiresAction', 'Succeeded', 'Failed', 'Cancelled', 'Expired'))
);

-- Index for querying user's payment attempts (most recent first)
CREATE INDEX idx_payment_attempts_user ON payment_attempts(user_id, created_at DESC);

-- Index for finding expired pending attempts
CREATE INDEX idx_payment_attempts_status_expires ON payment_attempts(status, expires_at);

-- Index for looking up attempts by provider reference (for webhooks)
CREATE INDEX idx_payment_attempts_provider_reference ON payment_attempts(payment_provider, provider_reference);

-- Anti-repeat DB guard: marks attempts subject to the "one purchase per user"
-- rule (billing_type=one_time + non-empty granted_role_ids). The partial unique
-- index on (user_id, target_id) WHERE status='Succeeded' AND is_one_time_role=TRUE
-- closes the concurrent double-purchase window that the application-layer
-- pre-check (purchase_service.rs) cannot fully prevent.
CREATE UNIQUE INDEX idx_payment_attempts_one_time_role
    ON payment_attempts(user_id, target_id)
    WHERE status = 'Succeeded' AND is_one_time_role = TRUE;

COMMENT ON TABLE payment_attempts IS 'Unified payment attempt tracking for initiator-based payment platforms';
COMMENT ON COLUMN payment_attempts.target_type IS 'Type of purchasable target: entitlement_mapping';
COMMENT ON COLUMN payment_attempts.target_id IS 'ID of the provider entitlement mapping being purchased';
COMMENT ON COLUMN payment_attempts.is_one_time_role IS
    'TRUE only for one_time + role entitlement mappings (anti-repeat). '
    'Gates the partial unique index that prevents concurrent double-purchase.';
COMMENT ON COLUMN payment_attempts.provider_reference IS 'Platform-specific order reference (session ID for Stripe)';
COMMENT ON COLUMN payment_attempts.provider_status IS 'Raw status from payment platform';
COMMENT ON COLUMN payment_attempts.expires_at IS 'Payment attempt expiration time (2 hours after creation)';
COMMENT ON COLUMN payment_attempts.completed_at IS 'Time when payment was completed (succeeded or failed)';

-- ====================================
-- Invoice Seller Config
-- ====================================
-- Realm-level seller configuration, auto-filled when creating invoices.
CREATE TABLE invoice_seller_config (
    realm_id TEXT PRIMARY KEY REFERENCES realm(id) ON DELETE CASCADE,
    seller_name TEXT NOT NULL,
    seller_address TEXT NOT NULL CHECK (BTRIM(seller_address) <> ''),
    seller_email TEXT,
    seller_phone TEXT,
    seller_tax_id TEXT NOT NULL,
    default_payment_terms TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE invoice_seller_config IS 'Realm-level seller configuration for invoice creation';
COMMENT ON COLUMN invoice_seller_config.seller_name IS 'Seller legal name';
COMMENT ON COLUMN invoice_seller_config.default_payment_terms IS 'Default payment terms text applied to new invoices';

-- ====================================
-- Invoice
-- ====================================
CREATE TABLE invoice (
    id UUID PRIMARY KEY,
    realm_id TEXT NOT NULL REFERENCES realm(id) ON DELETE CASCADE,
    invoice_number TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('admin_manual', 'user_application', 'external_sync')),
    provider VARCHAR(20) NOT NULL DEFAULT 'manual',
    payment_provider VARCHAR(20),
    account_id UUID,
    applicant_user_id UUID,
    subscription_id UUID REFERENCES subscription(id) ON DELETE SET NULL,
    payment_attempt_id UUID,
    status TEXT NOT NULL CHECK (status IN ('draft', 'issued', 'paid', 'void', 'overdue')),
    currency text NOT NULL,

    -- Dates
    issue_date DATE,
    due_date DATE,
    issued_at TIMESTAMPTZ,
    paid_at TIMESTAMPTZ,
    voided_at TIMESTAMPTZ,

    -- Monetary amounts in smallest currency unit (e.g. CNY cents)
    subtotal BIGINT NOT NULL DEFAULT 0 CHECK (subtotal >= 0),
    discount_amount BIGINT NOT NULL DEFAULT 0,
    tax_amount BIGINT NOT NULL DEFAULT 0,
    shipping_amount BIGINT NOT NULL DEFAULT 0,
    total BIGINT NOT NULL DEFAULT 0 CHECK (total >= 0 OR provider != 'manual'),

    -- Cached refund aggregates maintained in the same transaction as credit_note rows.
    -- amount_refunded = SUM(credit_note.amount WHERE status='active') for this invoice.
    -- amount_remaining = total - amount_refunded.
    amount_refunded BIGINT NOT NULL DEFAULT 0 CHECK (amount_refunded >= 0),
    amount_remaining BIGINT NOT NULL DEFAULT 0 CHECK (amount_remaining >= 0),

    -- Discount/tax/shipping mode and raw input value
    discount_mode TEXT CHECK (discount_mode IN ('fixed', 'percent')),
    discount_value NUMERIC(12, 4),
    tax_mode TEXT CHECK (tax_mode IN ('fixed', 'percent')),
    tax_value NUMERIC(12, 4),
    shipping_mode TEXT CHECK (shipping_mode IN ('fixed')),
    shipping_value NUMERIC(12, 4),

    -- Buyer info
    billing_name TEXT,
    billing_address TEXT CHECK (billing_address IS NULL OR BTRIM(billing_address) <> ''),
    billing_email TEXT,
    billing_phone TEXT,
    billing_tax_id TEXT,

    -- Seller info (snapshot at creation time)
    seller_name TEXT,
    seller_address TEXT CHECK (seller_address IS NULL OR BTRIM(seller_address) <> ''),
    seller_email TEXT,
    seller_phone TEXT,
    seller_tax_id TEXT,

    -- External invoice data
    external_invoice_id TEXT,
    external_order_id TEXT,
    external_status TEXT,
    external_hosted_url TEXT,
    external_pdf_url TEXT,
    external_payload JSONB,
    tax_details JSONB,

    -- Additional fields
    notes TEXT,
    payment_terms TEXT,
    void_reason TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT uk_invoice_realm_number UNIQUE (realm_id, invoice_number)
);

CREATE INDEX idx_invoice_realm_status ON invoice(realm_id, status);
CREATE INDEX idx_invoice_realm_account ON invoice(realm_id, account_id);
CREATE INDEX idx_invoice_realm_created ON invoice(realm_id, created_at DESC);
CREATE UNIQUE INDEX uk_invoice_realm_external_id ON invoice(realm_id, external_invoice_id) WHERE external_invoice_id IS NOT NULL;
CREATE UNIQUE INDEX uk_invoice_realm_external_order_id ON invoice(realm_id, external_order_id) WHERE external_order_id IS NOT NULL;
CREATE INDEX idx_invoice_realm_provider ON invoice(realm_id, provider);

COMMENT ON TABLE invoice IS 'Invoice records with buyer/seller snapshots and monetary amounts';
COMMENT ON COLUMN invoice.invoice_number IS 'Formatted as INV-{YEAR}-{SEQ}';
COMMENT ON COLUMN invoice.source IS 'admin_manual = created by realm admin; user_application = applied by end user; external_sync = synced from external platform';
COMMENT ON COLUMN invoice.subtotal IS 'Sum of all line item subtotals in smallest currency unit';
COMMENT ON COLUMN invoice.total IS 'subtotal - discount_amount + tax_amount + shipping_amount';
COMMENT ON COLUMN invoice.amount_refunded IS 'Accumulated refund amount in smallest currency unit (cached from credit_note)';
COMMENT ON COLUMN invoice.amount_remaining IS 'Remaining payable amount in smallest currency unit (= total - amount_refunded)';
COMMENT ON COLUMN invoice.discount_mode IS 'fixed = flat amount in currency unit; percent = percentage of subtotal';
COMMENT ON COLUMN invoice.provider IS 'Invoice source provider: manual, stripe, creem, wechat, shopify';
COMMENT ON COLUMN invoice.payment_provider IS 'Actual payment platform that collected payment';
COMMENT ON COLUMN invoice.external_invoice_id IS 'External invoice ID (e.g. Stripe invoice ID)';
COMMENT ON COLUMN invoice.external_order_id IS 'External order ID (e.g. Creem order ID)';
COMMENT ON COLUMN invoice.external_status IS 'Raw status from external platform';
COMMENT ON COLUMN invoice.external_hosted_url IS 'External hosted page URL';
COMMENT ON COLUMN invoice.external_pdf_url IS 'External PDF download URL';
COMMENT ON COLUMN invoice.external_payload IS 'Raw external invoice data snapshot (debug only)';
COMMENT ON COLUMN invoice.tax_details IS 'External tax details (e.g. from Creem MoR)';

-- ====================================
-- Invoice Line Item
-- ====================================
CREATE TABLE invoice_line_item (
    id UUID PRIMARY KEY,
    invoice_id UUID NOT NULL REFERENCES invoice(id) ON DELETE CASCADE,
    sort_order INT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    quantity NUMERIC(12, 3) NOT NULL,
    unit_price BIGINT NOT NULL,
    subtotal BIGINT NOT NULL
);

CREATE INDEX idx_invoice_line_item_invoice_sort ON invoice_line_item(invoice_id, sort_order);

COMMENT ON TABLE invoice_line_item IS 'Individual line items within an invoice';
COMMENT ON COLUMN invoice_line_item.subtotal IS 'Server-computed: round(quantity * unit_price)';
COMMENT ON COLUMN invoice_line_item.unit_price IS 'Unit price in smallest currency unit';

-- ====================================
-- Invoice History
-- ====================================
CREATE TABLE invoice_history (
    id UUID PRIMARY KEY,
    invoice_id UUID NOT NULL REFERENCES invoice(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL CHECK (event_type IN ('created', 'updated', 'issued', 'paid', 'voided', 'overdue', 'credit_note_created', 'credit_note_voided')),
    actor_user_id UUID,
    actor_type TEXT NOT NULL CHECK (actor_type IN ('user', 'system')),
    changes JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_invoice_history_invoice_created ON invoice_history(invoice_id, created_at);

COMMENT ON TABLE invoice_history IS 'Audit trail of invoice status transitions and field changes';
COMMENT ON COLUMN invoice_history.event_type IS 'Type of event: created, updated, issued, paid, voided, overdue, credit_note_created, credit_note_voided';
COMMENT ON COLUMN invoice_history.actor_type IS 'user = human action; system = automated job';
COMMENT ON COLUMN invoice_history.changes IS 'Change summary, e.g. {"field": "status", "from": "draft", "to": "issued"}';

-- ====================================
-- Credit Note
-- ====================================
-- Single table for both Stripe (passive sync) and Manual (admin created) credit notes.
-- source distinguishes the origin; source-specific fields are nullable.
-- status tracks active vs voided (voided refunds are reversed on the parent invoice).
CREATE TABLE credit_note (
    id UUID PRIMARY KEY,
    invoice_id UUID NOT NULL REFERENCES invoice(id) ON DELETE CASCADE,
    realm_id TEXT NOT NULL REFERENCES realm(id) ON DELETE CASCADE,
    amount BIGINT NOT NULL CHECK (amount > 0),
    currency TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('stripe', 'manual')),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'voided')),
    external_credit_note_id TEXT,
    memo TEXT,
    created_by_user_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_credit_note_invoice_id ON credit_note(invoice_id);
CREATE INDEX idx_credit_note_realm_id ON credit_note(realm_id);
CREATE UNIQUE INDEX uk_credit_note_external_id ON credit_note(external_credit_note_id) WHERE external_credit_note_id IS NOT NULL;

COMMENT ON TABLE credit_note IS 'Credit notes recording refunds against an invoice (Stripe sync or manual entry)';
COMMENT ON COLUMN credit_note.invoice_id IS 'Invoice this credit note refunds';
COMMENT ON COLUMN credit_note.realm_id IS 'Realm isolation key';
COMMENT ON COLUMN credit_note.amount IS 'Refund amount in smallest currency unit (must be positive)';
COMMENT ON COLUMN credit_note.currency IS 'Currency code, matches the invoice currency';
COMMENT ON COLUMN credit_note.source IS 'stripe = synced from Stripe credit_note.created webhook; manual = recorded by realm admin';
COMMENT ON COLUMN credit_note.status IS 'Lifecycle: active = applies to invoice; voided = reversed (credit_note.voided webhook or admin void)';
COMMENT ON COLUMN credit_note.external_credit_note_id IS 'Stripe Credit Note ID (idempotency key, only for source=stripe)';
COMMENT ON COLUMN credit_note.memo IS 'Manual refund reason (only for source=manual)';
COMMENT ON COLUMN credit_note.created_by_user_id IS 'Operator who created the manual credit note (only for source=manual)';

-- ====================================
-- Invoice Number Counter
-- ====================================
-- Provides transaction-safe sequential invoice numbering per realm+year.
-- Uses SELECT FOR UPDATE row lock to prevent concurrent counter collisions.
CREATE TABLE invoice_number_counter (
    realm_id TEXT NOT NULL,
    year INT NOT NULL,
    next_seq BIGINT NOT NULL DEFAULT 2,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (realm_id, year)
);

COMMENT ON TABLE invoice_number_counter IS 'Counter for sequential invoice numbering within a realm+year scope';
COMMENT ON COLUMN invoice_number_counter.next_seq IS 'Next available sequence number (first invoice uses seq=1 via INSERT)';

-- ====================================
-- Points Distribution Rules (multi-wallet grant rules model)
-- ====================================
-- Unified rule table: one row = one target account + one policy + a non-empty
-- trigger set, owned by an entitlement mapping or a realm registration config.
-- Replaces the single-target points-strategy columns removed from
-- provider_entitlement_mappings and the realm/user config tables above.
CREATE TABLE points_distribution_rules (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    realm_id TEXT NOT NULL,
    owner_type TEXT NOT NULL CHECK (owner_type IN ('entitlement_mapping', 'realm_registration')),
    entitlement_mapping_id UUID REFERENCES provider_entitlement_mappings(id) ON DELETE RESTRICT,
    bucket_id UUID NOT NULL REFERENCES credit_buckets(id) ON DELETE RESTRICT,
    trigger_sources TEXT[] NOT NULL,
    grant_mode TEXT NOT NULL CHECK (grant_mode IN ('fixed', 'quota')),
    points_amount BIGINT,
    validity_days BIGINT,
    grant_period_type TEXT CHECK (grant_period_type IS NULL OR grant_period_type IN ('once', 'daily', 'weekly', 'monthly')),
    quota_windows JSONB,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    display_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_pdr_owner_mapping
        CHECK ((owner_type = 'entitlement_mapping') = (entitlement_mapping_id IS NOT NULL)),
    CONSTRAINT chk_pdr_trigger_sources
        CHECK (
            cardinality(trigger_sources) > 0
            AND array_position(trigger_sources, NULL) IS NULL
            AND trigger_sources <@ ARRAY['topup','subscription_initial','subscription_renewal','subscription_upgrade','registration','free_periodic_grant']::TEXT[]
        ),
    -- fixed: needs points amount, no quota windows; quota: no points amount, needs windows.
    CONSTRAINT chk_pdr_fixed_policy
        CHECK (
            (grant_mode <> 'fixed')
            OR (points_amount IS NOT NULL AND points_amount > 0 AND quota_windows IS NULL)
        ),
    CONSTRAINT chk_pdr_quota_policy
        CHECK (
            (grant_mode <> 'quota')
            OR (points_amount IS NULL AND quota_windows IS NOT NULL)
        ),
    CONSTRAINT chk_pdr_validity_days
        CHECK (validity_days IS NULL OR validity_days >= 0)
);

CREATE INDEX idx_points_distribution_rules_realm_owner_mapping_enabled_order
    ON points_distribution_rules (realm_id, owner_type, entitlement_mapping_id, enabled, display_order);
CREATE INDEX idx_points_distribution_rules_bucket_id
    ON points_distribution_rules (bucket_id);

COMMENT ON TABLE points_distribution_rules IS 'Unified points distribution rules: one rule per target account + policy + trigger set, owned by a mapping or realm registration';
COMMENT ON COLUMN points_distribution_rules.owner_type IS 'entitlement_mapping (rule belongs to a provider entitlement mapping) or realm_registration (rule belongs to realm registration config)';
COMMENT ON COLUMN points_distribution_rules.entitlement_mapping_id IS 'Required when owner_type = entitlement_mapping, NULL when owner_type = realm_registration';
COMMENT ON COLUMN points_distribution_rules.bucket_id IS 'Target credit account for this rule';
COMMENT ON COLUMN points_distribution_rules.trigger_sources IS 'Non-empty subset of the six automatic triggers; domain layer further constrains the subset by owner and billing type';
COMMENT ON COLUMN points_distribution_rules.grant_mode IS 'fixed = fixed points grant, quota = rolling-window quota entitlement';
COMMENT ON COLUMN points_distribution_rules.points_amount IS 'Fixed points amount; required and > 0 when grant_mode = fixed, NULL for quota';
COMMENT ON COLUMN points_distribution_rules.validity_days IS 'Validity in days for fixed grants (0 = permanent)';
COMMENT ON COLUMN points_distribution_rules.grant_period_type IS 'Period type for free-periodic fixed rules (once/daily/weekly/monthly); NULL otherwise';
COMMENT ON COLUMN points_distribution_rules.quota_windows IS 'Snapshot of [{windowSeconds, limit, key}]; required when grant_mode = quota, NULL for fixed';
COMMENT ON COLUMN points_distribution_rules.enabled IS 'Soft-disable: a disabled rule does not participate in new events, but its row and FK are retained';

-- ====================================
-- Points Distribution Events (execution idempotency log)
-- ====================================
-- Lightweight completion record for the six automatic triggers. Serializes
-- concurrent execution via (realm, user, trigger, event_key); a completed row
-- captures the fixed first-execution result set so replay returns the original
-- result regardless of later rule config changes. Not a general event bus.
CREATE TABLE points_distribution_events (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    realm_id TEXT NOT NULL,
    user_id UUID NOT NULL REFERENCES account(id) ON DELETE RESTRICT,
    trigger TEXT NOT NULL CHECK (trigger IN (
        'topup',
        'subscription_initial',
        'subscription_renewal',
        'subscription_upgrade',
        'registration',
        'free_periodic_grant'
    )),
    event_key TEXT NOT NULL,
    source_id TEXT NOT NULL,
    owner_type TEXT NOT NULL CHECK (owner_type IN ('entitlement_mapping', 'realm_registration')),
    entitlement_mapping_id UUID REFERENCES provider_entitlement_mappings(id) ON DELETE RESTRICT,
    status TEXT NOT NULL CHECK (status IN ('processing', 'completed')),
    result_count INTEGER CHECK (result_count IS NULL OR result_count >= 0),
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_points_distribution_events_key
        UNIQUE (realm_id, user_id, trigger, event_key),
    CONSTRAINT chk_points_distribution_events_owner_mapping
        CHECK ((owner_type = 'entitlement_mapping') = (entitlement_mapping_id IS NOT NULL)),
    CONSTRAINT chk_points_distribution_events_completed
        CHECK (
            (status <> 'completed')
            OR (completed_at IS NOT NULL AND result_count IS NOT NULL)
        )
);

COMMENT ON TABLE points_distribution_events IS 'Idempotent execution log for the six automatic points distribution triggers';
COMMENT ON COLUMN points_distribution_events.trigger IS 'One of the six automatic distribution triggers (admin/sdk/system grant are excluded)';
COMMENT ON COLUMN points_distribution_events.event_key IS 'Stable business event key; unique per (realm, user, trigger)';
COMMENT ON COLUMN points_distribution_events.source_id IS 'Payment/subscription/registration source locator';
COMMENT ON COLUMN points_distribution_events.owner_type IS 'Owner that the executed rules belonged to at first execution';
COMMENT ON COLUMN points_distribution_events.status IS 'processing = in-flight inside the executing transaction (never committed), completed = result set finalized';
COMMENT ON COLUMN points_distribution_events.result_count IS 'Logical result count at completion; 0 for zero-rule events';
COMMENT ON COLUMN points_distribution_events.completed_at IS 'Completion timestamp; required when status = completed';

-- ====================================
-- Payment Attempt Point Rules (purchase-time rule snapshot)
-- ====================================
-- Captures the rule + target bucket snapshot at payment attempt creation for
-- the topup / subscription_initial triggers, preserving the
-- "purchase fixes its target accounts" semantics so a later rule disable does
-- not affect an already-paid attempt.
CREATE TABLE payment_attempt_point_rules (
    payment_attempt_id UUID NOT NULL REFERENCES payment_attempts(id) ON DELETE CASCADE,
    rule_id UUID NOT NULL REFERENCES points_distribution_rules(id) ON DELETE RESTRICT,
    bucket_id UUID NOT NULL REFERENCES credit_buckets(id) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (payment_attempt_id, rule_id)
);

CREATE INDEX idx_payment_attempt_point_rules_rule_id
    ON payment_attempt_point_rules (rule_id);

COMMENT ON TABLE payment_attempt_point_rules IS 'Snapshot of distribution rules captured at payment attempt creation (topup / subscription_initial)';
COMMENT ON COLUMN payment_attempt_point_rules.rule_id IS 'Distribution rule matched at purchase creation; later disabling does not affect this snapshot';
COMMENT ON COLUMN payment_attempt_point_rules.bucket_id IS 'Target account snapshot at purchase creation';

-- ====================================
-- Distribution attribution FK constraints (cross-table, post-declaration)
-- ====================================
-- points_credit_ledger / points_transactions / points_grant_schedules /
-- points_quota_entitlements reference points_distribution_events and
-- points_distribution_rules, which are declared above; the FKs are added here
-- so referenced tables always exist regardless of declaration order.

ALTER TABLE points_credit_ledger
    ADD CONSTRAINT fk_points_credit_ledger_distribution_event
        FOREIGN KEY (distribution_event_id) REFERENCES points_distribution_events(id) ON DELETE RESTRICT,
    ADD CONSTRAINT fk_points_credit_ledger_distribution_rule
        FOREIGN KEY (distribution_rule_id) REFERENCES points_distribution_rules(id) ON DELETE RESTRICT;

ALTER TABLE points_transactions
    ADD CONSTRAINT fk_points_transactions_distribution_event
        FOREIGN KEY (distribution_event_id) REFERENCES points_distribution_events(id) ON DELETE RESTRICT,
    ADD CONSTRAINT fk_points_transactions_distribution_rule
        FOREIGN KEY (distribution_rule_id) REFERENCES points_distribution_rules(id) ON DELETE RESTRICT;

ALTER TABLE points_grant_schedules
    ADD CONSTRAINT fk_points_grant_schedules_distribution_event
        FOREIGN KEY (distribution_event_id) REFERENCES points_distribution_events(id) ON DELETE RESTRICT,
    ADD CONSTRAINT fk_points_grant_schedules_distribution_rule
        FOREIGN KEY (distribution_rule_id) REFERENCES points_distribution_rules(id) ON DELETE RESTRICT;

ALTER TABLE points_quota_entitlements
    ADD CONSTRAINT fk_points_quota_entitlements_distribution_event
        FOREIGN KEY (distribution_event_id) REFERENCES points_distribution_events(id) ON DELETE RESTRICT,
    ADD CONSTRAINT fk_points_quota_entitlements_distribution_rule
        FOREIGN KEY (distribution_rule_id) REFERENCES points_distribution_rules(id) ON DELETE RESTRICT;
