// Purchase fulfillment service implementation

use std::sync::Arc;

use herald_domain::authorization::PermissionService;
use herald_domain::billing::{BillingRepository, BillingType, Subscription, SubscriptionStatus};
use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::payment_attempt::{PaymentAttempt, PaymentAttemptRepository};
use herald_domain::points::{
    DistributionEvent, DistributionGrantResult, DistributionRuleOwner, DistributionRuleSelection,
    DistributionTrigger, PointsRepository, credit_pair_for_trigger, event_key_for_payment,
    event_key_for_subscription_period,
};
use herald_domain::purchase::{
    FulfillmentResult, FulfillmentService, FulfillmentType, PointsGrant,
};
use herald_domain::user::{GrantRoleOutcome, UserRoleRepository};

fn billing_period_to_days(period: Option<&str>) -> i64 {
    match period.map(|p| p.trim().to_ascii_lowercase()).as_deref() {
        Some("daily") | Some("day") => 1,
        Some("weekly") | Some("week") => 7,
        Some("monthly") | Some("month") => 30,
        Some("quarterly") | Some("quarter") => 90,
        Some("yearly") | Some("annual") | Some("annually") | Some("year") => 365,
        _ => 30,
    }
}

/// Implementation of fulfillment service for unified purchase handling.
///
/// Generics:
/// - `P`: points repository (distribution rule executor).
/// - `B`: billing repository (entitlement mapping + subscription).
/// - `PA`: payment-attempt repository — loads the rule/bucket snapshot captured
///   at purchase creation for the `CapturedPaymentRules` executor selection.
/// - `U`: user-role repository — used by the payment-driven role grant loop
///   event, not an authenticated admin action (no `Identity::System` variant
///   exists — `backend/domain/src/authentication/identity.rs:27`).
/// - `C`: permission service — invoked solely for `invalidate_user_role_cache`
///   after a grant so subsequent permission checks see the new role. Injecting
///   this port keeps the fulfillment service free of any direct Redis
///   dependency (the concrete `RedisPermissionChecker` lives in infra).
pub struct PostgresFulfillmentService<P, B, PA, U, C>
where
    P: PointsRepository,
    B: BillingRepository,
    PA: PaymentAttemptRepository,
    U: UserRoleRepository,
    C: PermissionService,
{
    points_repository: Arc<P>,
    billing_repository: Arc<B>,
    payment_attempt_repository: Arc<PA>,
    user_role_repository: Arc<U>,
    permission_service: Arc<C>,
}

