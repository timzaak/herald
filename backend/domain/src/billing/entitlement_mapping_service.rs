use std::sync::Arc;

use crate::authentication::Identity;
use crate::billing::entities::EntitlementMapping;
use crate::billing::policies::BillingPolicy;
use crate::billing::ports::BillingRepository;
use crate::common::entities::app_errors::CoreError;
use crate::common::policies::ensure_policy;
use crate::points::{DistributionRuleOwner, RuleUpsert, validate_rule_for_owner};

///
/// The generic `POST /api/bill/{realmId}/entitlement-mappings` endpoint accepts
/// this shape for any provider (IAP, Stripe, Creem). Required identity fields
/// (`payment_provider`, `external_product_id`, `entitlement_key`,
/// `billing_type`) are non-optional; everything else defaults. The
/// `uq_pem_realm_provider_product_price` unique constraint is enforced by the
/// repository and surfaces as a 409 conflict at the HTTP layer.
///
/// Points distribution is configured via `point_rules` (an upsert set owned by
/// the new mapping); an empty array is a valid "no points grant" mapping
/// (role-only / pure payment record).
#[derive(Debug, Clone)]
pub struct CreateEntitlementMappingInput {
    pub payment_provider: String,
    pub external_product_id: String,
    /// Stripe Price ID for Stripe; `None` for IAP / Creem (price-less).
    pub external_price_id: Option<String>,
    pub entitlement_key: String,
    pub billing_type: crate::billing::entities::BillingType,
    /// Required when `billing_type == Recurring`.
    pub billing_period: Option<String>,
    /// Non-renewing service-period length (days). Required (`>= 1`) when
    pub service_duration_days: Option<i64>,
    /// Initial points distribution rules owned by the new mapping (upsert set;
    /// `None` and empty are both valid — no points grant). Each rule is
    /// validated against the mapping's `billing_type` before persistence.
    pub point_rules: Vec<RuleUpsert>,
    /// Roles auto-granted on payment success.
    pub granted_role_ids: Vec<uuid::Uuid>,
    /// Manually configured price in minor units (e.g. fen for CNY). WeChat
    /// only — it has no hosted catalog to sync from, so these land in the
    /// same `provider_product_info` keys the Stripe/Creem sync writes.
    /// Rejected for every other provider.
    pub price: Option<i64>,
    /// ISO 4217 currency code for the manual price. WeChat only.
    pub currency: Option<String>,
    pub enabled: bool,
}

pub struct EntitlementMappingService<R, P>
where
    R: BillingRepository,
    P: BillingPolicy,
{
    repository: Arc<R>,
    policy: Arc<P>,
}

