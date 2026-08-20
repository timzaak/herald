use chrono::{DateTime, Utc};
use sea_orm::DatabaseTransaction;
use serde_json::Value;
use uuid::Uuid;

use herald_api_base::application::http::state::AppState;
use herald_core::domain::billing::entities::BillingType;
use herald_core::domain::billing::{
    ACTOR_WEBHOOK, BillingRepository, EntitlementMapping, HistoryEventType, Subscription,
    SubscriptionHistoryEvent, calculate_changes, serialize_subscription_state,
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::points::DistributionPolicy;
use herald_core::infrastructure::billing::PostgresBillingRepository;

pub(crate) struct SyncSubscriptionInput {
    pub provider: &'static str,
    pub realm_id: String,
    pub user_id: Option<Uuid>,
    pub external_subscription_id: String,
    pub external_product_id: String,
    pub client_app_id: Option<Uuid>,
    pub entitlement_key: String,
    pub external_price_id: Option<String>,
    pub provider_metadata: Option<Value>,
    pub status: herald_core::domain::billing::SubscriptionStatus,
    pub current_period_start: Option<DateTime<Utc>>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub cancel_at: Option<DateTime<Utc>>,
    /// Pre-fetched subscription to avoid a redundant DB lookup.
    /// When provided, `sync_subscription` skips the `find_by_external_subscription_id` query.
    pub existing_subscription: Option<Subscription>,
}

pub(crate) async fn save_subscription_history(
    app_state: &AppState,
    previous: Option<&Subscription>,
    current: &Subscription,
    event_type: HistoryEventType,
) -> Result<(), CoreError> {
    let history_event = SubscriptionHistoryEvent {
        id: Uuid::now_v7().to_string(),
        subscription_id: current.id,
        event_type,
        timestamp: Utc::now(),
        actor: Some(ACTOR_WEBHOOK.to_string()),
        changes: previous.map(|previous| calculate_changes(previous, current)),
        previous_state: previous.map(serialize_subscription_state),
        new_state: Some(serialize_subscription_state(current)),
        realm_id: current.realm_id.clone(),
        created_at: Utc::now(),
    };

    app_state
        .billing_repository
        .save_history_event(history_event)
        .await?;

    Ok(())
}

pub(crate) async fn save_subscription_history_in_txn(
    billing_repo: &PostgresBillingRepository,
    txn: &DatabaseTransaction,
    previous: Option<&Subscription>,
    current: &Subscription,
    event_type: HistoryEventType,
) -> Result<(), CoreError> {
    let history_event = SubscriptionHistoryEvent {
        id: Uuid::now_v7().to_string(),
        subscription_id: current.id,
        event_type,
        timestamp: Utc::now(),
        actor: Some(ACTOR_WEBHOOK.to_string()),
        changes: previous.map(|previous| calculate_changes(previous, current)),
        previous_state: previous.map(serialize_subscription_state),
        new_state: Some(serialize_subscription_state(current)),
        realm_id: current.realm_id.clone(),
        created_at: Utc::now(),
    };

    let _ = billing_repo;
    PostgresBillingRepository::save_history_event_conn(txn, history_event).await?;

    Ok(())
}

/// Resolved entitlement for a webhook event: the projection `entitlement_key`
/// plus the **price-level** mapping that drives points strategy + billing_type.
/// `mapping` carries the strategy fields consumed for
/// price-aware points issuance.
pub(crate) struct ResolvedEntitlement {
    /// Stable projection key written to `subscription.entitlement_key`.
    pub entitlement_key: String,
    /// Price-level mapping resolved for this webhook event. Strategy source for
    /// points issuance (US-EM-008).
    pub mapping: EntitlementMapping,
}

pub(crate) async fn mapping_rule_value(
    app_state: &AppState,
    realm_id: &str,
    mapping_id: Uuid,
) -> Result<i64, CoreError> {
    let rules = app_state
        .billing_repository
        .find_mapping_rules(realm_id, mapping_id)
        .await?;
    Ok(rules
        .into_iter()
        .filter(|rule| rule.enabled)
        .map(|rule| match rule.policy {
            DistributionPolicy::Fixed { amount, .. } => amount,
            DistributionPolicy::Quota { windows } => {
                windows.into_iter().map(|window| window.limit).sum()
            }
        })
        .sum())
}

/// Fail-loud resolution error. Never silently degrades — callers
/// surface these as diagnostics so admins can see unresolved webhooks.
pub(crate) enum ResolveError {
    /// Multiple price mappings match and none could be uniquely selected
    /// (metadata missing, multi-price product, price not derivable).
    AmbiguousPrice { provider: String, product: String },
    /// No mapping row exists for the given (provider, product[, price]).
    NoMapping {
        provider: String,
        product: String,
        price: Option<String>,
    },
}

impl ResolveError {
    /// Render the error as an admin-visible diagnostic message.
    fn message(&self) -> String {
        match self {
            ResolveError::AmbiguousPrice { provider, product } => format!(
                "Webhook for provider '{provider}' product '{product}' could not resolve a unique price; entitlement_key missing and price ambiguous"
            ),
            ResolveError::NoMapping {
                provider,
                product,
                price,
            } => {
                let price_display = price.as_deref().unwrap_or("<null>");
                format!(
                    "No entitlement mapping for provider '{provider}' product '{product}' price '{price_display}'"
                )
            }
        }
    }
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl From<ResolveError> for CoreError {
    fn from(err: ResolveError) -> Self {
        // Fail loud as a 400 — resolution failure is a configuration/data gap the
        // admin must see, not a silent skip.
        CoreError::BadRequest(err.message())
    }
}

/// Price-aware webhook entitlement resolution (US-EM-008).
///
/// Resolution chain (encodes WHY each branch matters):
/// 1. `projection_key` candidate := metadata `herald_entitlement_key` (if non-empty).
/// 2. Look up mapping by `(realm, provider, product, price)`:
///    - Stripe: `Some(price_id)` extracted from `items[0].price.id`.
///    - Creem : `None` (price-less; repository maps to `IS NULL`).
/// 3. Decide:
///    - a) Unique hit: projection_key = metadata key OR mapping.entitlement_key;
///      strategy/billing_type from this mapping (price-level, kills shared-key ambiguity).
///    - b) Miss + metadata has key: re-locate strategy mapping by
///      `(entitlement_key, webhook price)`; hit use it, miss `AmbiguousPrice`.
///      Step-2 used webhook (product, price); on miss the product may be
///      mis-derived, so we re-anchor on the authoritative metadata key + price.
///    - c) Miss + no metadata: if `(provider, product)` has exactly 1 row use it;
///      >1 row `AmbiguousPrice` (fail loud); 0 rows `NoMapping`.
/// 4. Errors are logged as diagnostics; never silently degrades.
pub(crate) async fn resolve_entitlement_mapping(
    app_state: &AppState,
    realm_id: &str,
    provider: &str,
    external_product_id: &str,
    external_price_id: Option<&str>,
    metadata_entitlement_key: Option<&str>,
) -> Result<ResolvedEntitlement, ResolveError> {
    // Step 1: metadata projection-key candidate
    let metadata_key = metadata_entitlement_key.filter(|k| !k.is_empty());

    // Step 2: lookup by (realm, provider, product, price)
    let mapping = app_state
        .billing_repository
        .find_entitlement_mapping_by_provider_product_price(
            realm_id,
            provider,
            external_product_id,
            external_price_id,
        )
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                provider = %provider,
                external_product_id = %external_product_id,
                external_price_id = ?external_price_id,
                error = %e,
                "Webhook resolution DB error during (provider, product, price) lookup"
            );
            ResolveError::NoMapping {
                provider: provider.to_string(),
                product: external_product_id.to_string(),
                price: external_price_id.map(str::to_string),
            }
        })?;

    // Step 3a: unique price-level hit
    if let Some(mapping) = mapping {
        let projection_key = metadata_key
            .map(str::to_string)
            .unwrap_or_else(|| mapping.entitlement_key.clone());
        tracing::info!(
            realm_id = %realm_id,
            provider = %provider,
            external_product_id = %external_product_id,
            external_price_id = ?external_price_id,
            entitlement_key = %projection_key,
            mapping_id = %mapping.id,
            "Resolved entitlement mapping by (provider, product, price)"
        );
        return Ok(ResolvedEntitlement {
            entitlement_key: projection_key,
            mapping,
        });
    }

    // Step 3b: miss + metadata key present → re-locate strategy by (key, price)
    if let Some(key) = metadata_key {
        let relocated = app_state
            .billing_repository
            .find_entitlement_mapping_by_key_price(realm_id, key, external_price_id)
            .await
            .map_err(|e| {
                tracing::error!(
                    realm_id = %realm_id,
                    entitlement_key = %key,
                    external_price_id = ?external_price_id,
                    error = %e,
                    "Webhook resolution DB error during (entitlement_key, price) re-location"
                );
                ResolveError::AmbiguousPrice {
                    provider: provider.to_string(),
                    product: external_product_id.to_string(),
                }
            })?;

        return match relocated {
            Some(mapping) => {
                tracing::info!(
                    realm_id = %realm_id,
                    provider = %provider,
                    external_product_id = %external_product_id,
                    external_price_id = ?external_price_id,
                    entitlement_key = %key,
                    mapping_id = %mapping.id,
                    "Resolved entitlement mapping by (entitlement_key, price) re-location"
                );
                Ok(ResolvedEntitlement {
                    entitlement_key: key.to_string(),
                    mapping,
                })
            }
            None => {
                tracing::error!(
                    realm_id = %realm_id,
                    provider = %provider,
                    external_product_id = %external_product_id,
                    external_price_id = ?external_price_id,
                    entitlement_key = %key,
                    "Webhook resolution failed: (provider, product, price) miss and (entitlement_key, price) miss"
                );
                Err(ResolveError::AmbiguousPrice {
                    provider: provider.to_string(),
                    product: external_product_id.to_string(),
                })
            }
        };
    }

    // Step 3c: miss + no metadata → disambiguate by (provider, product) row count
    let rows = app_state
        .billing_repository
        .list_entitlement_mappings_by_provider_product(realm_id, provider, external_product_id)
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                provider = %provider,
                external_product_id = %external_product_id,
                error = %e,
                "Webhook resolution DB error during (provider, product) listing"
            );
            ResolveError::NoMapping {
                provider: provider.to_string(),
                product: external_product_id.to_string(),
                price: external_price_id.map(str::to_string),
            }
        })?;

    match rows.len() {
        1 => {
            let mapping = rows.into_iter().next().expect("exactly one row");
            let projection_key = mapping.entitlement_key.clone();
            tracing::info!(
                realm_id = %realm_id,
                provider = %provider,
                external_product_id = %external_product_id,
                external_price_id = ?external_price_id,
                entitlement_key = %projection_key,
                mapping_id = %mapping.id,
                "Resolved entitlement mapping by (provider, product) single-row fallback"
            );
            Ok(ResolvedEntitlement {
                entitlement_key: projection_key,
                mapping,
            })
        }
        0 => {
            tracing::error!(
                realm_id = %realm_id,
                provider = %provider,
                external_product_id = %external_product_id,
                external_price_id = ?external_price_id,
                "Webhook resolution failed: no mapping rows for (provider, product)"
            );
            Err(ResolveError::NoMapping {
                provider: provider.to_string(),
                product: external_product_id.to_string(),
                price: external_price_id.map(str::to_string),
            })
        }
        _ => {
            tracing::error!(
                realm_id = %realm_id,
                provider = %provider,
                external_product_id = %external_product_id,
                external_price_id = ?external_price_id,
                row_count = rows.len(),
                "Webhook resolution failed: {} mappings for (provider, product) and no metadata key to disambiguate",
                rows.len()
            );
            Err(ResolveError::AmbiguousPrice {
                provider: provider.to_string(),
                product: external_product_id.to_string(),
            })
        }
    }
}