impl<P, B, PA, U, C> PostgresFulfillmentService<P, B, PA, U, C>
where
    P: PointsRepository,
    B: BillingRepository,
    PA: PaymentAttemptRepository,
    U: UserRoleRepository,
    C: PermissionService,
{
    pub fn new(
        points_repository: Arc<P>,
        billing_repository: Arc<B>,
        payment_attempt_repository: Arc<PA>,
        user_role_repository: Arc<U>,
        permission_service: Arc<C>,
    ) -> Self {
        Self {
            points_repository,
            billing_repository,
            payment_attempt_repository,
            user_role_repository,
            permission_service,
        }
    }

    /// Grant every role in `role_ids` to `user_id` as a payment-driven grant,
    /// can re-process the attempt (it is NOT silently swallowed).
    async fn grant_payment_roles(
        &self,
        realm_id: &str,
        user_id: uuid::Uuid,
        role_ids: &[uuid::Uuid],
        source_id: &str,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), CoreError> {
        for role_id in role_ids {
            match self
                .user_role_repository
                .grant_role_by_payment(
                    realm_id, user_id, *role_id,
                    // PaymentAttempt does not carry a client_id; the user_roles
                    // client_id column is nullable, so pass None.
                    None, source_id, expires_at,
                )
                .await
            {
                Ok(GrantRoleOutcome::Granted) => {
                    tracing::info!(
                        user_id = %user_id,
                        role_id = %role_id,
                        source_id = %source_id,
                        "Payment role granted"
                    );
                }
                Ok(GrantRoleOutcome::AlreadyExists) => {
                    tracing::info!(
                        user_id = %user_id,
                        role_id = %role_id,
                        source_id = %source_id,
                        "Payment role already granted (idempotent skip)"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        user_id = %user_id,
                        role_id = %role_id,
                        source_id = %source_id,
                        error = %e,
                        "Failed to grant payment role"
                    );
                    return Err(e.into());
                }
            }
        }

        // Invalidate the user's cached roles/permissions so the newly granted
        // role is visible to subsequent authorization checks.
        if let Err(e) = self
            .permission_service
            .invalidate_user_role_cache(realm_id, &user_id.to_string())
            .await
        {
            // Cache invalidation is best-effort relative to the durable grant:
            // the row is already committed, and the cache entry has a TTL, so a
            // transient Redis failure must not roll back a successful payment.
            tracing::warn!(
                user_id = %user_id,
                realm_id = %realm_id,
                error = %e,
                "Failed to invalidate user role cache after payment grant (will expire on TTL)"
            );
        }

        Ok(())
    }

    /// Execute the distribution rules captured on the payment attempt at
    /// purchase creation, returning the multi-rule grant set. This is the single
    /// shared first-fulfillment points path for every provider (Stripe / Creem /
    /// IAP) and every purchase shape (topup / subscription-initial / non-renewing
    /// initial) — there is no per-provider fork.
    ///
    /// The captured snapshot is frozen at attempt creation: a rule disabled after
    /// capture still fires for this already-paid attempt. Replay of an
    /// already-completed event returns the first-run
    /// results without re-reading current rules (the executor's unique-key
    /// serialization + completion record handle this).
    async fn execute_captured_payment_rules(
        &self,
        attempt: &PaymentAttempt,
        mapping_id: uuid::Uuid,
        trigger: DistributionTrigger,
        event_key: String,
        source_id: String,
    ) -> Result<Vec<PointsGrant>, CoreError> {
        // Load the captured rule/bucket refs. An empty set is a valid
        // zero-rule attempt: the executor still completes a zero-result event
        // so a replay is idempotent, and we return an empty grant array.
        let refs = self
            .payment_attempt_repository
            .find_captured_rule_refs(&attempt.realm_id, attempt.id)
            .await?;

        let owner = DistributionRuleOwner::EntitlementMapping(mapping_id);
        let event = DistributionEvent {
            realm_id: attempt.realm_id.clone(),
            user_id: attempt.user_id,
            owner,
            trigger,
            event_key,
            source_id,
            effective_from: chrono::Utc::now(),
            effective_until: None,
        };

        let results = self
            .points_repository
            .execute_distribution_event_atomic(
                event,
                DistributionRuleSelection::CapturedPaymentRules(refs),
            )
            .await?;

        Ok(Self::grant_results_to_points_grants(
            results,
            trigger,
            &attempt.payment_provider,
        ))
    }

    /// Fold the executor's heterogeneous grant results into the flat
    /// `PointsGrant` array carried by `FulfillmentResult`. Each entry surfaces
    /// the rule id, target bucket, concrete result id (ledger / entitlement /
    /// schedule), credit type and amount. Quota grants report `points = None`
    /// (their value is a rolling window surfaced via the balance/quota APIs);
    /// fixed and schedule first-period grants report the granted amount.
    fn grant_results_to_points_grants(
        results: Vec<DistributionGrantResult>,
        trigger: DistributionTrigger,
        payment_provider: &str,
    ) -> Vec<PointsGrant> {
        let (credit_type, _source_type) = credit_pair_for_trigger(trigger);
        let points_type = credit_type.as_str().to_string();
        let provider_label = payment_provider;
        results
            .into_iter()
            .map(|result| match result {
                DistributionGrantResult::Fixed {
                    rule_id,
                    bucket_id,
                    ledger_id,
                    amount,
                } => PointsGrant {
                    rule_id,
                    bucket_id,
                    result_id: ledger_id,
                    points_type: points_type.clone(),
                    points: Some(amount),
                    description: format!("{trigger} grant ({provider_label})"),
                },
                DistributionGrantResult::Quota {
                    rule_id,
                    bucket_id,
                    entitlement_id,
                } => PointsGrant {
                    rule_id,
                    bucket_id,
                    result_id: entitlement_id,
                    points_type: points_type.clone(),
                    points: None,
                    description: format!("{trigger} quota entitlement ({provider_label})"),
                },
                DistributionGrantResult::Schedule {
                    rule_id,
                    bucket_id,
                    schedule_id,
                    first_ledger_id: _,
                } => PointsGrant {
                    rule_id,
                    bucket_id,
                    result_id: schedule_id,
                    points_type: points_type.clone(),
                    // The schedule's first-period amount is not carried on the
                    // result variant; quota-style null keeps the schedule's
                    // rolling nature explicit on the grant array.
                    points: None,
                    description: format!("{trigger} scheduled grant ({provider_label})"),
                },
            })
            .collect()
    }
}