impl<R, P> EntitlementMappingService<R, P>
where
    R: BillingRepository + Send + Sync,
    P: BillingPolicy,
{
    pub fn new(repository: Arc<R>, policy: Arc<P>) -> Self {
        Self { repository, policy }
    }

    pub async fn list_mappings(
        &self,
        identity: Identity,
        realm_id: &str,
        payment_provider: Option<&str>,
        enabled: Option<bool>,
        page: Option<u64>,
        page_size: Option<u64>,
    ) -> Result<(Vec<EntitlementMapping>, u64), CoreError> {
        ensure_policy(
            self.policy.can_view_billing(identity.clone()).await,
            "Insufficient permissions to view billing",
        )?;

        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot access billing from a different realm".to_string(),
            ));
        }

        self.repository
            .list_entitlement_mappings(realm_id, payment_provider, enabled, page, page_size)
            .await
    }

    pub async fn get_mapping(
        &self,
        identity: Identity,
        realm_id: &str,
        mapping_id: uuid::Uuid,
    ) -> Result<EntitlementMapping, CoreError> {
        ensure_policy(
            self.policy.can_view_billing(identity.clone()).await,
            "Insufficient permissions to view billing",
        )?;

        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot access billing from a different realm".to_string(),
            ));
        }

        let mapping = self
            .repository
            .find_entitlement_mapping_by_id(mapping_id)
            .await?
            .ok_or(CoreError::EntitlementMappingNotFound)?;

        if mapping.realm_id != realm_id {
            return Err(CoreError::EntitlementMappingNotFound);
        }

        Ok(mapping)
    }

    ///
    /// Caller is responsible for:
    /// - enforcing `billing.manage` (and `points.manage` when credit-strategy
    ///   fields are present) before invoking,
    /// - validating `granted_role_ids` realm membership,
    /// - `billing_period` presence when `billing_type == Recurring`.
    ///
    /// The `uq_pem_realm_provider_product_price` unique constraint is enforced
    /// by the repository; a violation surfaces as `CoreError::Conflict` and the
    /// handler maps it to HTTP 409.
    pub async fn create_mapping(
        &self,
        identity: Identity,
        realm_id: &str,
        input: CreateEntitlementMappingInput,
    ) -> Result<EntitlementMapping, CoreError> {
        ensure_policy(
            self.policy.can_manage_billing(identity.clone()).await,
            "Insufficient permissions to manage billing",
        )?;

        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot access billing from a different realm".to_string(),
            ));
        }

        Self::validate_entitlement_key(&input.entitlement_key)?;
        if !matches!(
            input.payment_provider.as_str(),
            "stripe" | "creem" | "apple" | "google" | "wechat"
        ) {
            return Err(CoreError::BadRequest(format!(
                "Unsupported payment provider: {}",
                input.payment_provider
            )));
        }
        if matches!(
            input.billing_type,
            crate::billing::entities::BillingType::Recurring
        ) && input.billing_period.as_deref().unwrap_or("").is_empty()
        {
            return Err(CoreError::BadRequest(
                "billing_period is required for recurring billing_type".to_string(),
            ));
        }
        // Channel × billing_type whitelist (PRD pay_model §2.2 — Herald does
        // not extend the Stripe/Creem product forms; WeChat has no
        // auto-renewal, PRD models WeChat subscriptions as non_renewing):
        // non-renewing subscriptions are an IAP product form, and a
        // Stripe/Creem non_renewing mapping could never be fulfilled as
        // configured — the next provider product sync would clobber its
        // billing_type back to recurring while the duration field lingered,
        // leaving an owned-by-nobody hybrid configuration.
        validate_provider_billing_type(&input.payment_provider, &input.billing_type)?;

        //   - service_duration_days must be present and >= 1 (US-PM-002 scene 2 → 400)
        //   - billing_period must be empty (mutually exclusive billing semantics → 400)
        validate_non_renewing(
            &input.billing_type,
            input.service_duration_days,
            input.billing_period.as_deref(),
        )?;

        // Resolve the manual price (WeChat only) before the struct below
        // consumes `input.entitlement_key`.
        let provider_product_info = validate_manual_price(
            &input.payment_provider,
            input.price,
            input.currency.as_deref(),
        )?
        .map(|mut info| {
            // `name` backs the purchase-page display_name; WeChat has
            // no catalog name to use, so seed it from the key.
            info.as_object_mut()
                .unwrap()
                .insert("name".to_string(), serde_json::json!(input.entitlement_key));
            info
        });

        let now = chrono::Utc::now();
        // Only non_renewing carries a service duration; other types store None
        // regardless of the input (the field is ignored for them). Resolve
        // before moving `input.billing_type` into the struct below.
        let billing_type = input.billing_type;
        let service_duration_days = match billing_type {
            crate::billing::entities::BillingType::NonRenewing => input.service_duration_days,
            _ => None,
        };
        // Validate each rule against the mapping's billing type before
        // persistence. Invalid combinations surface as a stable 400 from the
        // domain validator; the repository upsert is not reached.
        for rule in &input.point_rules {
            let resolved = rule.clone().into_rule_for_owner(
                realm_id,
                DistributionRuleOwner::EntitlementMapping(uuid::Uuid::nil()),
            );
            validate_rule_for_owner(&resolved, Some(billing_type.clone()))?;
        }
        let mapping = EntitlementMapping {
            id: uuid::Uuid::now_v7(),
            realm_id: realm_id.to_string(),
            payment_provider: input.payment_provider,
            external_product_id: input.external_product_id,
            external_price_id: input.external_price_id,
            entitlement_key: input.entitlement_key,
            billing_type: Some(billing_type),
            billing_period: input.billing_period,
            service_duration_days,
            enabled: input.enabled,
            provider_product_info,
            granted_role_ids: input.granted_role_ids,
            synced_at: None,
            created_at: now,
            updated_at: now,
        };

        self.repository
            .create_entitlement_mapping_with_rules(mapping, input.point_rules)
            .await
    }

    pub async fn find_mapping_by_provider_product(
        &self,
        realm_id: &str,
        payment_provider: &str,
        external_product_id: &str,
    ) -> Result<Option<EntitlementMapping>, CoreError> {
        self.repository
            .find_entitlement_mapping_by_provider_product_price(
                realm_id,
                payment_provider,
                external_product_id,
                None,
            )
            .await
    }

    pub async fn find_mapping_by_entitlement_key(
        &self,
        realm_id: &str,
        entitlement_key: &str,
    ) -> Result<Option<EntitlementMapping>, CoreError> {
        self.repository
            .find_entitlement_mapping_by_key(realm_id, entitlement_key)
            .await
    }

    /// Validate entitlement_key format: [a-z0-9-], length 1-64
    fn validate_entitlement_key(key: &str) -> Result<(), CoreError> {
        if key.is_empty() || key.len() > 64 {
            return Err(CoreError::BadRequest(
                "entitlement_key must be 1-64 characters".to_string(),
            ));
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(CoreError::BadRequest(
                "entitlement_key must match [a-z0-9-]".to_string(),
            ));
        }
        Ok(())
    }
}

