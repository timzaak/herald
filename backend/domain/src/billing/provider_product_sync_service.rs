use std::collections::HashMap;
use std::sync::Arc;

use crate::authentication::Identity;
use crate::billing::entities::EntitlementMapping;
use crate::billing::policies::BillingPolicy;
use crate::billing::ports::BillingRepository;
use crate::common::entities::app_errors::CoreError;
use crate::common::policies::ensure_policy;

/// A single price variant of a provider product.
///
/// Stripe exposes one `Price` per (product, currency, billing period) and gives
/// each a real `external_price_id`. Creem is product-level only and has no price
/// id, so `external_price_id` is `None` for Creem rows.
#[derive(Debug, Clone)]
pub struct ProviderPrice {
    pub external_price_id: Option<String>,
    pub price: Option<i64>,
    pub currency: Option<String>,
    pub billing_type: Option<String>,
    pub billing_period: Option<String>,
    pub price_metadata: Option<HashMap<String, String>>,
}

/// External provider product info returned by ProviderApiPort.
///
/// One product can carry multiple price variants (`prices`). A product with no
/// price info (Creem) carries an empty `prices` vec.
#[derive(Debug, Clone)]
pub struct ProviderProduct {
    pub external_product_id: String,
    pub name: String,
    pub description: Option<String>,
    pub product_metadata: Option<HashMap<String, String>>,
    pub prices: Vec<ProviderPrice>,
}

/// Stand-in `ProviderPrice` for products that carry no price variants at all
/// (Creem: product-level, no Stripe-style price object). Fields are all `None`,
/// which writes `external_price_id = NULL` and dedups via the `NULLS NOT
/// DISTINCT` unique constraint. A real `ProviderPrice` instance is
/// not `const`, so this is expressed as a plain static for the sync loop's
/// empty-prices fallback.
static NULL_PRICE: ProviderPrice = ProviderPrice {
    external_price_id: None,
    price: None,
    currency: None,
    billing_type: None,
    billing_period: None,
    price_metadata: None,
};

/// Result of a full provider sync operation
#[derive(Debug, Clone)]
pub struct SyncResult {
    pub products_synced: usize,
    pub prices_synced: usize,
    pub sync_status: SyncStatus,
    pub error: Option<String>,
    pub partial_errors: Vec<PartialSyncError>,
}

/// Sync status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    Completed,
    Partial,
    Failed,
}

/// Partial sync error detail
#[derive(Debug, Clone)]
pub struct PartialSyncError {
    pub external_id: String,
    pub reason: String,
}

