use std::future::Future;

use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;
use crate::legal::entities::{
    AgreementMode, AgreementType, LegalAgreementDraft, LegalAgreementVersion, UserAgreementConsent,
};

/// Repository for legal agreement versions.
///
/// Implementations live in the infrastructure layer (BE-D03). All methods are
/// `impl Future`-returning (see `billing::ports` for the established pattern).
pub trait LegalAgreementRepository: Send + Sync {
    /// Current effective version for a realm: the latest realm-scoped row if
    /// one exists, otherwise the platform default (`realm_id IS NULL`).
    fn current_effective(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> impl Future<Output = Result<Option<LegalAgreementVersion>, CoreError>> + Send;

    /// Platform default template for an agreement type (`realm_id IS NULL`).
    /// Used as the snapshot source when reverting a realm to defaults.
    fn current_default(
        &self,
        agreement_type: AgreementType,
    ) -> impl Future<Output = Result<Option<LegalAgreementVersion>, CoreError>> + Send;

    /// Version history for a realm + type, custom-priority with default
    /// fallback. Drives the admin history view.
    fn list_history(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        limit: u64,
    ) -> impl Future<Output = Result<Vec<LegalAgreementVersion>, CoreError>> + Send;

    /// Fetch a single version by primary key, including its full localized
    /// `content` body. Used by the admin history "view past version" dialog.
    /// Returns `None` when the id does not resolve (→ 404 at the handler).
    fn get_version_by_id(
        &self,
        version_id: Uuid,
    ) -> impl Future<Output = Result<Option<LegalAgreementVersion>, CoreError>> + Send;

    /// Publish a new per-realm custom version. The implementation computes
    /// `version_no = max(version_no) + 1` within `(realm_id, agreement_type)`
    /// and retries on the `(COALESCE(realm_id,''), agreement_type, version_no)`
    /// unique constraint (BE-D03).
    fn publish_custom_version(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        content: serde_json::Value,
        label: Option<String>,
        published_by: &str,
    ) -> impl Future<Output = Result<LegalAgreementVersion, CoreError>> + Send;

    /// Append a realm-scoped marker that switches effective resolution back
    /// to the live platform default while retaining the realm's history.
    fn publish_default_follow_marker(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        content: serde_json::Value,
        label: Option<String>,
        published_by: &str,
    ) -> impl Future<Output = Result<LegalAgreementVersion, CoreError>> + Send;

    fn publish_link_version(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        external_url: String,
        label: Option<String>,
        published_by: &str,
    ) -> impl Future<Output = Result<LegalAgreementVersion, CoreError>> + Send;

    /// Whether the realm has any custom (non-default) version published for the
    /// given type. Drives the "default vs custom" indicator in the admin view.
    fn has_custom(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> impl Future<Output = Result<bool, CoreError>> + Send;

    /// Get the staged draft for `(realm_id, agreement_type)`, if any.
    /// Drafts are kept in a separate table and never affect version resolution
    /// or the consent gate.
    fn get_draft(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> impl Future<Output = Result<Option<LegalAgreementDraft>, CoreError>> + Send;

    /// Upsert the draft for `(realm_id, agreement_type)`. Idempotent: a second
    /// save overwrites the existing draft (last-write-wins via the
    /// `(realm_id, agreement_type)` unique constraint).
    fn upsert_draft(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        content: serde_json::Value,
        version_label: Option<String>,
        mode: AgreementMode,
        external_url: Option<String>,
        updated_by: &str,
    ) -> impl Future<Output = Result<LegalAgreementDraft, CoreError>> + Send;

    /// Delete the draft for `(realm_id, agreement_type)`. Idempotent: deleting a
    /// missing draft is a no-op (returns Ok(())).
    fn delete_draft(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

/// Repository for user agreement consent records.
pub trait UserConsentRepository: Send + Sync {
    /// Upsert the consent row for `(user_id, agreement_type)` to the given
    /// version. Idempotent: re-consenting the same version is a no-op write.
    fn upsert_consent(
        &self,
        user_id: Uuid,
        realm_id: &str,
        agreement_type: AgreementType,
        version_id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// Fetch the user's recorded consent for an agreement type, if any.
    fn get_consent(
        &self,
        user_id: Uuid,
        agreement_type: AgreementType,
    ) -> impl Future<Output = Result<Option<UserAgreementConsent>, CoreError>> + Send;
}
