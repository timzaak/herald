use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::provider_common_types::validate_payment_provider_value;

/// Structured view of the `provider_product_info` JSONB synced from the
///
/// Field names are intentionally snake_case (no `rename_all`) so they match
/// the stored JSONB keys written by `build_provider_product_info` — the value
/// round-trips through `serde_json::from_value` without renaming. Every field
/// is optional because the backend stores the union of what each provider
/// exposes. `product_metadata` / `price_metadata` are strict string→string
/// maps (coerced at sync time in the provider adapter), matching how Stripe
/// and Creem model metadata.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProviderProductInfo {
    pub name: Option<String>,
    pub description: Option<String>,
    pub price: Option<i64>,
    pub currency: Option<String>,
    pub billing_type: Option<String>,
    pub billing_period: Option<String>,
    pub product_metadata: Option<HashMap<String, String>>,
    pub price_metadata: Option<HashMap<String, String>>,
}

/// Read-side view of one distribution rule. Surfaced on Mapping responses and
/// registration-rule responses.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PointDistributionRuleResponse {
    pub id: Uuid,
    /// Target credit bucket.
    pub bucket_id: Uuid,
    /// Non-empty; only the triggers legal for the parent owner / billing type.
    pub trigger_sources: Vec<String>,
    /// `fixed` or `quota`.
    pub grant_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points_amount: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validity_days: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_period_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_windows: Option<Vec<EntitlementQuotaWindowResponse>>,
    pub enabled: bool,
    pub display_order: i32,
}

/// Write-side view of one distribution rule. `id` is `None` to create,
/// `Some` to update an existing rule under the same parent owner.
#[derive(Debug, Clone, Deserialize, ToSchema, validator::Validate)]
#[serde(rename_all = "camelCase")]
pub struct PointDistributionRuleWrite {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    pub bucket_id: Uuid,
    /// Non-empty; the backend validates the owner/billing-type subset.
    pub trigger_sources: Vec<String>,
    /// `fixed` or `quota`.
    pub grant_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points_amount: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validity_days: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_period_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_windows: Option<Vec<QuotaWindowInput>>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub display_order: i32,
}

/// Reference to a distribution rule that targets a Credit Bucket. Surfaced on
/// the bucket management views; user-facing balance responses never carry this
/// field.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DistributionRuleReferenceResponse {
    pub rule_id: Uuid,
    /// `entitlement_mapping` / `realm_registration`.
    pub owner_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entitlement_mapping_id: Option<Uuid>,
    pub trigger_sources: Vec<String>,
    pub enabled: bool,
}

/// Response for a single entitlement mapping
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementMappingResponse {
    pub id: Uuid,
    pub payment_provider: String,
    pub external_product_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_price_id: Option<String>,
    pub entitlement_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_period: Option<String>,
    /// Non-renewing service-period length (days). Present only for
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_duration_days: Option<i64>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_product_info: Option<ProviderProductInfo>,
    /// Distribution rules owned by this mapping. Loaded separately and
    /// surfaced as `pointRules`; an empty array is a valid "no points grant"
    /// mapping (role-only / pure payment record).
    pub point_rules: Vec<PointDistributionRuleResponse>,
    /// (not skip-serializing) so the frontend always sees the field; empty
    /// array when no role grant is configured.
    pub granted_role_ids: Vec<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Read-side quota window view.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementQuotaWindowResponse {
    /// Sliding window length in seconds.
    pub window_seconds: i64,
    /// Quota limit.
    pub limit: i64,
    /// Stable display key (derived from `windowSeconds`, e.g. "5h"/"week"/"month").
    pub key: String,
}

/// Response for listing entitlement mappings
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementMappingListResponse {
    pub items: Vec<EntitlementMappingResponse>,
    pub total: i64,
}

/// Query parameters for listing entitlement mappings
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementMappingQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u64>,
}