impl<P, B, PA, U, C> FulfillmentService for PostgresFulfillmentService<P, B, PA, U, C>
where
    P: PointsRepository + Send + Sync,
    B: BillingRepository + Send + Sync,
    PA: PaymentAttemptRepository + Send + Sync,
    U: UserRoleRepository + Send + Sync,
    C: PermissionService + Send + Sync,
{
    async fn fulfill_subscription_purchase(
        &self,
        attempt: &PaymentAttempt,
        provider_transaction_id: String,
    ) -> Result<FulfillmentResult, CoreError> {
        tracing::info!(
            payment_attempt_id = %attempt.id,
            realm_id = %attempt.realm_id,
            user_id = %attempt.user_id,
            target_id = %attempt.target_id,
            "Fulfilling subscription purchase"
        );
        self.fulfill_subscription_shape(attempt, provider_transaction_id, BillingType::Recurring)
            .await
    }

    async fn fulfill_one_time_purchase(
        &self,
        attempt: &PaymentAttempt,
        provider_transaction_id: String,
    ) -> Result<FulfillmentResult, CoreError> {
        tracing::info!(
            payment_attempt_id = %attempt.id,
            realm_id = %attempt.realm_id,
            user_id = %attempt.user_id,
            target_id = %attempt.target_id,
            "Fulfilling one-time purchase"
        );

        // Read mapping from billing_repository by target_id with realm isolation check.
        // The mapping owns the distribution rules; its id is the rule owner and
        // the entitlement-key source for the role grant.
        let mapping = self
            .billing_repository
            .find_entitlement_mapping_by_id(attempt.target_id)
            .await?
            .filter(|m| m.realm_id == attempt.realm_id)
            .ok_or_else(|| {
                CoreError::not_found(&format!(
                    "Entitlement mapping {} for one-time purchase",
                    attempt.target_id
                ))
            })?;
        // Disabled mapping: no points, no roles (PRD support-iap §4.1). The
        // attempt still completes as succeeded — the user paid the store.
        if !mapping.enabled {
            tracing::info!(
                realm_id = %attempt.realm_id,
                mapping_id = %mapping.id,
                attempt_id = %attempt.id,
                "mapping disabled — one-time purchase completes without points/role grants"
            );
            return Ok(FulfillmentResult {
                fulfillment_type: FulfillmentType::PointsGranted,
                subscription_id: None,
                point_grants: Vec::new(),
                granted_at: chrono::Utc::now(),
            });
        }

        // Execute the captured rule snapshot. The executor is idempotent on the
        // event key `payment:{attempt_id}`: a replayed (already-completed) event
        // returns the first-run results without re-reading current rules, and a
        // zero-rule attempt completes an empty event. `source_id` = attempt id
        // (the snapshot locator the executor's CapturedPaymentRules branch
        // parses back into a payment_attempt_id).
        let granted_at = chrono::Utc::now();
        let point_grants = self
            .execute_captured_payment_rules(
                attempt,
                mapping.id,
                DistributionTrigger::Topup,
                event_key_for_payment(attempt.id),
                attempt.id.to_string(),
            )
            .await?;

        // Role grant follows the points transaction, keeping the existing
        // idempotent / best-effort-cache-invalidation compensation semantics.
        // One-time role grants are permanent: source_id = attempt.id.
        if !mapping.granted_role_ids.is_empty() {
            self.grant_payment_roles(
                &attempt.realm_id,
                attempt.user_id,
                &mapping.granted_role_ids,
                &attempt.id.to_string(),
                None,
            )
            .await?;
        }

        let _ = provider_transaction_id;
        Ok(FulfillmentResult {
            fulfillment_type: FulfillmentType::PointsGranted,
            subscription_id: None,
            point_grants,
            granted_at,
        })
    }

    /// not auto-renew. Delegates to [`fulfill_subscription_shape`] with
    /// `BillingType::NonRenewing`, which derives the service period from
    /// `mapping.service_duration_days` and stamps `cancel_at = period_end`.
    async fn fulfill_non_renewing_purchase(
        &self,
        attempt: &PaymentAttempt,
        provider_transaction_id: String,
    ) -> Result<FulfillmentResult, CoreError> {
        tracing::info!(
            payment_attempt_id = %attempt.id,
            realm_id = %attempt.realm_id,
            user_id = %attempt.user_id,
            target_id = %attempt.target_id,
            "Fulfilling non-renewing subscription purchase"
        );
        self.fulfill_subscription_shape(attempt, provider_transaction_id, BillingType::NonRenewing)
            .await
    }
}

