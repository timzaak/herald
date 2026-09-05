use super::entities::MappingRow;
use crate::common::entities::app_errors::CoreError;
use std::future::Future;

/// Repository port for the `custom_domain_mapping` table.
///
/// This is the request-time query surface for host→realm resolution.
/// Implementations must ensure the "one enabled row per realm"
/// invariant on writes (see [`Self::upsert_for_realm`]).
///
/// Effectiveness rule: a hostname is considered active
/// iff `enabled = true`. `cname_verified` / `tls_ready` are surface-only and are
/// NOT part of request-time resolution.
#[cfg_attr(test, mockall::automock)]
pub trait CustomDomainMappingRepository: Send + Sync {
    /// Look up the enabled mapping for a hostname.
    ///
    /// Used by the host→realm middleware, the dynamic CORS predicate, the Caddy
    /// ask endpoint and the public resolve endpoint. Only rows with
    /// `enabled = true` are returned (effectiveness rule).
    fn find_by_hostname(
        &self,
        hostname: &str,
    ) -> impl Future<Output = Result<Option<MappingRow>, CoreError>> + Send;

    /// Insert-or-replace the enabled hostname mapping for a realm.
    ///
    /// On save the new hostname becomes the realm's single enabled mapping.
    /// If the realm previously had a *different* enabled hostname row, that old
    /// row is **deleted** (not disabled) so that at most one enabled row exists
    /// per realm. If the new hostname already equals the realm's current enabled
    /// hostname the call is idempotent. The freshly written row is returned with
    /// `enabled = true`, `cname_verified = false`, `tls_ready = false`
    /// (status is probed later by CNAME/ACME).
    ///
    /// A hostname already owned by another realm is a conflict (409); the
    /// implementation surfaces this as [`CoreError::Conflict`].
    fn upsert_for_realm(
        &self,
        realm_id: &str,
        hostname: &str,
    ) -> impl Future<Output = Result<MappingRow, CoreError>> + Send;

    /// Delete mapping rows matching the realm and/or hostname.
    ///
    /// At least one filter must be `Some`:
    /// - `hostname = Some(h)` deletes the single matching hostname row.
    /// - `realm_id = Some(r)` deletes all mapping rows for that realm (e.g.
    ///   restoring to "no custom domain").
    /// - Both `Some` deletes rows matching either predicate (OR semantics),
    ///   deleting the realm's current mapping and a conflicting hostname in one
    ///   call when needed.
    ///
    /// Returns the number of rows deleted. A zero-affected delete is not an
    /// error (idempotent on absent rows).
    fn delete_by_realm_or_hostname(
        &self,
        realm_id: Option<String>,
        hostname: Option<String>,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;

    /// Update the surface-only CNAME/TLS status for a hostname.
    ///
    /// Sets `cname_verified`, `tls_ready` and stamps `status_checked_at = now()`.
    /// Does NOT touch `enabled` (status is not part of resolution).
    /// Returns [`CoreError::NotFound`] if no row exists for the hostname.
    fn update_status(
        &self,
        hostname: &str,
        cname_verified: bool,
        tls_ready: bool,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}