/// Request to update an entitlement mapping
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEntitlementMappingRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entitlement_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Non-renewing service-period length (days). Same 3-state semantics as
    /// `point_rules`: `None` ⟺ leave unchanged; `Some(null)` ⟺ clear (only
    /// valid when the stored billing_type is not `non_renewing`); `Some(n)` ⟺
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_duration_days: Option<Option<i64>>,
    /// Points distribution rule upsert set owned by this mapping. `None` ⟺
    /// leave the existing rule set untouched; `Some(rules)` ⟺ upsert the given
    /// rules under this mapping (rules absent from the set are left untouched;
    /// disabling requires explicit `enabled = false`). Non-empty / present
    /// triggers the `points.manage` credit-field permission gate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_rules: Option<Vec<PointDistributionRuleWrite>>,
    /// Manual price in minor units — WeChat mappings only (WeChat has no
    /// hosted catalog to sync from). Merges into the stored
    /// `provider_product_info`. Rejected for every other provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<i64>,
    /// ISO 4217 currency code for the manual price — WeChat mappings only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

///
/// Generic over provider (IAP, Stripe, Creem). The
/// `uq_pem_realm_provider_product_price` unique constraint is enforced by the
/// repository and surfaces as HTTP 409. A request that carries `pointRules`
/// additionally requires the `points.manage` permission.
#[derive(Debug, Deserialize, ToSchema, validator::Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateEntitlementMappingRequest {
    /// `"apple"` / `"google"` / `"stripe"` / `"creem"` / `"wechat"`.
    #[validate(custom(function = "validate_payment_provider_value"))]
    pub payment_provider: String,
    pub external_product_id: String,
    /// Stripe Price ID for Stripe; `None` for IAP / Creem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_price_id: Option<String>,
    pub entitlement_key: String,
    /// `"recurring"` / `"one_time"` / `"non_renewing"`.
    #[validate(custom(function = "validate_create_billing_type"))]
    pub billing_type: String,
    /// Required when `billing_type == "recurring"`; must be empty for
    /// `"non_renewing"` (mutually exclusive billing semantics).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_period: Option<String>,
    /// Non-renewing service-period length (days). Required (`>= 1`) when
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_duration_days: Option<i64>,
    /// Initial points distribution rules owned by the new mapping (upsert
    /// set; empty / omitted is a valid "no points grant" mapping). Each rule
    /// is validated against the mapping's billing type before persistence.
    /// Non-empty triggers the `points.manage` credit-field permission gate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub point_rules: Vec<PointDistributionRuleWrite>,
    /// Roles auto-granted on payment success.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub granted_role_ids: Vec<Uuid>,
    /// Manual price in minor units (e.g. fen for CNY) — required for WeChat
    /// (no hosted catalog to sync from; the price drives the WeChat order
    /// amount) and rejected for every other provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<i64>,
    /// ISO 4217 currency code for the manual price — required for WeChat,
    /// rejected for every other provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

fn validate_create_billing_type(billing_type: &str) -> Result<(), validator::ValidationError> {
    if matches!(billing_type, "recurring" | "one_time" | "non_renewing") {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_billing_type"))
    }
}

/// Request to sync provider products
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncProviderRequest {
    pub payment_provider: String,
}

/// Response from provider product sync
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncProviderResponse {
    pub products_synced: i64,
    pub prices_synced: i64,
    pub sync_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub partial_errors: Vec<PartialSyncError>,
}

/// Partial sync error detail
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartialSyncError {
    pub external_id: String,
    pub reason: String,
}

/// Single one-time mapping item for frontend display
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OneTimeMappingItem {
    pub id: String,
    pub entitlement_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_product_info: Option<ProviderProductInfo>,
    /// Distribution rules owned by this mapping (topup trigger). The frontend
    /// renders the per-account grants; an empty array means no points grant.
    pub point_rules: Vec<PointDistributionRuleResponse>,
    pub payment_provider: String,
}

/// Response for listing one-time mappings
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OneTimeMappingListResponse {
    pub items: Vec<OneTimeMappingItem>,
}