/// Validate the non-renewing billing-type invariants
///   - when `billing_type == NonRenewing`, `service_duration_days` must be
///     `Some(>= 1)` (US-PM-002 scene 2 → 400) and `billing_period` must be
///     empty (mutually exclusive billing semantics → 400);
///   - for other billing types this is a no-op (their duration/period rules
///     are enforced separately).
///
/// Pure/free function so the rule is unit-testable without a repository or
/// policy mock (avoids requiring the service's `R`/`P` generics to be inferred).
/// `create_mapping` routes through it. The PATCH handler applies the equivalent
/// resolved check inline against the stored mapping (3-state input), since it
/// must reconcile `service_duration_days` against the existing value.
fn validate_non_renewing(
    billing_type: &crate::billing::entities::BillingType,
    service_duration_days: Option<i64>,
    billing_period: Option<&str>,
) -> Result<(), CoreError> {
    if !matches!(
        billing_type,
        crate::billing::entities::BillingType::NonRenewing
    ) {
        return Ok(());
    }
    match service_duration_days {
        Some(days) if days >= 1 => {}
        _ => {
            return Err(CoreError::BadRequest(
                "service_duration_days is required and must be >= 1 for non_renewing billing_type"
                    .to_string(),
            ));
        }
    }
    if !billing_period.unwrap_or("").is_empty() {
        return Err(CoreError::BadRequest(
            "billing_period must be empty for non_renewing billing_type".to_string(),
        ));
    }
    Ok(())
}

/// Channel × billing_type whitelist:
///   - WeChat has no auto-renewal (merchant-initiated deduction is a separate
///     future feature), so a recurring mapping could never be fulfilled as
///     configured — PRD models WeChat subscriptions as non_renewing;
///   - non-renewing subscriptions are an IAP product form (plus the WeChat
///     non_renewing modeling). PRD pay_model §2.2 does not extend the
///     Stripe/Creem product forms, and a non_renewing mapping there would be
///     clobbered back to recurring by the next provider product sync while
///     the duration field lingered.
///
/// Pure/free function for the same testability reason as
/// `validate_non_renewing`; `create_mapping` routes through it. The PATCH
/// path is unaffected — billing_type is immutable on update.
fn validate_provider_billing_type(
    payment_provider: &str,
    billing_type: &crate::billing::entities::BillingType,
) -> Result<(), CoreError> {
    if payment_provider == "wechat"
        && matches!(
            billing_type,
            crate::billing::entities::BillingType::Recurring
        )
    {
        return Err(CoreError::BadRequest(
            "recurring billing_type is not supported for WeChat; use non_renewing".to_string(),
        ));
    }
    if matches!(payment_provider, "stripe" | "creem")
        && matches!(
            billing_type,
            crate::billing::entities::BillingType::NonRenewing
        )
    {
        return Err(CoreError::BadRequest(
            "non_renewing billing_type is not supported for Stripe/Creem; it is an IAP/WeChat product form"
                .to_string(),
        ));
    }
    Ok(())
}