/// Port for accessing external provider APIs (Stripe, Creem, etc.)
/// Concrete implementations live in the infra layer.
pub trait ProviderApiPort: Send + Sync {
    /// Fetch all products from the provider for a given realm
    fn fetch_products(
        &self,
        realm_id: &str,
        payment_provider: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<ProviderProduct>, CoreError>> + Send + '_>,
    >;
}

/// ProviderProductSyncService - Syncs provider products into local entitlement mappings
///
/// Generic over:
/// - `R`: the billing repository (mapping upserts + lookups)
/// - `P`: the billing policy (permission gate)
/// - `A`: the external provider API (Stripe / Creem product fetch)
///
/// A freshly-synced product with no pre-existing mapping is inserted as an
/// unconfigured draft (`enabled = false`, no entitlement key configured by the
/// operator, no distribution rules). Points distribution rules are configured
/// separately by the operator via the Mapping management endpoints; a draft
/// carries no bucket binding (the old `bucket_id` column and the
/// registration-pool resolver have been removed with the multi-wallet rule
/// model).
pub struct ProviderProductSyncService<R, P, A>
where
    R: BillingRepository,
    P: BillingPolicy,
    A: ProviderApiPort,
{
    repository: Arc<R>,
    policy: Arc<P>,
    provider_api: Arc<A>,
}

impl<R, P, A> ProviderProductSyncService<R, P, A>
where
    R: BillingRepository + Send + Sync,
    P: BillingPolicy,
    A: ProviderApiPort,
{
    pub fn new(repository: Arc<R>, policy: Arc<P>, provider_api: Arc<A>) -> Self {
        Self {
            repository,
            policy,
            provider_api,
        }
    }

    /// Sync provider products into local entitlement mappings
    pub async fn sync_provider_products(
        &self,
        identity: Identity,
        realm_id: &str,
        payment_provider: &str,
    ) -> Result<SyncResult, CoreError> {
        ensure_policy(
            self.policy.can_manage_billing(identity.clone()).await,
            "Insufficient permissions to manage billing",
        )?;

        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot access billing from a different realm".to_string(),
            ));
        }

        let products = self
            .provider_api
            .fetch_products(realm_id, payment_provider)
            .await?;

        let mut products_synced = 0usize;
        let mut prices_synced = 0usize;
        let mut partial_errors = Vec::new();

        for product in products {
            // A product with zero price variants still needs one mapping row
            // (Creem: product-level, no price concept). Treat the empty case as
            // a single NULL-price variant so Creem keeps producing exactly one
            // row with `external_price_id IS NULL` (the
            // NULLS NOT DISTINCT unique constraint dedups on it).
            let prices: Vec<&ProviderPrice> = if product.prices.is_empty() {
                vec![&NULL_PRICE]
            } else {
                product.prices.iter().collect()
            };

            let mut product_had_any_upsert = false;

            for price in prices {
                let external_price_id = price.external_price_id.as_deref();

                let existing = self
                    .repository
                    .find_entitlement_mapping_by_provider_product_price(
                        realm_id,
                        payment_provider,
                        &product.external_product_id,
                        external_price_id,
                    )
                    .await?;

                let (mapping_id, entitlement_key, is_new_draft) = match existing.as_ref() {
                    Some(existing_mapping) => (
                        existing_mapping.id,
                        existing_mapping.entitlement_key.clone(),
                        false,
                    ),
                    None => (
                        uuid::Uuid::now_v7(),
                        draft_entitlement_key(&product.external_product_id),
                        true,
                    ),
                };

                let mapping = EntitlementMapping {
                    id: mapping_id,
                    realm_id: realm_id.to_string(),
                    payment_provider: payment_provider.to_string(),
                    external_product_id: product.external_product_id.clone(),
                    external_price_id: price.external_price_id.clone(),
                    entitlement_key,
                    // Provider word form → domain BillingType. When the provider
                    // reports no recognizable billing_type (NULL_PRICE rows, an
                    // unknown word form), PRESERVE the mapping's existing value
                    // on re-sync — billing_type on an operator-configured
                    // mapping is Herald-managed config (PRD subscription §4.1:
                    // sync must not override Herald-managed entitlement/points/
                    // quota policy), mirroring the service_duration_days
                    // preserve-on-resync policy below. Clearing it would leave
                    // purchases failing with "has no billing_type set".
                    billing_type: price
                        .billing_type
                        .as_deref()
                        .and_then(|s: &str| s.parse().ok())
                        .or_else(|| existing.as_ref().and_then(|m| m.billing_type.clone())),
                    billing_period: price.billing_period.clone(),
                    // Provider product sync never carries a service-period
                    // length (non_renewing duration is an admin-configured
                    // concept, not a provider-observed field). Preserve an
                    // existing mapping's value when re-syncing, else None.
                    service_duration_days: existing.as_ref().and_then(|m| m.service_duration_days),
                    // A freshly-synced draft starts disabled with no points
                    // grant; the operator configures rules via the Mapping
                    // management endpoints. An existing mapping keeps its
                    // configured `enabled` state on re-sync.
                    enabled: if is_new_draft {
                        false
                    } else {
                        existing.as_ref().map(|m| m.enabled).unwrap_or(false)
                    },
                    provider_product_info: Some(build_provider_product_info(&product, price)),
                    // Same preserve-on-resync policy for `granted_role_ids`
                    // (no role grant); the DB column default already enforces
                    // `'{}'`, but the domain struct requires a concrete value.
                    granted_role_ids: existing
                        .as_ref()
                        .map(|m| m.granted_role_ids.clone())
                        .unwrap_or_default(),
                    synced_at: Some(chrono::Utc::now()),
                    created_at: existing
                        .as_ref()
                        .map(|m| m.created_at)
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: chrono::Utc::now(),
                };

                match self.repository.upsert_entitlement_mapping(mapping).await {
                    Ok(_) => {
                        // `prices_synced` counts price-level upsert rows:
                        // every successful price-level upsert,
                        // including disabled/draft/Creem-NULL-price rows,
                        // increments. `products_synced` counts distinct
                        // products with at least one successful upsert.
                        prices_synced += 1;
                        product_had_any_upsert = true;
                    }
                    Err(e) => {
                        let external_id = match external_price_id {
                            Some(p) => format!("{}#{}", product.external_product_id, p),
                            None => product.external_product_id.clone(),
                        };
                        partial_errors.push(PartialSyncError {
                            external_id,
                            reason: e.to_string(),
                        });
                    }
                }
            }

            if product_had_any_upsert {
                products_synced += 1;
            }
        }

        let (sync_status, error) = if partial_errors.is_empty() {
            (SyncStatus::Completed, None)
        } else if products_synced > 0 {
            (SyncStatus::Partial, None)
        } else {
            (
                SyncStatus::Failed,
                Some("All products failed to sync".to_string()),
            )
        };

        Ok(SyncResult {
            products_synced,
            prices_synced,
            sync_status,
            error,
            partial_errors,
        })
    }
}