impl<P, B, PA, U, C> PostgresFulfillmentService<P, B, PA, U, C>
where
    P: PointsRepository + Send + Sync,
    B: BillingRepository + Send + Sync,
    PA: PaymentAttemptRepository + Send + Sync,
    U: UserRoleRepository + Send + Sync,
    C: PermissionService + Send + Sync,
{
    /// Shared mechanics for both subscription-shape fulfillment paths:
    /// [`fulfill_subscription_purchase`] (`Recurring`) and
    /// [`fulfill_non_renewing_purchase`] (`NonRenewing`). Handles external-id
    /// idempotency, the realm-isolated mapping lookup, subscription creation,
    /// the subscribe-time credit grant, and the payment-driven role grant.
    ///
    /// The two paths differ only in how the service period is derived and how
    /// the snapshot is stamped — both derived from `billing_type`:
    /// - `Recurring`: `period_days = billing_period_to_days(mapping.billing_period)`,
    ///   `cancel_at = None`;
    /// - `NonRenewing`: `service_duration_days` must be `>= 1` (else 400,
    ///   to 500), and `cancel_at = Some(period_end)` expresses "does not renew".
    ///
    /// Re-purchase after expiry is a new attempt with a new external id, so it
    /// creates an independent Subscription row (unblocked, unmerged — PRD
    /// US-PM-006 scenario 3); the M3 duplicate-purchase guard only applies to
    /// one_time+role purchases, so it never wrongly blocks a re-purchase.
    ///
    /// `OneTime` is fulfilled via [`fulfill_one_time_purchase`] and never
    /// reaches here; the `_` arm fails loud if misused.
    async fn fulfill_subscription_shape(
        &self,
        attempt: &PaymentAttempt,
        provider_transaction_id: String,
        billing_type: BillingType,
    ) -> Result<FulfillmentResult, CoreError> {
        // Idempotency: same external subscription id check for both paths.
        // A duplicate webhook / replay returns the existing subscription
        // without re-granting.
        if let Some(existing_subscription) = self
            .billing_repository
            .find_by_external_subscription_id(&provider_transaction_id, &attempt.payment_provider)
            .await?
        {
            tracing::info!(
                payment_attempt_id = %attempt.id,
                existing_subscription_id = %existing_subscription.id,
                "Existing subscription found for payment attempt, returning existing fulfillment"
            );

            let period_start_token = existing_subscription
                .current_period_start
                .unwrap_or(existing_subscription.created_at)
                .to_rfc3339();
            let point_grants = self
                .execute_captured_payment_rules(
                    attempt,
                    attempt.target_id,
                    DistributionTrigger::SubscriptionInitial,
                    event_key_for_subscription_period(
                        existing_subscription.id,
                        &period_start_token,
                    ),
                    attempt.id.to_string(),
                )
                .await?;

            return Ok(FulfillmentResult {
                fulfillment_type: FulfillmentType::SubscriptionCreated,
                subscription_id: Some(existing_subscription.id),
                point_grants,
                granted_at: existing_subscription.created_at,
            });
        }

        // Look up entitlement mapping by ID with realm isolation check.
        let mapping = self
            .billing_repository
            .find_entitlement_mapping_by_id(attempt.target_id)
            .await?
            .filter(|m| m.realm_id == attempt.realm_id)
            .ok_or_else(|| {
                CoreError::not_found(&format!(
                    "Entitlement mapping {} for subscription fulfillment",
                    attempt.target_id
                ))
            })?;

        let entitlement_key = mapping.entitlement_key.clone();

        let now = chrono::Utc::now();
        // Derive the service-period length from the billing type. NonRenewing
        // reads `service_duration_days` (failing loud with 400 if missing / < 1,
        // since a malformed value would otherwise produce a degenerate
        // `current_period_end = now`); Recurring reads `billing_period`.
        let period_days = match billing_type {
            BillingType::Recurring => billing_period_to_days(mapping.billing_period.as_deref()),
            BillingType::NonRenewing => mapping
                .service_duration_days
                .filter(|d| *d >= 1)
                .ok_or_else(|| {
                    CoreError::BadRequest(format!(
                        "Non-renewing mapping '{}' is missing a valid service_duration_days (>= 1)",
                        attempt.target_id
                    ))
                })?,
            // OneTime is fulfilled via fulfill_one_time_purchase.
            _ => unreachable!(
                "fulfill_subscription_shape is for subscription-shape billing types only"
            ),
        };
        let period_end = now + chrono::Duration::days(period_days);
        // NonRenewing stamps `cancel_at = period_end` to express "will not renew
        // because there is no auto-renewal to flip off.
        let cancel_at = matches!(billing_type, BillingType::NonRenewing).then_some(period_end);

        // Initial points fulfillment uses the captured rule snapshot below.
        let subscription = Subscription {
            id: uuid::Uuid::now_v7(),
            realm_id: attempt.realm_id.clone(),
            user_id: attempt.user_id,
            external_subscription_id: provider_transaction_id.clone(),
            external_product_id: attempt.target_id.to_string(),
            payment_provider: attempt.payment_provider.clone(),
            status: SubscriptionStatus::Active,
            entitlement_key: entitlement_key.clone(),
            billing_type,
            external_price_id: mapping.external_price_id.clone(),
            provider_metadata: None,
            synced_at: Some(now),
            current_period_start: Some(now),
            current_period_end: Some(period_end),
            cancel_at_period_end: false,
            client_app_id: None,
            cancel_at,
            created_at: now,
            updated_at: now,
        };

        tracing::info!(
            subscription_id = %subscription.id,
            realm_id = %subscription.realm_id,
            user_id = ?subscription.user_id,
            entitlement_key = %entitlement_key,
            billing_type = %subscription.billing_type.as_str(),
            period_days,
            "Creating new subscription from payment attempt"
        );

        // Create subscription in database
        let created_subscription = self
            .billing_repository
            .create_subscription(subscription)
            .await?;

        tracing::info!(
            subscription_id = %created_subscription.id,
            "Subscription created successfully"
        );

        // Execute the captured subscription_initial rule snapshot. The event
        // key is `subscription:{subscription_id}:period:{period_start}` (shared
        // with the renewal key so a replayed period webhook converges on the
        // same row); `source_id` = attempt id so the executor's
        // CapturedPaymentRules branch resolves the snapshot. The executor is
        // idempotent on the event key and freezes the first-run result set.
        //
        // A disabled mapping keeps the subscription projection (the row above)
        // but grants nothing (PRD support-iap §4.1: notifications for a
        // disabled mapping update the projection without points or roles;
        // re-enabling resumes grants at the next event).
        if !mapping.enabled {
            tracing::info!(
                realm_id = %attempt.realm_id,
                mapping_id = %mapping.id,
                subscription_id = %created_subscription.id,
                "mapping disabled — subscription projected without points/role grants"
            );
            return Ok(FulfillmentResult {
                fulfillment_type: FulfillmentType::SubscriptionCreated,
                subscription_id: Some(created_subscription.id),
                point_grants: Vec::new(),
                granted_at: created_subscription.created_at,
            });
        }

        let period_start_token = created_subscription
            .current_period_start
            .unwrap_or(created_subscription.created_at)
            .to_rfc3339();
        let point_grants = self
            .execute_captured_payment_rules(
                attempt,
                mapping.id,
                DistributionTrigger::SubscriptionInitial,
                event_key_for_subscription_period(created_subscription.id, &period_start_token),
                attempt.id.to_string(),
            )
            .await?;

        // Role grant follows the points transaction, keeping the existing
        // idempotent / best-effort-cache-invalidation compensation semantics.
        // Source id is the subscription id; expiry aligns to the period end so
        // roles naturally lapse at expiry for NonRenewing (and the M4 sweep /
        // explicit revoke catch them).
        if !mapping.granted_role_ids.is_empty() {
            self.grant_payment_roles(
                &attempt.realm_id,
                attempt.user_id,
                &mapping.granted_role_ids,
                &created_subscription.id.to_string(),
                created_subscription.current_period_end,
            )
            .await?;
        }

        let _ = entitlement_key;
        Ok(FulfillmentResult {
            fulfillment_type: FulfillmentType::SubscriptionCreated,
            subscription_id: Some(created_subscription.id),
            point_grants,
            granted_at: created_subscription.created_at,
        })
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_billing_period_to_days() {
        assert_eq!(billing_period_to_days(Some("daily")), 1);
        assert_eq!(billing_period_to_days(Some("day")), 1);
        assert_eq!(billing_period_to_days(Some("weekly")), 7);
        assert_eq!(billing_period_to_days(Some("month")), 30);
        assert_eq!(billing_period_to_days(Some("yearly")), 365);
        assert_eq!(billing_period_to_days(None), 30);
    }
}
