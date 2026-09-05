//! Realm-level invoice eligibility evaluation.
//!
//! Provides a backend-computed eligibility *result* that the frontend consumes
//! to gate Create/Apply invoice buttons BEFORE submit (policy=none, missing
//! seller config), instead of relying on post-submit backend rejection.
//!
//! Regular users consume this result; they do NOT read admin config/policy
//! APIs directly. The realm-level evaluation is wired into `feature-availability`
//! so no separate realm-level endpoint is added.
//!
//! ## Single home for all eligibility judgments
//!
//! Both the realm-level judgment (`evaluate_realm_invoice_eligibility`) and the
//! per-resource judgment (`determine_invoice_apply_route`) live here so the
//! read-path rules do not diverge from the write-path validators
//! (`validate_not_mor_provider`, `validate_invoice_policy_allows_creation`) in
//! `herald_core::domain::billing::invoice_service`. The pure
//! `determine_invoice_apply_route` is the only place encoding the
//! "External-if-synced" decision; the per-resource endpoint resolves the facts
//! (ownership/provider/policy/seller/external) and delegates to it.

use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;

use crate::invoice_handlers::get_invoice_policy;

/// Realm-level invoice eligibility summary.
///
/// Surfaced to regular users via `feature-availability.invoiceEligibility`.
/// The `reason` field is `None` when everything is configured and allowed.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceEligibilitySummary {
    /// Whether the realm has an invoice seller config saved.
    pub has_seller_config: bool,
    /// Invoice policy: "provider_first" | "manual_only" | "none".
    /// Defaults to "provider_first" when unconfigured (same as mutation paths).
    pub policy: String,
    /// Realm-level: manual invoice creation is allowed (`policy != "none"`).
    pub can_create_manual_invoice: bool,
    /// Realm-level: applying for an invoice is allowed. Equal to
    /// `can_create_manual_invoice`; per-resource route checks are a later phase.
    pub can_apply_invoice: bool,
    /// Human-readable reason when eligibility is limited, else `None`.
    pub reason: Option<String>,
}

/// Evaluate realm-level invoice eligibility.
///
/// Reuses the policy-reading logic from `invoice_handlers::get_invoice_policy`
/// (no duplicated SQL/realm_config read) and the seller-config fact already
/// loaded by `feature-availability` (no second seller-config query).
///
/// Reason rules:
/// - `policy == "none"`        => "Realm does not issue Herald invoices"
/// - `!has_seller_config`      => "Seller information not configured"
/// - otherwise                 => `None`
pub async fn evaluate_realm_invoice_eligibility(
    state: &AppState,
    realm_id: &str,
    has_seller_config: bool,
) -> Result<InvoiceEligibilitySummary, ApiError> {
    let policy_config = get_invoice_policy(state, realm_id).await?;
    let policy = policy_config.policy.clone();

    let can_create_manual_invoice = policy != "none";
    // Realm-level: applying mirrors manual-creation eligibility. Per-resource
    // route checks (provider_first + provider capability) are a later phase.
    let can_apply_invoice = can_create_manual_invoice;

    let reason = if policy == "none" {
        Some("Realm does not issue Herald invoices".to_string())
    } else if !has_seller_config {
        Some("Seller information not configured".to_string())
    } else {
        None
    };

    Ok(InvoiceEligibilitySummary {
        has_seller_config,
        policy,
        can_create_manual_invoice,
        can_apply_invoice,
        reason,
    })
}

// =============================================================================
// Per-resource apply-eligibility
// =============================================================================
//
// The per-resource endpoint (GET
// /api/bill/{realmId}/my/invoices/apply-eligibility) resolves the facts and
// delegates here. Keeping this pure makes the rules trivially unit-testable and
// guarantees the read-path and write-path (`apply_invoice` →
// `validate_invoice_creation_policy` + seller-config check) stay in lockstep.

/// Verdict returned by [`determine_invoice_apply_route`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyRouteVerdict {
    /// `"external_provider" | "manual_fallback" | "disabled"`.
    pub route: String,
    pub can_apply: bool,
    pub reason: Option<String>,
}

