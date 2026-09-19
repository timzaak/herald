//! IAP error type (design §5.9).
//!
//! `IapError` is the single error enum surfaced by the `herald-infra-iap`
//! crate. It carries provider-agnostic variants that map cleanly to HTTP
//! status codes in the api-billing layer:
//!
//! | variant               | HTTP |
//! |-----------------------|------|
//! | `NotConfigured`       | 404  |
//! | `OwnershipMismatch`   | 409  |
//! | `AppleVerification`   | 422  |
//! | `GoogleApi`           | 422  |
//! | `AlreadyConsumed`     | 422  |
//! | `InvalidPathSegment`  | 422  |
//! | `ServiceAccountAuth`  | 500  |
//! | `Transport`           | 500  |
//! | `Json`                | 500  |

use uuid::Uuid;

/// Errors surfaced by the IAP infrastructure crate.
///
/// Kept provider-agnostic on purpose so the api-billing handlers can map the
/// variants to HTTP responses without re-implementing Apple/Google specifics.
/// The variant set mirrors design §5.9; the `display` strings are stable
/// enough for diagnostic logs but should not be forwarded verbatim to clients
/// (the api layer maps variants to `failureReason` codes).
#[derive(Debug, thiserror::Error)]
pub enum IapError {
    /// Apple JWS/x5c verification failed (signature, chain, bundle id or
    /// environment mismatch). Maps to HTTP 422 `verification_failed`.
    #[error("apple JWS verification failed: {0}")]
    AppleVerification(String),

    /// Google Play Developer API returned a non-success status. Maps to HTTP
    /// 422 (`verification_failed`).
    #[error("google API error: status={status} body={body}")]
    GoogleApi { status: u16, body: String },

    /// Service-account JWT grant flow failed (signing, network, or OAuth
    /// error response). Maps to HTTP 500 (configuration / transport issue).
    #[error("service account JWT grant failed: {0}")]
    ServiceAccountAuth(String),

    /// The Realm has not configured the requested IAP provider credentials.
    /// Maps to HTTP 404 `iap credentials not configured`.
    #[error("iap credentials not configured for realm {realm_id} provider {provider}")]
    NotConfigured { realm_id: String, provider: String },

    /// The credential's ownership marker does not match the requesting user.
    /// Maps to HTTP 409 `ownership_mismatch`.
    #[error("ownership mismatch: credential does not belong to user {user_id}")]
    OwnershipMismatch { user_id: Uuid },

    /// The product has already been consumed (Google one-time). Maps to HTTP
    /// 422 `already_consumed`.
    #[error("product already consumed")]
    AlreadyConsumed,

    /// A caller-supplied identifier (receipt token / product id) cannot be
    /// addressed as a single URL path segment — it is `.` or `..`, which URL
    /// dot-segment normalization folds into a different resource path (and
    /// `%2E` folds too, so escaping cannot rescue it). Such a value can never
    /// be a real store identifier. Maps to HTTP 422 `verification_failed`.
    #[error("invalid identifier for API path segment: {0:?}")]
    InvalidPathSegment(String),

    /// Underlying HTTP transport error. Maps to HTTP 500.
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// JSON (de)serialization error. Maps to HTTP 500.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