/// Assemble the `provider_product_info` JSONB value written to
/// `provider_entitlement_mappings` during sync.
///
/// Extracted from the sync loop purely so the JSON shape (the contract surface
/// downstream consumers — admin UI, backend/test scenarios — rely on) is
/// unit-testable without mocking the repository. Behavior is unchanged: same
/// keys, same source fields, same `null`-on-`None` serialization.
fn build_provider_product_info(
    product: &ProviderProduct,
    price: &ProviderPrice,
) -> serde_json::Value {
    serde_json::json!({
        "name": product.name,
        "description": product.description,
        "price": price.price,
        "currency": price.currency,
        "billing_type": price.billing_type,
        "billing_period": price.billing_period,
        "product_metadata": product.product_metadata,
        "price_metadata": price.price_metadata,
    })
}

/// Build a deterministic placeholder `entitlement_key` for a freshly-synced
/// provider product that has not yet been configured by an operator.
///
/// Why this exists: `provider_entitlement_mappings.entitlement_key` has a
/// `CHECK (entitlement_key ~ '^[a-z0-9-]{1,64}$')` constraint (migration
/// `20260607_product_reduce.sql`) that rejects the empty string, so a draft
/// insert must already carry a key that satisfies the regex. The placeholder
/// is derived from the product's `external_product_id` so it is:
/// - stable across re-syncs (idempotent: the same external id yields the same
///   key, and dedup is by `(realm_id, provider, external_product_id,
///   external_price_id)` via `find_entitlement_mapping_by_provider_product_price`,
///   so this never creates a duplicate row),
/// - never consumed by `resolve_bucket_id_for_entitlement`, because drafts are
///   inserted with `enabled=false` and the resolver filters on `enabled`,
/// - overwritten by the operator's PATCH with the real Herald entitlement key
///   (e.g. `herald-live-creem-entitlement`) before the mapping is enabled.
///
/// Algorithm: lowercase, replace every non-[a-z0-9] char with `-`, collapse
/// runs of `-`, trim leading/trailing `-`, cap at 64 chars, fall back to
/// `"draft"` if the result is empty (e.g. the external id was all symbols).
fn draft_entitlement_key(external_product_id: &str) -> String {
    let mut slug = String::with_capacity(external_product_id.len());
    let mut prev_was_dash = false;
    for ch in external_product_id.chars() {
        // Lowercase ASCII letters stay (as lowercase); ASCII digits stay.
        // Every other char (uppercase letters are folded to lowercase first,
        // so they never reach the separator branch) becomes a single `-`,
        // collapsing consecutive separators.
        if ch.is_ascii_uppercase() {
            slug.push(ch.to_ascii_lowercase());
            prev_was_dash = false;
        } else if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            slug.push(ch);
            prev_was_dash = false;
        } else if !prev_was_dash {
            slug.push('-');
            prev_was_dash = true;
        }
        if slug.len() >= 64 {
            break;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "draft".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderPrice, ProviderProduct};
    use super::{build_provider_product_info, draft_entitlement_key};
    use std::collections::HashMap;

    /// Mirrors the DB CHECK constraint `entitlement_key ~ '^[a-z0-9-]{1,64}$'`
    /// (migration `20260607_product_reduce.sql`) and the char-class validation
    /// the PATCH handler applies (`entitlement_mapping_handlers.rs`).
    fn satisfies_check(key: &str) -> bool {
        !key.is_empty()
            && key.len() <= 64
            && key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    }

    /// The draft key MUST satisfy the DB CHECK constraint. A regression here is
    /// exactly what broke the demo sync (empty-string insert rejected by the
    /// CHECK). This test fails the moment the slug stops matching the regex.
    #[test]
    fn draft_entitlement_key_always_satisfies_check_constraint() {
        let cases = [
            "prod_abc123",
            "PROD-ABC-123",
            "creem::monthly::herald",
            "x",
            &"a".repeat(200),
            "___",
            "UPPER/CASE/ID",
            "12345",
            "café-résumé",
            "",
        ];
        for input in cases {
            let key = draft_entitlement_key(input);
            assert!(
                satisfies_check(&key),
                "draft key {key:?} derived from {input:?} violates CHECK ^[a-z0-9-]{{1,64}}$"
            );
        }
    }

    /// Re-sync must produce the SAME draft key for the SAME external id, so the
    /// existing dedup-by-provider-product path updates the row in place instead
    /// of creating a duplicate. Idempotency is the load-bearing property for
    /// the placeholder approach to stay collision-free.
    #[test]
    fn draft_entitlement_key_is_deterministic_across_calls() {
        let input = "Creem_Prod_Herald_Monthly_2026";
        assert_eq!(draft_entitlement_key(input), draft_entitlement_key(input));
    }

    /// Sanity-check the normalization for a representative external id.
    #[test]
    fn draft_entitlement_key_normalizes_realistic_external_id() {
        assert_eq!(
            draft_entitlement_key("prod_HeraldLive_001"),
            "prod-heraldlive-001"
        );
    }

    /// An external id made entirely of separator chars must still yield a
    /// valid (non-empty) key — the `"draft"` fallback — so the CHECK never
    /// rejects the insert.
    #[test]
    fn draft_entitlement_key_falls_back_when_input_is_all_separators() {
        assert_eq!(draft_entitlement_key("___"), "draft");
        assert_eq!(draft_entitlement_key(""), "draft");
    }

    /// The slug must be capped at 64 chars to satisfy the `{1,64}` bound.
    #[test]
    fn draft_entitlement_key_caps_at_64_chars() {
        let key = draft_entitlement_key(&"a".repeat(200));
        assert!(key.len() <= 64, "slug length {} exceeds 64", key.len());
    }

    /// `provider_product_info` is the JSONB contract surface downstream
    /// consumers (admin UI, backend/test sync scenarios) rely on. This pins
    /// the assembled shape: every existing key is present with the right source
    /// field, the two new metadata keys are carried through, and `None`
    /// metadata serializes to JSON `null` (not omitted, not `"null"`).
    #[test]
    fn build_provider_product_info_assembles_expected_json_shape() {
        let product = ProviderProduct {
            external_product_id: "prod_123".to_string(),
            name: "Herald Live".to_string(),
            description: Some("Monthly plan".to_string()),
            product_metadata: Some(HashMap::from([("tier".to_string(), "pro".to_string())])),
            prices: Vec::new(),
        };
        let price = ProviderPrice {
            external_price_id: Some("price_abc".to_string()),
            price: Some(1999),
            currency: Some("usd".to_string()),
            billing_type: Some("recurring".to_string()),
            billing_period: Some("month".to_string()),
            price_metadata: Some(HashMap::from([(
                "nickname".to_string(),
                "Monthly".to_string(),
            )])),
        };

        let info = build_provider_product_info(&product, &price);

        // Existing keys unchanged.
        assert_eq!(info["name"], "Herald Live");
        assert_eq!(info["description"], "Monthly plan");
        assert_eq!(info["price"], 1999);
        assert_eq!(info["currency"], "usd");
        assert_eq!(info["billing_type"], "recurring");
        assert_eq!(info["billing_period"], "month");
        // New metadata keys carry the source value through verbatim.
        assert_eq!(info["product_metadata"]["tier"], "pro");
        assert_eq!(info["price_metadata"]["nickname"], "Monthly");

        // `None` metadata MUST serialize to JSON `null` (the NULL_PRICE /
        // Creem-empty fallback path), not be omitted or stringified.
        let null_product = ProviderProduct {
            external_product_id: "prod_456".to_string(),
            name: "Creem Product".to_string(),
            description: None,
            product_metadata: None,
            prices: Vec::new(),
        };
        let null_price = ProviderPrice {
            external_price_id: None,
            price: None,
            currency: None,
            billing_type: None,
            billing_period: None,
            price_metadata: None,
        };
        let null_info = build_provider_product_info(&null_product, &null_price);
        assert!(null_info["product_metadata"].is_null());
        assert!(null_info["price_metadata"].is_null());
        // Existing `None` fields keep their null serialization too.
        assert!(null_info["description"].is_null());
        assert!(null_info["price"].is_null());
    }
}
