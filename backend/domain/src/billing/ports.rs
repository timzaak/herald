use std::future::Future;

use uuid::Uuid;

use crate::billing::{
    BatchMappingError, BatchUpdateMappingsInput, BatchUpdateResult, EntitlementMapping,
    FeatureFacts, PaymentEvent, Subscription, SubscriptionHistoryEvent, SubscriptionHistoryQuery,
};
use crate::common::entities::app_errors::CoreError;
use crate::points::{PointsDistributionRule, RuleUpsert};

/// Repository for billing operations
pub trait BillingRepository: Send + Sync {
    /// Create a new subscription
    fn create_subscription(
        &self,
        sub: Subscription,
    ) -> impl Future<Output = Result<Subscription, CoreError>> + Send;

    /// Find subscription by realm ID
    fn find_by_realm_id(
        &self,
        realm_id: &str,
    ) -> impl Future<Output = Result<Option<Subscription>, CoreError>> + Send;

    /// Find subscription by external subscription ID and provider
    fn find_by_external_subscription_id(
        &self,
        external_sub_id: &str,
        provider: &str,
    ) -> impl Future<Output = Result<Option<Subscription>, CoreError>> + Send;

    /// Find subscription by local subscription ID
    fn find_subscription_by_id(
        &self,
        subscription_id: Uuid,
    ) -> impl Future<Output = Result<Option<Subscription>, CoreError>> + Send;

    /// Update subscription
    fn update_subscription(
        &self,
        sub: Subscription,
    ) -> impl Future<Output = Result<Subscription, CoreError>> + Send;

    /// Create a payment event record
    fn create_payment_event(
        &self,
        event: PaymentEvent,
    ) -> impl Future<Output = Result<PaymentEvent, CoreError>> + Send;

    /// Find payment event by realm + external event ID + provider (for
    /// idempotency). Realm must be part of the key: realms that share one
    /// provider account receive the same external event id independently.
    fn find_payment_event_by_external_id(
        &self,
        realm_id: &str,
        external_event_id: &str,
        payment_provider: &str,
    ) -> impl Future<Output = Result<Option<PaymentEvent>, CoreError>> + Send;