pub(crate) async fn sync_subscription(
    app_state: &AppState,
    input: SyncSubscriptionInput,
) -> Result<Option<(Subscription, Option<Subscription>)>, CoreError> {
    let SyncSubscriptionInput {
        provider,
        realm_id,
        user_id,
        external_subscription_id,
        external_product_id,
        client_app_id,
        entitlement_key,
        external_price_id,
        provider_metadata,
        status,
        current_period_start,
        current_period_end,
        cancel_at_period_end,
        cancel_at,
        existing_subscription,
    } = input;

    let existing = {
        if let Some(prefetched) = existing_subscription {
            // Use the pre-fetched subscription to avoid a redundant DB query
            Some(prefetched)
        } else {
            let existing = app_state
                .billing_repository
                .find_by_external_subscription_id(&external_subscription_id, provider)
                .await?;

            if existing.is_some() {
                existing
            } else if let Some(client_app_id) = client_app_id {
                app_state
                    .billing_repository
                    .find_subscription_by_client_app_id(client_app_id)
                    .await?
            } else {
                None
            }
        }
    };

    if existing.is_none() && client_app_id.is_none() && external_subscription_id.is_empty() {
        return Ok(None);
    }

    if external_subscription_id.is_empty() {
        return Err(CoreError::BadRequest(
            "Missing external_subscription_id".to_string(),
        ));
    }

    // Realm binding: the external-id lookup above is deliberately realm-free
    // (the provider id is globally unique per provider account). An event
    // signed for this realm must never mutate a subscription row belonging to
    // another realm — e.g. two realms misconfigured to share one provider
    // account, or a leaked webhook secret.
    if let Some(sub) = existing.as_ref()
        && sub.realm_id != realm_id
    {
        return Err(CoreError::Forbidden(format!(
            "subscription {external_subscription_id} does not belong to realm {realm_id}"
        )));
    }

    let now = Utc::now();

    if let Some(mut subscription) = existing {
        let previous = subscription.clone();

        subscription.external_subscription_id = external_subscription_id.clone();
        subscription.external_product_id = external_product_id.clone();
        subscription.payment_provider = provider.to_string();
        subscription.status = status;
        subscription.entitlement_key = if entitlement_key.is_empty() {
            subscription.entitlement_key.clone()
        } else {
            entitlement_key
        };
        subscription.external_price_id = external_price_id.or(subscription.external_price_id);
        subscription.provider_metadata = provider_metadata.or(subscription.provider_metadata);
        subscription.synced_at = Some(now);
        if let Some(user_id) = user_id {
            subscription.user_id = user_id;
        }
        subscription.current_period_start = current_period_start.or(previous.current_period_start);
        subscription.current_period_end = current_period_end.or(previous.current_period_end);
        subscription.cancel_at_period_end = cancel_at_period_end;
        subscription.cancel_at = cancel_at;
        if client_app_id.is_some() {
            subscription.client_app_id = client_app_id;
        }
        subscription.updated_at = now;

        let updated = app_state
            .billing_repository
            .update_subscription(subscription)
            .await?;
        Ok(Some((updated, Some(previous))))
    } else {
        let user_id = user_id.ok_or_else(|| {
            CoreError::BadRequest("Missing user_id for subscription creation".to_string())
        })?;
        let subscription = Subscription {
            id: Uuid::now_v7(),
            realm_id,
            user_id,
            external_subscription_id,
            external_product_id,
            payment_provider: provider.to_string(),
            status,
            entitlement_key,
            // Stripe/Creem webhook sync is the recurring subscription path
            // (non_renewing is fulfilled via the purchase path, not webhook
            billing_type: BillingType::Recurring,
            external_price_id,
            provider_metadata,
            synced_at: Some(now),
            current_period_start,
            current_period_end,
            cancel_at_period_end,
            client_app_id,
            cancel_at,
            created_at: now,
            updated_at: now,
        };

        let created = app_state
            .billing_repository
            .create_subscription(subscription)
            .await?;
        Ok(Some((created, None)))
    }
}

