-- Provider-refund ledger for topup refund revocation. One row per processed
-- payment-provider refund (Stripe `re_...` / Creem refund id):
--   * the UNIQUE (realm_id, payment_provider, refund_id) constraint is the
--     persistent idempotency unit for refund revocation — a repeated
--     push of the same refund never revokes twice, regardless of the event id
--     it arrives under (replaces the 24h-TTL `refund:topup:` idempotency_keys
--     business key);
--   * `amount` is that single refund's own amount (the incremental revocation
--     input), never the cumulative provider figure;
--   * `cumulative_refunded_after` snapshots the cumulative refund total after
--     this refund (Stripe: provider-authoritative payload `amount_refunded`;
--     Creem: in-transaction SUM over this attempt's rows) — the recorded input
--     of the full-refund gate that decides payment-role revocation.
-- No backfill: refunds that arrived before this table are not reconstructed
-- (history is not retroactively compensated).
CREATE TABLE payment_refunds (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    realm_id TEXT NOT NULL,
    payment_provider TEXT NOT NULL CHECK (payment_provider IN ('stripe', 'creem')),
    payment_attempt_id UUID NOT NULL,
    refund_id TEXT NOT NULL,
    amount BIGINT NOT NULL CHECK (amount > 0),
    original_payment_amount BIGINT NOT NULL CHECK (original_payment_amount > 0),
    cumulative_refunded_after BIGINT NOT NULL CHECK (cumulative_refunded_after > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uk_payment_refunds_provider_refund
        UNIQUE (realm_id, payment_provider, refund_id)
);

CREATE INDEX idx_payment_refunds_attempt ON payment_refunds(payment_attempt_id);

COMMENT ON TABLE payment_refunds IS 'Processed provider refunds: persistent refund-id idempotency and cumulative-refund tracking for topup refund revocation';
COMMENT ON COLUMN payment_refunds.amount IS 'This refund''s own amount (minimal currency unit) — incremental revocation input, not the cumulative total';
COMMENT ON COLUMN payment_refunds.original_payment_amount IS 'Immutable snapshot of payment_attempts.amount at refund time — gate denominator';
COMMENT ON COLUMN payment_refunds.cumulative_refunded_after IS 'Cumulative refunded total including this refund (Stripe payload amount_refunded / Creem in-table SUM) — full-refund gate input';
