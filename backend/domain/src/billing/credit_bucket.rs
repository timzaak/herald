// Credit Bucket domain entities
//
// The Credit Bucket is the unit of points-pool isolation. This module defines
// the domain DTOs returned by the infra-layer bucket directory CRUD and
// consumed by api-billing handlers. There is intentionally NO `is_default`
// field; registration routing is
// now expressed by `realm_registration` distribution rules, and a bucket is
// referenced by zero or more rules (surfaced on the management views as
// `rule_references`).

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;
use crate::points::DistributionRuleReference;

/// Credit Bucket catalog row.
///
/// Mirrors `credit_buckets` table columns (minus audit timestamps which are not
/// surfaced to API consumers in list/shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditBucket {
    pub id: Uuid,
    pub realm_id: String,
    /// Matches `^[a-z0-9-]{1,64}$` (DB CHECK constraint `chk_credit_buckets_key`).
    pub bucket_key: String,
    pub name: String,
    pub description: Option<String>,
    pub display_order: i32,
    pub enabled: bool,
}

/// Input for creating a Credit Bucket.
///
/// The coverage set (`client_app_ids`) MUST be non-empty (handler enforces 400 on
/// empty).
#[derive(Debug, Clone)]
pub struct CreateCreditBucketInput {
    pub realm_id: String,
    pub bucket_key: String,
    pub name: String,
    pub description: Option<String>,
    pub display_order: i32,
    pub enabled: bool,
    /// Coverage set — at least one entry required.
    pub client_app_ids: Vec<Uuid>,
}

/// Input for updating a Credit Bucket.
///
/// All provided fields replace the stored state (coverage set is fully
/// replaced, not merged — "coverage-set changes do not retroactively reclaim
/// balances" still holds: only future routing is affected).
#[derive(Debug, Clone)]
pub struct UpdateCreditBucketInput {
    pub realm_id: String,
    pub bucket_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub display_order: i32,
    pub enabled: bool,
    /// Replacement coverage set — at least one entry required.
    pub client_app_ids: Vec<Uuid>,
}

/// Detail view: bucket plus explicit client app ids plus the rules referencing
/// it. `rule_references` aggregates both `entitlement_mapping` and
/// `realm_registration` owners; an empty vec means no rule currently targets
/// this bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditBucketDetail {
    #[serde(flatten)]
    pub bucket: CreditBucket,
    pub client_app_ids: Vec<Uuid>,
    pub rule_references: Vec<DistributionRuleReference>,
}

/// List-item view with aggregate counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditBucketListItem {
    #[serde(flatten)]
    pub bucket: CreditBucket,
    pub covered_client_app_count: i64,
    pub rule_reference_count: i64,
}

/// Per-credit-type balance totals for a single bucket (overview / wallets).
///
/// Keys follow the `credit_type` DB enum values; missing types default to 0.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BucketByCreditType {
    pub topup: i64,
    pub subscription: i64,
    pub registration: i64,
    pub free_periodic: i64,
    pub granted: i64,
}

impl BucketByCreditType {
    pub fn total(&self) -> i64 {
        self.topup
            .saturating_add(self.subscription)
            .saturating_add(self.registration)
            .saturating_add(self.free_periodic)
            .saturating_add(self.granted)
    }
}

/// One row in the overview matrix (per bucket × credit type).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditBucketOverviewRow {
    pub bucket_id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub by_credit_type: BucketByCreditType,
    pub bucket_total: i64,
}

/// Result of `list_bucket_overview`: rows per bucket (residual rows kept for
/// disabled buckets) + a SEPARATE grand total across all buckets.
///
/// `grand_total` is intentionally a sibling field of `rows`, NOT mixed into the
/// rows array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditBucketOverview {
    pub rows: Vec<CreditBucketOverviewRow>,
    pub grand_total: BucketByCreditType,
}

/// Credit Bucket directory operation errors.
///
/// These carry structured bodies so api-billing handlers can emit
/// the exact error contracts. Convertible to `CoreError` for uniform
/// propagation through the repository layer; handlers translate back via
/// `ApiError::conflict_json` with the structured payload.
#[derive(Debug, Clone, Error)]
pub enum CreditBucketError {
    /// `bucket_key` collides with another bucket in the same realm (unique index
    /// `uq_credit_buckets_realm_key`). HTTP 400 `bucket_key_duplicate`.
    #[error("bucket_key_duplicate: bucketKey already exists in realm {realm_id}")]
    BucketKeyDuplicate { realm_id: String },