pub(crate) async fn sync_subscription_in_txn(
    txn: &DatabaseTransaction,
    input: SyncSubscriptionInput,
) -> Result<Option<(Subscription, Option<Subscription>)>, CoreError> {
    let SyncSubscriptionInput {
        provider,
        realm_id,
        user_id,
        external_subscription_id,
        external_product_id,
        client_app_id,
        entitlement_key,
        external_price_id,
        provider_metadata,
        status,
        current_period_start,
        current_period_end,
        cancel_at_period_end,
        cancel_at,
        existing_subscription,
    } = input;

    let existing = {
        if let Some(prefetched) = existing_subscription {
            Some(prefetched)
        } else {
            let existing = PostgresBillingRepository::find_by_external_subscription_id_conn(
                txn,
                &external_subscription_id,
                provider,
            )
            .await?;

            if existing.is_some() {
                existing
            } else if let Some(client_app_id) = client_app_id {
                PostgresBillingRepository::find_subscription_by_client_app_id_conn(
                    txn,
                    client_app_id,
                )
                .await?
            } else {
                None
            }
        }
    };

    if existing.is_none() && client_app_id.is_none() && external_subscription_id.is_empty() {
        return Ok(None);
    }

    if external_subscription_id.is_empty() {
        return Err(CoreError::BadRequest(
            "Missing external_subscription_id".to_string(),
        ));
    }

    // Realm binding: the external-id lookup above is deliberately realm-free
    // (the provider id is globally unique per provider account). An event
    // signed for this realm must never mutate a subscription row belonging to
    // another realm — e.g. two realms misconfigured to share one provider
    // account, or a leaked webhook secret.
    if let Some(sub) = existing.as_ref()
        && sub.realm_id != realm_id
    {
        return Err(CoreError::Forbidden(format!(
            "subscription {external_subscription_id} does not belong to realm {realm_id}"
        )));
    }

    let now = Utc::now();

    if let Some(mut subscription) = existing {
        let previous = subscription.clone();

        subscription.external_subscription_id = external_subscription_id.clone();
        subscription.external_product_id = external_product_id.clone();
        subscription.payment_provider = provider.to_string();
        subscription.status = status;
        subscription.entitlement_key = if entitlement_key.is_empty() {
            subscription.entitlement_key.clone()
        } else {
            entitlement_key
        };
        subscription.external_price_id = external_price_id.or(subscription.external_price_id);
        subscription.provider_metadata = provider_metadata.or(subscription.provider_metadata);
        subscription.synced_at = Some(now);
        if let Some(user_id) = user_id {
            subscription.user_id = user_id;
        }
        subscription.current_period_start = current_period_start.or(previous.current_period_start);
        subscription.current_period_end = current_period_end.or(previous.current_period_end);
        subscription.cancel_at_period_end = cancel_at_period_end;
        subscription.cancel_at = cancel_at;
        if client_app_id.is_some() {
            subscription.client_app_id = client_app_id;
        }
        subscription.updated_at = now;

        let updated =
            PostgresBillingRepository::update_subscription_conn(txn, subscription).await?;
        Ok(Some((updated, Some(previous))))
    } else {
        let user_id = user_id.ok_or_else(|| {
            CoreError::BadRequest("Missing user_id for subscription creation".to_string())
        })?;
        let subscription = Subscription {
            id: Uuid::now_v7(),
            realm_id,
            user_id,
            external_subscription_id,
            external_product_id,
            payment_provider: provider.to_string(),
            status,
            entitlement_key,
            // Stripe/Creem webhook sync is the recurring subscription path
            // (non_renewing is fulfilled via the purchase path, not webhook
            billing_type: BillingType::Recurring,
            external_price_id,
            provider_metadata,
            synced_at: Some(now),
            current_period_start,
            current_period_end,
            cancel_at_period_end,
            client_app_id,
            cancel_at,
            created_at: now,
            updated_at: now,
        };

        let created =
            PostgresBillingRepository::create_subscription_conn(txn, subscription).await?;
        Ok(Some((created, None)))
    }
}