/// Decide the per-resource invoice apply route from resolved facts.
///
/// Inputs are exactly the facts the endpoint resolves before calling this:
/// - `provider`              — resolved `payment_provider` (Stripe/Apple/Google/WeChat/Creem/None).
/// - `policy`                — `provider_first` / `manual_only` / `none`.
/// - `has_seller_config`     — `find_seller_config(realm)` returned `Some`.
/// - `has_external_invoice`  — an invoice with `source = external_sync` exists
///   for this resource's id (matched on `payment_attempt_id` or
///   `subscription_id`).
/// - `external_capability`   — the provider's external-invoice capability
///   switch (`externalInvoiceEnabled`, default true when unconfigured).
///
/// Rules are mutually exclusive and evaluated in this order:
///
/// 1. `policy == "none"`     => `disabled` (Herald invoices off)
/// 2. `provider` is an MoR provider (`creem`/`apple`/`google`) => `disabled`
///    (platform is Merchant of Record; mirrors `validate_not_mor_provider`
///    in the write path)
/// 3. `!has_seller_config`   => `disabled` (mirrors the `apply_invoice` 400 path)
/// 4. `policy == "provider_first" && provider == Some("stripe")
///    && external_capability` => `external_provider` (Stripe invoices are
///    pushed via webhook when the realm prefers provider invoices; with the
///    capability off the resource degrades to manual fallback — PRD §4.3)
/// 5. `has_external_invoice` => `external_provider` (read-only — a provider
///    invoice already exists; do not offer a duplicate Herald invoice)
/// 6. otherwise              => `manual_fallback, canApply=true`
///    (manual_only allows all non-MoR providers, and provider_first still
///    permits non-Stripe/no-provider manual fallback when no external invoice
///    exists.)
pub(crate) fn determine_invoice_apply_route(
    provider: Option<&str>,
    policy: &str,
    has_seller_config: bool,
    has_external_invoice: bool,
    external_capability: bool,
) -> ApplyRouteVerdict {
    // Rule 1: realm policy disables Herald invoices entirely.
    if policy == "none" {
        return ApplyRouteVerdict {
            route: "disabled".to_string(),
            can_apply: false,
            reason: Some("Invoice creation is disabled by policy".to_string()),
        };
    }

    // Rule 2: Merchant-of-Record providers (Creem, Apple App Store, Google
    // Play) — Herald must not create a competing invoice, regardless of the
    // realm's invoice_policy. Mirrors `validate_not_mor_provider` in the
    // write path (support-iap PRD §4.1: invoice_policy 不影响该约束).
    if matches!(provider, Some("creem") | Some("apple") | Some("google")) {
        return ApplyRouteVerdict {
            route: "disabled".to_string(),
            can_apply: false,
            reason: Some(format!(
                "{} transactions are managed by the platform as Merchant of Record",
                provider.unwrap_or_default()
            )),
        };
    }

    // Rule 3: no seller info configured — mirrors the `apply_invoice` 400 path.
    if !has_seller_config {
        return ApplyRouteVerdict {
            route: "disabled".to_string(),
            can_apply: false,
            reason: Some(
                "No seller configuration found for this realm. An admin must configure seller info first."
                    .to_string(),
            ),
        };
    }

    // Rule 4: with provider_first, Stripe invoices are expected to arrive via
    // webhook. Keep the resource read-only even before the external invoice has
    // been synced — unless the realm has turned Stripe's external-invoice
    // capability OFF, in which case the resource degrades to manual fallback
    // (PRD §4.3 provider-capability degradation).
    if policy == "provider_first" && provider == Some("stripe") && external_capability {
        return ApplyRouteVerdict {
            route: "external_provider".to_string(),
            can_apply: false,
            reason: None,
        };
    }

    // Rule 5: an externally-synced invoice already exists for this resource —
    // read-only. Do not offer a duplicate Herald invoice.
    if has_external_invoice {
        let provider_label = provider.unwrap_or("the provider");
        return ApplyRouteVerdict {
            route: "external_provider".to_string(),
            can_apply: false,
            reason: Some(format!(
                "An invoice from {} already exists for this resource.",
                provider_label
            )),
        };
    }

    // Rule 6: manual_only for any non-MoR provider, or provider_first for
    // non-Stripe/no provider, with seller config and no external invoice.
    // Manual fallback remains available.
    ApplyRouteVerdict {
        route: "manual_fallback".to_string(),
        can_apply: true,
        reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Boundary tests for `determine_invoice_apply_route` (Rule 9 — encode WHY).
    // These guard against divergence between the read-path rules and the
    // write-path validators. If anyone tightens or loosens the rules here,
    // `validate_invoice_creation_policy` / `apply_invoice` must be re-checked.

    #[test]
    fn policy_none_disables_apply() {
        let v = determine_invoice_apply_route(Some("stripe"), "none", true, false, true);
        assert_eq!(v.route, "disabled");
        assert!(!v.can_apply);
        assert!(v.reason.as_deref().unwrap().contains("disabled by policy"));
    }

    #[test]
    fn creem_provider_disables_apply_regardless_of_policy() {
        // Even with seller config and no external invoice, Creem is MoR.
        for policy in ["provider_first", "manual_only"] {
            let v = determine_invoice_apply_route(Some("creem"), policy, true, false, true);
            assert_eq!(v.route, "disabled", "policy={}", policy);
            assert!(!v.can_apply);
            assert!(v.reason.as_deref().unwrap().contains("Merchant of Record"));
        }
    }

    #[test]
    fn missing_seller_config_disables_apply() {
        // Missing seller config must disable apply before provider routing so
        // the read path mirrors the write path's seller-config rejection.
        let v = determine_invoice_apply_route(Some("stripe"), "provider_first", false, false, true);
        assert_eq!(v.route, "disabled");
        assert!(!v.can_apply);
        assert!(v.reason.as_deref().unwrap().contains("seller"));
    }

    #[test]
    fn provider_first_stripe_is_external_provider_before_sync() {
        // Under provider_first, Stripe invoices are pushed via webhook. Keep
        // apply disabled even before the external invoice has landed.
        let v = determine_invoice_apply_route(Some("stripe"), "provider_first", true, false, true);
        assert_eq!(v.route, "external_provider");
        assert!(!v.can_apply);
        assert!(v.reason.is_none());
    }

    #[test]
    fn external_invoice_is_external_provider_for_manual_only() {
        // Once a provider invoice exists, manual apply would create a duplicate.
        let v = determine_invoice_apply_route(Some("stripe"), "manual_only", true, true, true);
        assert_eq!(v.route, "external_provider");
        assert!(!v.can_apply);
        assert!(v.reason.as_deref().unwrap().contains("stripe"));
    }

    #[test]
    fn manual_only_non_creem_with_seller_is_manual_fallback() {
        // manual_only means Herald self-issues invoices for every non-MoR
        // provider when no external invoice already exists.
        let v = determine_invoice_apply_route(Some("stripe"), "manual_only", true, false, true);
        assert_eq!(v.route, "manual_fallback");
        assert!(v.can_apply);
    }

    #[test]
    fn provider_first_no_provider_with_seller_is_manual_fallback() {
        // provider_first still has a manual fallback when no external-provider
        // route is known for the resource.
        let v = determine_invoice_apply_route(None, "provider_first", true, false, true);
        assert_eq!(v.route, "manual_fallback");
        assert!(v.can_apply);
    }

    #[test]
    fn external_provider_label_falls_back_when_provider_none() {
        // Resource somehow has an external invoice but no resolved provider
        // (should not happen in practice, but the verdict must not panic).
        let v = determine_invoice_apply_route(None, "provider_first", true, true, true);
        assert_eq!(v.route, "external_provider");
        assert!(v.reason.as_deref().unwrap().contains("the provider"));
    }

    #[test]
    fn provider_first_stripe_capability_off_degrades_to_manual_fallback() {
        // PRD §4.3: a provider whose external-invoice capability is switched
        // OFF under provider_first degrades to the manual fallback route —
        // the write path must stay writable for it too.
        let v = determine_invoice_apply_route(Some("stripe"), "provider_first", true, false, false);
        assert_eq!(v.route, "manual_fallback");
        assert!(v.can_apply);
    }

    #[test]
    fn policy_none_takes_precedence_over_creem() {
        // Rule 1 is checked before Rule 2: policy=none + creem => disabled by
        // policy (either reason is correct; precedence is the invariant).
        let v = determine_invoice_apply_route(Some("creem"), "none", true, false, true);
        assert_eq!(v.route, "disabled");
        assert!(!v.can_apply);
    }
}