    /// Delete refused: bucket is in use by in-flight subscriptions or wallets with
    /// remaining balance. HTTP 409 `bucket_in_use` with structured body.
    #[error(
        "credit bucket {bucket_id} is in use ({active_subscriptions} active subscriptions, {holders_with_balance} wallets with balance)"
    )]
    BucketInUse {
        bucket_id: Uuid,
        active_subscriptions: i64,
        holders_with_balance: i64,
    },

    /// Transparent passthrough for non-structured errors (not-found, DB errors).
    /// Handlers map this back to the wrapped `CoreError` for status selection.
    #[error(transparent)]
    Other(#[from] CoreError),
}

impl From<CreditBucketError> for CoreError {
    fn from(err: CreditBucketError) -> Self {
        match err {
            CreditBucketError::Other(inner) => inner,
            // BucketKeyDuplicate is a 400 (bad request), not a 409 conflict —
            // mirror api-billing's structured-body mapping for generic propagation.
            CreditBucketError::BucketKeyDuplicate { realm_id: _ } => {
                CoreError::BadRequest(err.to_string())
            }
            // Preserve the structured message; handlers that need the structured
            // body should match on CreditBucketError directly before converting.
            other => CoreError::Conflict(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BucketByCreditType::total()` must sum all five credit-type balances.
    /// This is the value surfaced as `bucketTotal` and aggregated into
    /// `grand_total` — a wrong sum would mislead admin overview / user balances.
    #[test]
    fn bucket_by_credit_type_total_sums_all_types() {
        let by_type = BucketByCreditType {
            topup: 100,
            subscription: 200,
            registration: 50,
            free_periodic: 30,
            granted: 20,
        };
        assert_eq!(by_type.total(), 400);

        // Default (all zero) must report zero, not None/panic.
        assert_eq!(BucketByCreditType::default().total(), 0);
    }

    /// Saturating add guards against i64 overflow when summing across many wallets
    /// in `list_bucket_overview`. A naive `+` would panic in debug / wrap in
    /// release; saturating caps at i64::MAX.
    #[test]
    fn bucket_by_credit_type_total_saturates_on_overflow() {
        let by_type = BucketByCreditType {
            topup: i64::MAX - 10,
            subscription: 100,
            registration: 0,
            free_periodic: 0,
            granted: 0,
        };
        assert_eq!(by_type.total(), i64::MAX);
    }

    /// `CreditBucketError::Other` preserves the wrapped `CoreError` on round-trip
    /// through `From<CreditBucketError> for CoreError`, so not-found / DB errors
    /// retain their original status mapping when propagated generically.
    #[test]
    fn credit_bucket_error_other_round_trips_core_error() {
        let original = CoreError::NotFound;
        let bucket_err: CreditBucketError = original.clone().into();
        let back: CoreError = bucket_err.into();
        assert_eq!(back, original);
    }

    /// The structured conflict variant must NOT collapse to `CoreError::NotFound`
    /// — `BucketInUse` is a 409 so handlers can map it to a `bucket_in_use` body.
    #[test]
    fn structured_bucket_errors_map_to_conflict_status() {
        let in_use = CreditBucketError::BucketInUse {
            bucket_id: Uuid::nil(),
            active_subscriptions: 1,
            holders_with_balance: 2,
        };
        let core: CoreError = in_use.into();
        assert!(matches!(core, CoreError::Conflict(_)));
    }

    /// `BucketKeyDuplicate` must map to a 400 (bad request), not a 409 — it is a
    /// validation-style conflict on the requested `bucket_key`, surfaced as
    /// `bucket_key_duplicate`.
    #[test]
    fn bucket_key_duplicate_maps_to_bad_request() {
        let dup = CreditBucketError::BucketKeyDuplicate {
            realm_id: "r1".into(),
        };
        let core: CoreError = dup.into();
        assert!(matches!(core, CoreError::BadRequest(_)));
    }
}