///
/// WeChat is the only provider whose mapping price is configured by hand
/// (no hosted catalog to sync from), so the price/currency the caller sends
/// must land in `provider_product_info` using the same keys the Stripe/Creem
/// sync writes. Every other provider rejects manual price fields outright —
/// their price truth lives in the provider catalog.
///
/// Pure/free function for the same testability reason as
/// `validate_non_renewing`. Returns the JSONB fragment (without `name`; the
/// caller seeds it) or `None` when the provider is not WeChat and no manual
/// price was supplied.
fn validate_manual_price(
    payment_provider: &str,
    price: Option<i64>,
    currency: Option<&str>,
) -> Result<Option<serde_json::Value>, CoreError> {
    if payment_provider != "wechat" {
        if price.is_some() || currency.is_some() {
            return Err(CoreError::BadRequest(
                "price/currency can only be configured for WeChat mappings".to_string(),
            ));
        }
        return Ok(None);
    }
    // A WeChat mapping without a positive price can never produce a valid
    // order (the create-order call requires a positive amount), so fail at
    // write time instead of at checkout.
    let price = price.filter(|&p| p >= 1).ok_or_else(|| {
        CoreError::BadRequest(
            "price (minor units) is required and must be >= 1 for WeChat mappings".to_string(),
        )
    })?;
    let currency = currency.ok_or_else(|| {
        CoreError::BadRequest("currency is required for WeChat mappings".to_string())
    })?;
    crate::billing::validate_currency_code(currency)?;
    Ok(Some(serde_json::json!({
        "price": price,
        "currency": currency,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::billing::entities::BillingType;

    #[test]
    fn non_renewing_requires_service_duration_days() {
        // Missing duration → BadRequest (US-PM-002 scene 2 → 400).
        let err = validate_non_renewing(&BillingType::NonRenewing, None, None).unwrap_err();
        assert!(matches!(err, CoreError::BadRequest(_)), "got {err:?}");
    }

    #[test]
    fn non_renewing_rejects_zero_or_negative_duration() {
        for bad in [0_i64, -1, -5] {
            let err =
                validate_non_renewing(&BillingType::NonRenewing, Some(bad), None).unwrap_err();
            assert!(
                matches!(err, CoreError::BadRequest(_)),
                "value {bad} accepted"
            );
        }
    }

    #[test]
    fn non_renewing_accepts_positive_duration_without_period() {
        assert!(validate_non_renewing(&BillingType::NonRenewing, Some(30), None,).is_ok());
        assert!(validate_non_renewing(&BillingType::NonRenewing, Some(1), Some(""),).is_ok());
    }

    #[test]
    fn non_renewing_rejects_billing_period_as_mutually_exclusive() {
        // A non-renewing mapping carries a fixed service period; a recurring
        // billing_period is a conflicting billing semantics → 400.
        let err = validate_non_renewing(&BillingType::NonRenewing, Some(30), Some("monthly"))
            .unwrap_err();
        assert!(matches!(err, CoreError::BadRequest(_)), "got {err:?}");
    }

    #[test]
    fn non_renewing_validation_is_noop_for_other_billing_types() {
        // recurring / one_time are not subject to the non-renewing invariants
        // here (their own rules are enforced separately), so the duration and
        // period arguments are ignored.
        assert!(validate_non_renewing(&BillingType::Recurring, None, Some("monthly"),).is_ok());
        assert!(validate_non_renewing(&BillingType::OneTime, None, None,).is_ok());
    }

    #[test]
    fn billing_type_stays_immutable_on_update_path() {
        // The update path resolves the duration against the EXISTING billing_type
        // (never the request), so a non_renewing mapping cannot be silently
        // downgraded. This pins the resolved-type branch: a recurring mapping
        // with a cleared duration must still pass (duration is meaningless for
        // recurring), while a non_renewing mapping with the same input must fail.
        // (Execised via validate_non_renewing with the resolved type.)
        assert!(validate_non_renewing(&BillingType::Recurring, None, None,).is_ok());
        assert!(validate_non_renewing(&BillingType::NonRenewing, None, None,).is_err());
    }

    #[test]
    fn provider_billing_type_rejects_stripe_creem_non_renewing() {
        // PRD pay_model §2.2: Herald does not extend the Stripe/Creem product
        // forms — non-renewing is an IAP/WeChat form, and a Stripe/Creem
        // non_renewing row would drift (the product sync writes billing_type
        // back to recurring while the duration field lingers).
        for provider in ["stripe", "creem"] {
            let err =
                validate_provider_billing_type(provider, &BillingType::NonRenewing).unwrap_err();
            assert!(
                matches!(err, CoreError::BadRequest(_)),
                "{provider} non_renewing accepted"
            );
            // The existing forms stay valid on both channels.
            assert!(validate_provider_billing_type(provider, &BillingType::OneTime).is_ok());
            assert!(validate_provider_billing_type(provider, &BillingType::Recurring).is_ok());
        }
    }

    #[test]
    fn provider_billing_type_keeps_wechat_recurring_rejection_and_iap_forms() {
        // WeChat keeps its recurring rejection (no auto-renewal) while its
        // non_renewing modeling stays valid; IAP channels accept all three
        // forms.
        let err = validate_provider_billing_type("wechat", &BillingType::Recurring).unwrap_err();
        assert!(
            matches!(err, CoreError::BadRequest(_)),
            "wechat recurring accepted"
        );
        assert!(
            validate_provider_billing_type("wechat", &BillingType::NonRenewing).is_ok(),
            "wechat non_renewing is the PRD-modeled subscription form"
        );
        for provider in ["apple", "google"] {
            for billing_type in [
                BillingType::OneTime,
                BillingType::NonRenewing,
                BillingType::Recurring,
            ] {
                assert!(
                    validate_provider_billing_type(provider, &billing_type).is_ok(),
                    "{provider} {billing_type:?} rejected"
                );
            }
        }
    }

    #[test]
    fn manual_price_rejects_non_wechat_providers() {
        // Stripe/Creem/IAP price truth lives in the provider catalog; a manual
        // price on those rows would create a second, unsynced source → 400.
        for provider in ["stripe", "creem", "apple", "google"] {
            let err = validate_manual_price(provider, Some(1990), Some("CNY")).unwrap_err();
            assert!(
                matches!(err, CoreError::BadRequest(_)),
                "{provider} accepted"
            );
        }
        assert!(
            validate_manual_price("stripe", None, None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn manual_price_wechat_requires_positive_price_and_currency() {
        // Without a positive price the WeChat create-order call can never
        // succeed, so the write itself must fail loud.
        for bad in [None, Some(0), Some(-1)] {
            let err = validate_manual_price("wechat", bad, Some("CNY")).unwrap_err();
            assert!(matches!(err, CoreError::BadRequest(_)), "{bad:?} accepted");
        }
        let err = validate_manual_price("wechat", Some(1990), None).unwrap_err();
        assert!(matches!(err, CoreError::BadRequest(_)));
        let err = validate_manual_price("wechat", Some(1990), Some("cny")).unwrap_err();
        assert!(
            matches!(err, CoreError::BadRequest(_)),
            "lowercase code accepted"
        );
        let err = validate_manual_price("wechat", Some(1990), Some("XTS")).unwrap_err();
        assert!(
            matches!(err, CoreError::BadRequest(_)),
            "reserved code accepted"
        );
    }

    #[test]
    fn manual_price_wechat_emits_sync_compatible_jsonb() {
        // The fragment must use the exact keys the Stripe/Creem sync writes so
        // every read path (purchase snapshot, purchase options, WeChat order
        // amount) treats it identically.
        let info = validate_manual_price("wechat", Some(1990), Some("CNY"))
            .unwrap()
            .unwrap();
        assert_eq!(info["price"], serde_json::json!(1990));
        assert_eq!(info["currency"], serde_json::json!("CNY"));
    }
}