/// Response for subscription detail
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDetailResponse {
    pub id: Uuid,
    pub client_app_id: Option<Uuid>,
    pub entitlement_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_price_id: Option<String>,
    pub payment_provider: String,
    pub status: String,
    /// Billing type snapshot (`"recurring"` / `"non_renewing"`). Lets the
    /// admin subscription view distinguish non-renewing subscriptions and hide
    pub billing_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_at_period_end: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Response for subscription list item
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionListItemResponse {
    pub id: Uuid,
    pub client_app_id: Option<Uuid>,
    pub entitlement_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_price_id: Option<String>,
    pub payment_provider: String,
    pub status: String,
    /// Billing type snapshot (`"recurring"` / `"non_renewing"`). Lets the
    /// admin subscription list distinguish non-renewing subscriptions and hide
    pub billing_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Response for listing subscriptions
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionListResponse {
    pub items: Vec<SubscriptionListItemResponse>,
    pub total: i64,
}

/// Query parameters for listing subscriptions
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionListQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entitlement_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u64>,
}

/// Request to cancel subscription
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelSubscriptionRequest {
    pub cancel_at_period_end: bool,
}

/// Response for subscription cancellation
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelSubscriptionResponse {
    pub subscription_id: String,
    pub canceled_at: String,
    pub message: String,
}

///
/// Carries only the editable fields; the stable display `key` is derived by
/// the backend from `windowSeconds` (via `derive_window_key`) before
/// persistence, so callers cannot drift window identity. Mirrors the
/// points-domain quota-window input shape; kept local to billing so
/// api-billing does not depend on api-points.
#[derive(Debug, Clone, Deserialize, ToSchema, validator::Validate)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindowInput {
    /// Sliding window length in seconds. Must be > 0.
    #[validate(range(min = 1))]
    pub window_seconds: i64,
    /// Quota limit. Must be >= 0 (0 = window grants nothing but is a valid
    /// config edge case).
    #[validate(range(min = 0))]
    pub limit: i64,
}

/// Single price-mapping row within a batch save.
///
/// One row per price of a product; `entitlement_key` is shared across the
/// product's prices. A row that carries `point_rules` requires `points.manage`
/// server-side.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PriceMappingUpdate {
    pub mapping_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// `None` ⟺ leave the existing rule set untouched; `Some(rules)` ⟺ upsert
    /// the given rules under this mapping. Non-empty / present triggers the
    /// `points.manage` credit-field permission gate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_rules: Option<Vec<PointDistributionRuleWrite>>,
    /// `Some([])` ⟺ clear (no role grant); `Some(non-empty)` ⟺ set.
    /// Non-empty triggers realm-membership validation server-side (does NOT
    /// require `roles.manage`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub granted_role_ids: Option<Vec<Uuid>>,
}

/// PUT `/api/bill/entitlement-mappings/batch` request body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchUpdateEntitlementMappingsRequest {
    pub payment_provider: String,
    pub external_product_id: String,
    pub updates: Vec<PriceMappingUpdate>,
}

/// PUT `/api/bill/entitlement-mappings/batch` response body.
///
/// `prices` returns the product's full latest set of price rows.
/// Reuses `EntitlementMappingResponse` as the per-price view.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchUpdateEntitlementMappingsResponse {
    pub saved: u32,
    pub prices: Vec<EntitlementMappingResponse>,
}

/// A purchasable price-level option for the purchase page.
///
/// Flat list; the frontend groups by `external_product_id` / billing period.
/// Field set evolves `OneTimeMappingItem` down to price granularity.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOptionView {
    pub mapping_id: Uuid,
    pub external_product_id: String,
    /// Stripe real price id; `None` for price-less providers (Creem).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_price_id: Option<String>,
    pub payment_provider: String,
    pub entitlement_key: String,
    /// `recurring` | `one_time`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_type: Option<String>,
    /// `month` / `year` / …
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_period: Option<String>,
    /// Product / plan name (card title).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Price amount (minor units).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Distribution rules owned by this mapping (the purchasable points
    /// entitlement). The frontend renders per-account grants; quota and fixed
    /// credits are shown separately and must NOT be summed. Empty ⟺ no points
    /// grant for this option.
    pub point_rules: Vec<PointDistributionRuleResponse>,
    pub enabled: bool,
    /// Whether purchasing this option grants a role that is one-per-user
    /// `billing_type=one_time` + non-empty `granted_role_ids`; points packages
    /// and subscriptions are never gated, so `grants_role` is `false` for them
    /// even if they happen to carry role grants.
    pub grants_role: bool,
    /// Whether the authenticated user already owns this one-time+role
    /// (`grants_role == true`); `false` otherwise. Lets the frontend disable the
    /// card pre-purchase. Ownership predicate:
    /// `user_has_any_role(granted_role_ids) || has_succeeded_attempt(target_id)`.
    pub already_owned: bool,
}

/// GET `/api/bill/client/{clientAppId}/purchase-options` response.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOptionListResponse {
    pub items: Vec<PurchaseOptionView>,
}
