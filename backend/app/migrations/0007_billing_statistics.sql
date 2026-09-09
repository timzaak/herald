-- Realm-scoped window aggregation for payment statistics and the existing
-- admin purchase-history query. Mirrors idx_points_transactions_realm_created
-- (0002_billing.sql) — payment_attempts had no realm-leading index.
CREATE INDEX idx_payment_attempts_realm_created
    ON payment_attempts(realm_id, created_at DESC);
