-- ====================================
-- Realm-scoped payment_event idempotency
-- ====================================
-- The original UNIQUE (external_event_id, payment_provider) constraint and the
-- matching repository lookup were realm-unscoped. Two realms that share one
-- provider account (each registers its own per-realm webhook URL, so the
-- provider delivers the same event id to both) collided: the second realm's
-- insert hit the unique constraint and its idempotency pre-check matched the
-- first realm's row, silently skipping fulfillment. Scope both the constraint
-- and the code lookup to the realm.
--
-- Existing data cannot violate the new constraint: it is strictly narrower
-- than the old global unique constraint.

ALTER TABLE payment_event
    DROP CONSTRAINT IF EXISTS payment_event_unique_external_provider,
    ADD CONSTRAINT payment_event_unique_external_provider
        UNIQUE (realm_id, external_event_id, payment_provider);
