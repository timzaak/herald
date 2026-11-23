use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

use crate::authorization::PermissionService;
use crate::billing::entities::EntitlementMapping;
use crate::common::entities::app_errors::CoreError;
use crate::common::entities::{generate_uuid_v7, now_utc};
use crate::points::{
    DistributionEvent, DistributionGrantResult, DistributionRuleOwner, DistributionRuleSelection,
    DistributionTrigger, PointsQuotaEntitlement,
    dtos::RevokePointsOutput,
    entities::{CreditType, QuotaEntitlementStatus, QuotaSourceType, QuotaWindow, RevocationType},
    event_key_for_subscription_period, event_key_for_subscription_upgrade,
    ports::PointsRepository,
    service::PointsService,
};
use crate::user::admin_ports::{GrantRoleOutcome, RevokeRoleOutcome, UserRoleRepository};
use crate::webhook_outbound::{
    OutboundEventData, OutboundEventInput, OutboundEventPublisher, OutboundEventType,
    RoleEventData, RoleEventSource, SubscriptionEventData,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelMode {
    DefaultCancel,
    ImmediateCancel,
}

pub struct SubscriptionService<R, P, U, C>
where
    R: PointsRepository + Send + Sync,
    P: crate::points::policies::PointsPolicy,
    U: UserRoleRepository,
    C: PermissionService,
{
    _points_service: Arc<PointsService<R, P>>,
    repo: Arc<R>,
    user_role_repository: Arc<U>,
    permission_service: Arc<C>,
    _grant_scheduler: Option<Arc<crate::points::services::GrantScheduler<R, P>>>,
    publisher: Arc<dyn OutboundEventPublisher>,
}

impl<R, P, U, C> SubscriptionService<R, P, U, C>
where
    R: PointsRepository + Send + Sync,
    P: crate::points::policies::PointsPolicy,
    U: UserRoleRepository,
    C: PermissionService,
{
    pub fn new(
        points_service: Arc<PointsService<R, P>>,
        repo: Arc<R>,
        user_role_repository: Arc<U>,
        permission_service: Arc<C>,
        grant_scheduler: Option<Arc<crate::points::services::GrantScheduler<R, P>>>,
        publisher: Arc<dyn OutboundEventPublisher>,
    ) -> Self {
        Self {
            _points_service: points_service,
            repo,
            user_role_repository,
            permission_service,
            _grant_scheduler: grant_scheduler,
            publisher,
        }
    }

    async fn grant_payment_roles(
        &self,
        realm_id: &str,
        user_id: Uuid,
        role_ids: &[Uuid],
        source_id: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(), CoreError> {
        let mut granted_events = Vec::new();
        for role_id in role_ids {
            match self
                .user_role_repository
                .grant_role_by_payment(realm_id, user_id, *role_id, None, source_id, expires_at)
                .await
            {
                // Granted-only emission keeps payment-event replays
                // (AlreadyExists) from fanning out duplicate role.granted.
                Ok(GrantRoleOutcome::Granted) => {
                    granted_events.push(OutboundEventInput {
                        realm_id: realm_id.to_string(),
                        event_type: OutboundEventType::RoleGranted,
                        occurred_at: Utc::now(),
                        data: OutboundEventData::Role(RoleEventData {
                            user_id,
                            role_id: *role_id,
                            role_name: None,
                            source: RoleEventSource::Payment,
                            expires_at,
                        }),
                    });
                }
                Ok(GrantRoleOutcome::AlreadyExists) => {}
                Err(error) => {
                    // Each grant commits independently; emit the events for
                    // the grants that landed before propagating, or a replay
                    // (AlreadyExists skip) would strand them forever.
                    if !granted_events.is_empty() {
                        self.publisher
                            .emit(std::mem::take(&mut granted_events))
                            .await;
                    }
                    return Err(error.into());
                }
            }
        }
        if !granted_events.is_empty() {
            self.publisher.emit(granted_events).await;
        }
        if !role_ids.is_empty()
            && let Err(error) = self
                .permission_service
                .invalidate_user_role_cache(realm_id, &user_id.to_string())
                .await
        {
            tracing::warn!(%realm_id, %user_id, %error, "failed to invalidate role cache after subscription grant");
        }
        Ok(())
    }

    /// Explicit direct-write quota command used only by the internal quota
    /// endpoint. It deliberately bypasses configured rules.
    pub async fn grant_quota_entitlement(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        source_type: QuotaSourceType,
        source_id: String,
        quota_windows: Vec<QuotaWindow>,
        effective_from: DateTime<Utc>,
        effective_until: Option<DateTime<Utc>>,
        idempotency_key: String,
    ) -> Result<PointsQuotaEntitlement, CoreError> {
        let now = now_utc();
        self.repo
            .grant_quota_entitlement_atomic(PointsQuotaEntitlement {
                id: generate_uuid_v7(),
                user_id,
                realm_id: realm_id.to_string(),
                bucket_id,
                credit_type,
                source_type,
                source_id,
                quota_windows,
                effective_from,
                effective_until,
                status: QuotaEntitlementStatus::Active,
                idempotency_key,
                distribution_event_id: None,
                distribution_rule_id: None,
                created_at: now,
                updated_at: now,
            })
            .await
    }

    pub async fn revoke_quota_entitlement(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        source_id: &str,
        revoke_at: DateTime<Utc>,
    ) -> Result<(), CoreError> {
        self.repo
            .revoke_quota_entitlement_atomic(
                realm_id,
                user_id,
                bucket_id,
                credit_type,
                source_id,
                revoke_at,
            )
            .await
    }

    /// Renewal is the only subscription-paid points path here. Initial
    /// subscription fulfillment is owned by the captured Payment Attempt flow
    /// (BE-D04), which must not be replaced by current Mapping rules.
    pub async fn handle_subscription_paid(
        &self,
        user_id: Uuid,
        subscription_id: Uuid,
        realm_id: &str,
        mapping: &EntitlementMapping,
        is_renewal: bool,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        _provider_event_id: String,
    ) -> Result<Vec<DistributionGrantResult>, CoreError> {
        if !mapping.enabled {
            // A disabled mapping skips points and roles, but the renewal
            // payment itself is still the documented subscription.renewed
            // trigger.
            self.emit_subscription_renewed(
                realm_id,
                user_id,
                subscription_id,
                &mapping.entitlement_key,
                period_start,
                period_end,
            )
            .await;
            return Ok(Vec::new());
        }
        let source_id = subscription_id.to_string();
        let results = if is_renewal {
            let event = DistributionEvent {
                realm_id: realm_id.to_string(),
                user_id,
                owner: DistributionRuleOwner::EntitlementMapping(mapping.id),
                trigger: DistributionTrigger::SubscriptionRenewal,
                event_key: event_key_for_subscription_period(
                    subscription_id,
                    &period_start.to_rfc3339(),
                ),
                source_id: source_id.clone(),
                effective_from: period_start,
                effective_until: Some(period_end),
            };
            self.repo
                .execute_distribution_event_atomic(
                    event,
                    DistributionRuleSelection::CurrentOwnerRules,
                )
                .await?
        } else {
            Vec::new()
        };

        self.grant_payment_roles(
            realm_id,
            user_id,
            &mapping.granted_role_ids,
            &source_id,
            Some(period_end),
        )
        .await?;

        // Renewal is a subscription lifecycle fact independent of the role
        // grant outcome — emitted even when the mapping grants no roles.
        if is_renewal {
            self.emit_subscription_renewed(
                realm_id,
                user_id,
                subscription_id,
                &mapping.entitlement_key,
                period_start,
                period_end,
            )
            .await;
        }
        Ok(results)
    }

    /// Post-commit best-effort `subscription.renewed` for a successful
    /// renewal payment.
    async fn emit_subscription_renewed(
        &self,
        realm_id: &str,
        user_id: Uuid,
        subscription_id: Uuid,
        entitlement_key: &str,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
    ) {
        self.publisher
            .emit(vec![OutboundEventInput {
                realm_id: realm_id.to_string(),
                event_type: OutboundEventType::SubscriptionRenewed,
                occurred_at: Utc::now(),
                data: OutboundEventData::Subscription(SubscriptionEventData {
                    user_id,
                    subscription_id: Some(subscription_id),
                    entitlement_key: Some(entitlement_key.to_string()),
                    current_period_start: Some(period_start),
                    current_period_end: Some(period_end),
                }),
            }])
            .await;
    }

    /// Revoke all currently remaining fixed/quota results for the subscription
    /// and execute the new Mapping's upgrade rules in one repository
    /// transaction. Replays return the original upgrade results without
    /// repeating either half.
    pub async fn handle_subscription_upgrade(
        &self,
        user_id: Uuid,
        realm_id: &str,
        subscription_id: Uuid,
        new_mapping: &EntitlementMapping,
        period_end: DateTime<Utc>,
        provider_event_id: &str,
    ) -> Result<Vec<DistributionGrantResult>, CoreError> {
        if !new_mapping.enabled {
            return Ok(Vec::new());
        }
        let source_id = subscription_id.to_string();
        let now = Utc::now();
        let event = DistributionEvent {
            realm_id: realm_id.to_string(),
            user_id,
            owner: DistributionRuleOwner::EntitlementMapping(new_mapping.id),
            trigger: DistributionTrigger::SubscriptionUpgrade,
            event_key: event_key_for_subscription_upgrade(subscription_id, provider_event_id),
            source_id: source_id.clone(),
            effective_from: now,
            effective_until: Some(period_end),
        };
        self.repo
            .replace_distribution_source_atomic(
                &source_id,
                RevocationType::UpgradeRevoke,
                "Subscription upgrade".to_string(),
                event,
                DistributionRuleSelection::CurrentOwnerRules,
            )
            .await
    }

    /// A downgrade never changes the current period's grants. Persisting the
    /// new Mapping on the Subscription is sufficient; its rules are selected
    /// only by the next renewal event.
    pub async fn handle_subscription_downgrade(
        &self,
        user_id: Uuid,
        subscription_id: Uuid,
        realm_id: &str,
        old_mapping: &EntitlementMapping,
        new_mapping: &EntitlementMapping,
    ) -> Result<(), CoreError> {
        tracing::info!(
            %realm_id,
            %user_id,
            %subscription_id,
            old_mapping_id = %old_mapping.id,
            new_mapping_id = %new_mapping.id,
            "subscription downgrade deferred until renewal"
        );
        Ok(())
    }

    pub async fn handle_subscription_cancel(
        &self,
        user_id: Uuid,
        realm_id: &str,
        subscription_id: Uuid,
        cancel_mode: CancelMode,
        _period_end: Option<DateTime<Utc>>,
        _entitlement_key: Option<&str>,
    ) -> Result<RevokePointsOutput, CoreError> {
        if cancel_mode == CancelMode::DefaultCancel {
            return Ok(RevokePointsOutput::empty());
        }

        let source_id = subscription_id.to_string();
        let output = self
            .repo
            .revoke_distribution_source_atomic(
                realm_id,
                user_id,
                &source_id,
                RevocationType::CancelRevoke,
                "Subscription cancelled".to_string(),
                format!("subscription-cancel:{subscription_id}"),
            )
            .await?;

        match self
            .user_role_repository
            .revoke_roles_by_payment_source(realm_id, user_id, &source_id)
            .await?
        {
            // The delete's own RETURNING is the ground truth of what was
            // removed — no snapshot/delete race window can strand a grant
            // without its role.revoked.
            RevokeRoleOutcome::Revoked(revoked_role_ids) => {
                let revoked_events = revoked_role_ids
                    .into_iter()
                    .map(|role_id| OutboundEventInput {
                        realm_id: realm_id.to_string(),
                        event_type: OutboundEventType::RoleRevoked,
                        occurred_at: Utc::now(),
                        data: OutboundEventData::Role(RoleEventData {
                            user_id,
                            role_id,
                            role_name: None,
                            source: RoleEventSource::Payment,
                            expires_at: None,
                        }),
                    })
                    .collect::<Vec<_>>();
                if !revoked_events.is_empty() {
                    self.publisher.emit(revoked_events).await;
                }
                if let Err(error) = self
                    .permission_service
                    .invalidate_user_role_cache(realm_id, &user_id.to_string())
                    .await
                {
                    tracing::warn!(%realm_id, %user_id, %error, "failed to invalidate role cache after subscription revoke");
                }
            }
            RevokeRoleOutcome::NotFound => {}
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renewal_and_upgrade_have_distinct_stable_event_keys() {
        let subscription_id = Uuid::now_v7();
        let renewal =
            event_key_for_subscription_period(subscription_id, "2026-07-29T00:00:00+00:00");
        let upgrade = event_key_for_subscription_upgrade(subscription_id, "evt_upgrade_1");
        assert_ne!(renewal, upgrade);
        assert_eq!(
            renewal,
            format!("subscription:{subscription_id}:period:2026-07-29T00:00:00+00:00")
        );
        assert_eq!(
            upgrade,
            format!("subscription:{subscription_id}:upgrade:evt_upgrade_1")
        );
    }
}