    /// Mark payment event as processed
    fn mark_payment_event_processed(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// Find subscription by client app ID
    fn find_subscription_by_client_app_id(
        &self,
        client_app_id: Uuid,
    ) -> impl Future<Output = Result<Option<Subscription>, CoreError>> + Send;

    /// Cancel subscription
    fn cancel_subscription(
        &self,
        subscription_id: Uuid,
        cancel_at_period_end: bool,
    ) -> impl Future<Output = Result<Subscription, CoreError>> + Send;

    /// Cancel all active subscriptions matching an external_subscription_id for a user in a realm.
    /// Returns the number of rows updated.
    fn cancel_subscriptions_by_external_id(
        &self,
        realm_id: &str,
        user_id: Uuid,
        external_subscription_id: &str,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;

    /// Cancel all active subscriptions matching an entitlement_key for a user in a realm.
    /// Returns the number of rows updated.
    fn cancel_subscriptions_by_entitlement_key(
        &self,
        realm_id: &str,
        user_id: Uuid,
        entitlement_key: &str,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;

    /// List a user's in-effect subscriptions in a realm (statuses that grant
    /// access: active, trialing, scheduled_cancel, dispute). Used by account
    /// self-delete to cancel in-effect subscriptions post-transaction.
    fn list_active_subscriptions_by_user(
        &self,
        realm_id: &str,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Subscription>, CoreError>> + Send;

    // ===== Subscription History =====
    /// Save a subscription history event
    fn save_history_event(
        &self,
        event: SubscriptionHistoryEvent,
    ) -> impl Future<Output = Result<SubscriptionHistoryEvent, CoreError>> + Send;

    /// Get history for a specific subscription
    fn get_subscription_history(
        &self,
        realm_id: &str,
        subscription_id: &Uuid,
    ) -> impl Future<Output = Result<Vec<SubscriptionHistoryEvent>, CoreError>> + Send;

    /// List subscription history with filtering and pagination
    /// Returns (events, total_count)
    fn list_subscription_history(
        &self,
        realm_id: &str,
        query: SubscriptionHistoryQuery,
    ) -> impl Future<Output = Result<(Vec<SubscriptionHistoryEvent>, u64), CoreError>> + Send;

    // ===== Entitlement Mapping CRUD =====

    /// Create an entitlement mapping
    fn create_entitlement_mapping(
        &self,
        mapping: EntitlementMapping,
    ) -> impl Future<Output = Result<EntitlementMapping, CoreError>> + Send;

    /// Find entitlement mapping by ID
    fn find_entitlement_mapping_by_id(
        &self,
        mapping_id: Uuid,
    ) -> impl Future<Output = Result<Option<EntitlementMapping>, CoreError>> + Send;

    /// List entitlement mappings for a realm with optional filters
    fn list_entitlement_mappings(
        &self,
        realm_id: &str,
        payment_provider: Option<&str>,
        enabled: Option<bool>,
        page: Option<u64>,
        page_size: Option<u64>,
    ) -> impl Future<Output = Result<(Vec<EntitlementMapping>, u64), CoreError>> + Send;

    /// Update an entitlement mapping
    fn update_entitlement_mapping(
        &self,
        mapping: EntitlementMapping,
    ) -> impl Future<Output = Result<EntitlementMapping, CoreError>> + Send;

    /// Upsert an entitlement mapping by (realm_id, payment_provider, external_product_id)
    fn upsert_entitlement_mapping(
        &self,
        mapping: EntitlementMapping,
    ) -> impl Future<Output = Result<EntitlementMapping, CoreError>> + Send;

    /// Find entitlement mapping by (realm, provider, external product, external price).
    ///
    /// `external_price_id: Option<&str>` — Stripe passes `Some(price_id)`; Creem
    /// (product-level, no price id) passes `None`, which maps to
    /// `external_price_id IS NULL` and relies on the `NULLS NOT DISTINCT` unique
    /// constraint (migration `20260607_product_reduce.sql`) for dedup.
    fn find_entitlement_mapping_by_provider_product_price(
        &self,
        realm_id: &str,
        payment_provider: &str,
        external_product_id: &str,
        external_price_id: Option<&str>,
    ) -> impl Future<Output = Result<Option<EntitlementMapping>, CoreError>> + Send;

    /// Find entitlement mapping by entitlement key
    fn find_entitlement_mapping_by_key(
        &self,
        realm_id: &str,
        entitlement_key: &str,
    ) -> impl Future<Output = Result<Option<EntitlementMapping>, CoreError>> + Send;

    /// Find entitlement mapping by (realm, entitlement_key, external_price_id).
    ///
    /// Used by the webhook resolution chain step 3.b to re-locate
    /// the strategy mapping when the (provider, product, price) lookup missed but
    /// a metadata `herald_entitlement_key` is present. Under shared-key + multiple
    /// prices this disambiguates to the price-level row.
    ///
    /// `external_price_id: Option<&str>` — Stripe passes `Some(price_id)`; Creem
    /// passes `None`, mapping to `external_price_id IS NULL`.
    fn find_entitlement_mapping_by_key_price(
        &self,
        realm_id: &str,
        entitlement_key: &str,
        external_price_id: Option<&str>,
    ) -> impl Future<Output = Result<Option<EntitlementMapping>, CoreError>> + Send;

    /// List entitlement mappings for (realm, payment_provider, external_product_id).
    ///
    /// Used by the webhook resolution chain step 3.c to detect the
    /// ambiguous multi-price case (>1 row) when metadata is missing.
    fn list_entitlement_mappings_by_provider_product(
        &self,
        realm_id: &str,
        payment_provider: &str,
        external_product_id: &str,
    ) -> impl Future<Output = Result<Vec<EntitlementMapping>, CoreError>> + Send;

    /// List enabled one-time entitlement mappings with product info for external API
    fn list_one_time_mappings(
        &self,
        realm_id: &str,
    ) -> impl Future<Output = Result<Vec<EntitlementMapping>, CoreError>> + Send;

    /// List enabled Stripe mapping rows for an entitlement key, scoped to the
    /// realm. Backs the external currency-set aggregation and the by-currency
    /// default-price resolution; Creem/IAP rows are excluded because their
    /// pricing is provider/store-side.
    fn find_enabled_stripe_mappings_by_entitlement(
        &self,
        realm_id: &str,
        entitlement_key: &str,
    ) -> impl Future<Output = Result<Vec<EntitlementMapping>, CoreError>> + Send;

    /// Find the external subscription ID associated with a payment intent,
    /// by tracing through previously stored payment events (e.g. checkout.session.completed).
    fn find_external_subscription_id_by_payment_intent(
        &self,
        payment_intent: &str,
        provider: &str,
        realm_id: &str,
    ) -> impl Future<Output = Result<Option<String>, CoreError>> + Send;

    /// List subscriptions for a realm with filters and pagination.
    /// Returns (subscriptions, total_count).
    fn list_subscriptions(
        &self,
        realm_id: &str,
        entitlement_key: Option<&str>,
        status: Option<&str>,
        payment_provider: Option<&str>,
        page: u64,
        page_size: u64,
    ) -> impl Future<Output = Result<(Vec<Subscription>, u64), CoreError>> + Send;

    /// Check feature availability facts for a realm by querying multiple billing tables.
    fn check_feature_facts(
        &self,
        realm_id: &str,
        pool: &sqlx::PgPool,
    ) -> impl Future<Output = Result<FeatureFacts, CoreError>> + Send;

    /// Atomically batch-upsert ALL price rows for one product.
    ///
    /// Single transaction: upsert every row, and reject the WHOLE batch with
    /// [`BatchMappingError::ActiveSubscriptionLock`] if any row transitions
    /// `enabled` true→false while protected by an active subscription.
    /// `entitlement_key` is provider-owned/read-only and not written by this
    /// path; cross-realm/product `mapping_id` tampering surfaces as
    /// [`BatchMappingError::MappingNotInGroup`].
    fn batch_update_mappings(
        &self,
        input: BatchUpdateMappingsInput,
    ) -> impl Future<Output = Result<BatchUpdateResult, BatchMappingError>> + Send;

    // ===== Distribution Rules (multi-wallet grant rules) =====

    /// Create a mapping and its initial rule set in a single transaction.
    ///
    /// The mapping row is inserted first, then each rule in `rules` is upserted
    /// under `EntitlementMapping(new_mapping_id)`. Rules absent from `rules`
    /// are irrelevant on create (there are no prior rules). Any error rolls
    /// back both the mapping and the rules (DEC-005 atomic upsert).
    fn create_entitlement_mapping_with_rules(
        &self,
        mapping: EntitlementMapping,
        rules: Vec<RuleUpsert>,
    ) -> impl Future<Output = Result<EntitlementMapping, CoreError>> + Send;

    /// Atomically upsert a mapping's base fields together with its rule set
    /// (DEC-005). The mapping base fields are written, then the rule set is
    /// applied as an upsert under `EntitlementMapping(mapping.id)`:
    /// - rules in `rules` with `id = None` are created;
    /// - rules with `id = Some(existing)` are updated (must already belong to
    ///   this mapping — otherwise `distribution_rule_conflict`);
    /// - rules NOT present in `rules` are left untouched (DEC-007: referenced
    ///   rules are never hard-deleted; disabling requires explicit
    ///   `enabled = false`).
    ///
    /// All writes commit in one transaction; any error rolls back the whole
    /// set so no partial rule state is left.
    fn upsert_mapping_with_rules(
        &self,
        realm_id: &str,
        mapping: EntitlementMapping,
        rules: Vec<RuleUpsert>,
    ) -> impl Future<Output = Result<EntitlementMapping, CoreError>> + Send;

    /// Load the rules owned by a mapping (all rules, enabled and disabled),
    /// ordered by `(display_order, id)` for stable display.
    /// Returns an empty vec for a mapping with no rules.
    fn find_mapping_rules(
        &self,
        realm_id: &str,
        mapping_id: Uuid,
    ) -> impl Future<Output = Result<Vec<PointsDistributionRule>, CoreError>> + Send;

    /// Atomically upsert the Realm's registration rule set (DEC-005). Same
    /// upsert/disable semantics as [`Self::upsert_mapping_with_rules`] but for
    /// the `RealmRegistration` owner. Returns the full current rule set ordered
    /// by `(display_order, id)`.
    fn upsert_registration_rules(
        &self,
        realm_id: &str,
        rules: Vec<RuleUpsert>,
    ) -> impl Future<Output = Result<Vec<PointsDistributionRule>, CoreError>> + Send;

    /// Load the Realm's registration rules (all rules, enabled and disabled),
    /// ordered by `(display_order, id)` for stable display. Returns an empty
    /// vec when no registration rules are configured.
    fn find_registration_rules(
        &self,
        realm_id: &str,
    ) -> impl Future<Output = Result<Vec<PointsDistributionRule>, CoreError>> + Send;
}
