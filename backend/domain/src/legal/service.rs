use std::sync::Arc;

use uuid::Uuid;

use crate::audit::{AuditAction, AuditCategory, AuditContext, AuditResult, AuditTargetType};
use crate::audit::{AuditEventRepository, NewAuditEvent};
use crate::common::entities::app_errors::CoreError;
use crate::legal::entities::{
    AgreementMode, AgreementType, ConsentSource, ConsentStatusItem, LegalAgreementDraft,
    LegalAgreementVersion,
};
use crate::legal::error::LegalError;
use crate::legal::ports::{LegalAgreementRepository, UserConsentRepository};

/// Legal + consent use-case service.
///
/// Orchestrates the BE-D02 ports (`LegalAgreementRepository`,
/// `UserConsentRepository`) with `AuditEventRepository`. Held by `AppState`
/// and constructed once at wiring time (BE-D04 → BE-D05/BE-D06).
///
/// Follows the established domain service pattern (see billing
/// `EntitlementMappingService`): generic over the concrete repository types
/// rather than `Arc<dyn Trait>`, because the port traits return
/// `impl Future` and are therefore not object-safe.
pub struct LegalService<L, U, A>
where
    L: LegalAgreementRepository,
    U: UserConsentRepository,
    A: AuditEventRepository,
{
    legal_repo: Arc<L>,
    user_consent_repo: Arc<U>,
    audit_repo: Arc<A>,
}

impl<L, U, A> LegalService<L, U, A>
where
    L: LegalAgreementRepository,
    U: UserConsentRepository,
    A: AuditEventRepository,
{
    pub fn new(legal_repo: Arc<L>, user_consent_repo: Arc<U>, audit_repo: Arc<A>) -> Self {
        Self {
            legal_repo,
            user_consent_repo,
            audit_repo,
        }
    }

    /// Current effective version for a realm + type (realm custom if present,
    /// otherwise platform default). Thin pass-through over the repository.
    pub async fn current_effective(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> Result<Option<LegalAgreementVersion>, CoreError> {
        self.legal_repo
            .current_effective(realm_id, agreement_type)
            .await
    }

    /// Reconsent gate verdict for both agreement types.
    ///
    /// For each type: resolve the current effective version; if none is
    /// deployed for this type, skip it (a missing type must not block the
    /// whole batch). `needs_reconsent` is true when the user has no recorded
    /// consent for the type, or recorded a version other than the current
    /// effective one.
    pub async fn consent_status(
        &self,
        user_id: Uuid,
        realm_id: &str,
    ) -> Result<Vec<ConsentStatusItem>, CoreError> {
        let mut out = Vec::new();
        for agreement_type in [AgreementType::TermsOfService, AgreementType::PrivacyPolicy] {
            let current = match self
                .legal_repo
                .current_effective(realm_id, agreement_type.clone())
                .await?
            {
                Some(v) => v,
                None => continue,
            };
            let consented = self
                .user_consent_repo
                .get_consent(user_id, agreement_type.clone())
                .await?;
            let consented_version_id = consented.map(|c| c.consented_version_id);
            let needs_reconsent = consented_version_id != Some(current.id);
            out.push(ConsentStatusItem {
                agreement_type,
                current_version_id: current.id,
                consented_version_id,
                needs_reconsent,
            });
        }
        Ok(out)
    }

    /// Record consent for one or more agreement types.
    ///
    /// Each item must reference the *current effective* version for its type;
    /// a stale `version_id` is rejected as `LegalError::StaleVersion` (→
    /// `CoreError::Conflict`, HTTP 409) so the caller re-reads the effective
    /// version before retrying. Consent rows are upserted (idempotent on
    /// repeat of the same version), and one `agreement.consent` audit event
    /// is written per item. Audit-write failures are logged and do not roll
    /// back the consent write (audit is best-effort, matching the login
    /// handler pattern).
    pub async fn record_consent(
        &self,
        user_id: Uuid,
        realm_id: &str,
        items: Vec<(AgreementType, Uuid)>,
        source: ConsentSource,
        ctx: AuditContext,
    ) -> Result<(), CoreError> {
        for (agreement_type, version_id) in items {
            let agreement_type_str = agreement_type.as_ref().to_string();
            let current = self
                .legal_repo
                .current_effective(realm_id, agreement_type.clone())
                .await?
                .ok_or(LegalError::VersionNotFound)?;
            if current.id != version_id {
                return Err(LegalError::StaleVersion.into());
            }

            self.user_consent_repo
                .upsert_consent(user_id, realm_id, agreement_type, version_id)
                .await?;

            let details = serde_json::json!({
                "agreement_type": agreement_type_str,
                "version_id": version_id,
                "source": source.as_ref(),
            });
            if let Err(audit_err) = self
                .audit_repo
                .create(NewAuditEvent {
                    realm_id: realm_id.to_string(),
                    category: AuditCategory::Compliance,
                    action: AuditAction::AgreementConsent,
                    actor_id: ctx.actor_id.clone(),
                    actor_type: ctx.actor_type,
                    actor_name: ctx.actor_name.clone(),
                    target_type: AuditTargetType::User,
                    target_id: user_id.to_string(),
                    target_name: ctx.actor_name.clone(),
                    result: AuditResult::Success,
                    details: Some(details),
                    ip_address: ctx.ip_address.clone(),
                    user_agent: ctx.user_agent.clone(),
                    trace_id: ctx.trace_id.clone(),
                })
                .await
            {
                tracing::warn!(
                    error = %audit_err,
                    %user_id,
                    agreement_type = %agreement_type_str,
                    "Failed to record agreement consent audit event"
                );
            }
        }
        Ok(())
    }

    /// Publish a new per-realm custom agreement version.
    ///
    /// `content` must be a locale → body map containing at least one locale
    /// entry; an empty / non-object payload is rejected as `BadRequest`. The
    /// repository computes the next `version_no` and handles the unique
    /// constraint retry. A `agreement.published` audit event is recorded
    /// against the realm.
    pub async fn publish_custom(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        content: serde_json::Value,
        label: Option<String>,
        published_by: &str,
        ctx: AuditContext,
    ) -> Result<LegalAgreementVersion, CoreError> {
        if !content.is_object() || content.as_object().is_none_or(|m| m.is_empty()) {
            return Err(CoreError::BadRequest(
                "agreement content must be a non-empty locale map".to_string(),
            ));
        }

        let new_version = self
            .legal_repo
            .publish_custom_version(
                realm_id,
                agreement_type.clone(),
                content,
                label,
                published_by,
            )
            .await?;

        let details = serde_json::json!({
            "agreement_type": agreement_type.as_ref(),
            "version_id": new_version.id,
            "version_no": new_version.version_no,
        });
        if let Err(audit_err) = self
            .audit_repo
            .create(NewAuditEvent {
                realm_id: realm_id.to_string(),
                category: AuditCategory::Compliance,
                action: AuditAction::AgreementPublished,
                actor_id: ctx.actor_id.clone(),
                actor_type: ctx.actor_type,
                actor_name: ctx.actor_name.clone(),
                target_type: AuditTargetType::Realm,
                target_id: realm_id.to_string(),
                target_name: None,
                result: AuditResult::Success,
                details: Some(details),
                ip_address: ctx.ip_address.clone(),
                user_agent: ctx.user_agent.clone(),
                trace_id: ctx.trace_id.clone(),
            })
            .await
        {
            tracing::warn!(
                error = %audit_err,
                realm = realm_id,
                agreement_type = %agreement_type.as_ref(),
                "Failed to record agreement published audit event"
            );
        }

        Ok(new_version)
    }

    /// Revert a realm's agreement to the platform default.
    ///
    /// Appends a realm-scoped default-follow marker (new `id`, monotonic
    /// `version_no`). History remains append-only, while effective resolution
    /// follows the latest platform default after the marker. Records an
    /// `agreement.reverted` audit event with
    /// `reverted_from_custom: true`.
    pub async fn revert_to_default(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        published_by: &str,
        ctx: AuditContext,
    ) -> Result<LegalAgreementVersion, CoreError> {
        let default = self
            .legal_repo
            .current_default(agreement_type.clone())
            .await?
            .ok_or(LegalError::VersionNotFound)?;

        let new_version = self
            .legal_repo
            .publish_default_follow_marker(
                realm_id,
                agreement_type.clone(),
                default.content.clone(),
                default.version_label.clone(),
                published_by,
            )
            .await?;

        let details = serde_json::json!({
            "agreement_type": agreement_type.as_ref(),
            "version_id": new_version.id,
            "reverted_from_custom": true,
        });
        if let Err(audit_err) = self
            .audit_repo
            .create(NewAuditEvent {
                realm_id: realm_id.to_string(),
                category: AuditCategory::Compliance,
                action: AuditAction::AgreementReverted,
                actor_id: ctx.actor_id.clone(),
                actor_type: ctx.actor_type,
                actor_name: ctx.actor_name.clone(),
                target_type: AuditTargetType::Realm,
                target_id: realm_id.to_string(),
                target_name: None,
                result: AuditResult::Success,
                details: Some(details),
                ip_address: ctx.ip_address.clone(),
                user_agent: ctx.user_agent.clone(),
                trace_id: ctx.trace_id.clone(),
            })
            .await
        {
            tracing::warn!(
                error = %audit_err,
                realm = realm_id,
                agreement_type = %agreement_type.as_ref(),
                "Failed to record agreement reverted audit event"
            );
        }

        Ok(new_version)
    }

    /// Whether the realm has any custom (non-default) version for the type.
    /// Thin pass-through; handlers compose the admin view from this +
    /// `list_history` + `current_effective`.
    pub async fn has_custom(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> Result<bool, CoreError> {
        self.legal_repo.has_custom(realm_id, agreement_type).await
    }

    /// Version history (custom-first, then default fallback) for the admin
    /// view. Thin pass-through.
    pub async fn list_history(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        limit: u64,
    ) -> Result<Vec<LegalAgreementVersion>, CoreError> {
        self.legal_repo
            .list_history(realm_id, agreement_type, limit)
            .await
    }

    /// Fetch a single version by id (with full content body) for the admin
    /// "view past version" dialog. Thin pass-through.
    pub async fn get_version_by_id(
        &self,
        version_id: Uuid,
    ) -> Result<Option<LegalAgreementVersion>, CoreError> {
        self.legal_repo.get_version_by_id(version_id).await
    }

    /// Get the staged draft for `(realm_id, agreement_type)`, if any. Drafts
    /// are isolated from published versions and never affect the consent gate.
    pub async fn get_draft(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> Result<Option<LegalAgreementDraft>, CoreError> {
        self.legal_repo.get_draft(realm_id, agreement_type).await
    }

    /// Stage a draft for `(realm_id, agreement_type)`. `content` must be a
    /// non-empty locale → body map (same rule as `publish_custom`); an invalid
    /// payload is rejected as `BadRequest`. Idempotent: a repeat save
    /// overwrites the existing draft. Does NOT publish — the agreement stays
    /// unchanged for end users until [`publish_from_draft`](Self::publish_from_draft).
    pub async fn save_draft(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        content: serde_json::Value,
        version_label: Option<String>,
        updated_by: &str,
    ) -> Result<LegalAgreementDraft, CoreError> {
        self.save_draft_with_mode(
            realm_id,
            agreement_type,
            content,
            version_label,
            AgreementMode::FullText,
            None,
            updated_by,
        )
        .await
    }

    pub async fn save_draft_with_mode(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        content: serde_json::Value,
        version_label: Option<String>,
        mode: AgreementMode,
        external_url: Option<String>,
        updated_by: &str,
    ) -> Result<LegalAgreementDraft, CoreError> {
        if mode == AgreementMode::Link {
            validate_external_url(external_url.as_deref())?;
        }
        if (!content.is_object() || content.as_object().is_none_or(|m| m.is_empty()))
            && mode == AgreementMode::FullText
        {
            return Err(CoreError::BadRequest(
                "agreement content must be a non-empty locale map".to_string(),
            ));
        }
        self.legal_repo
            .upsert_draft(
                realm_id,
                agreement_type,
                content,
                version_label,
                mode,
                external_url,
                updated_by,
            )
            .await
    }

    /// Discard the staged draft. Idempotent: discarding a missing draft is a
    /// no-op. The published version table is untouched.
    pub async fn discard_draft(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> Result<(), CoreError> {
        self.legal_repo.delete_draft(realm_id, agreement_type).await
    }

    /// Publish the staged draft as a new per-realm custom version.
    ///
    /// Reads the draft, delegates to [`publish_custom`](Self::publish_custom)
    /// (which computes the next `version_no`, writes the immutable version row,
    /// and records an `agreement.published` audit event), then deletes the
    /// draft. An optional `version_label_override` replaces the draft's label
    /// for this publish (letting the admin adjust the label at publish time
    /// without re-saving the draft). Returns `DraftNotFound` (→ 404) when no
    /// draft exists for the type.
    ///
    /// Note: this is not wrapped in an explicit DB transaction, but the two
    /// effects are safe in sequence — `publish_custom_version` creates an
    /// independent immutable version row, and a leftover draft (if the delete
    /// failed) is harmless: the next save overwrites it and the next publish
    /// re-publishes the same content as a fresh version (version_no still
    /// advances). Acceptable per the established best-effort pattern in this
    /// service (audit writes are likewise not transactional with the write).
    pub async fn publish_from_draft(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        version_label_override: Option<String>,
        published_by: &str,
        ctx: AuditContext,
    ) -> Result<LegalAgreementVersion, CoreError> {
        let draft = self
            .legal_repo
            .get_draft(realm_id, agreement_type.clone())
            .await?
            .ok_or(LegalError::DraftNotFound)?;

        let label = version_label_override.or(draft.version_label);
        let new_version = match draft.mode {
            AgreementMode::FullText => {
                self.publish_custom(
                    realm_id,
                    agreement_type.clone(),
                    draft.content,
                    label,
                    published_by,
                    ctx.clone(),
                )
                .await?
            }
            AgreementMode::Link => {
                let external_url = draft.external_url.ok_or(LegalError::InvalidExternalUrl)?;
                validate_external_url(Some(&external_url))?;
                let version = self
                    .legal_repo
                    .publish_link_version(
                        realm_id,
                        agreement_type.clone(),
                        external_url.clone(),
                        label,
                        published_by,
                    )
                    .await?;
                let details = serde_json::json!({
                    "agreement_type": agreement_type.as_ref(),
                    "version_id": version.id,
                    "version_no": version.version_no,
                    "mode": "link",
                    "external_url": external_url,
                });
                if let Err(error) = self
                    .audit_repo
                    .create(NewAuditEvent {
                        realm_id: realm_id.to_string(),
                        category: AuditCategory::Compliance,
                        action: AuditAction::AgreementPublished,
                        actor_id: ctx.actor_id.clone(),
                        actor_type: ctx.actor_type,
                        actor_name: ctx.actor_name.clone(),
                        target_type: AuditTargetType::Realm,
                        target_id: realm_id.to_string(),
                        target_name: None,
                        result: AuditResult::Success,
                        details: Some(details),
                        ip_address: ctx.ip_address.clone(),
                        user_agent: ctx.user_agent.clone(),
                        trace_id: ctx.trace_id.clone(),
                    })
                    .await
                {
                    tracing::warn!(error = %error, "Failed to record link agreement publish audit");
                }
                version
            }
        };

        // Best-effort draft cleanup; a failure here does not unwind the publish
        // (the version is already effective). The leftover draft is overwritten
        // on the next save and surfaced to the admin via the GET-draft endpoint.
        if let Err(cleanup_err) = self.legal_repo.delete_draft(realm_id, agreement_type).await {
            tracing::warn!(
                error = %cleanup_err,
                realm = realm_id,
                "Failed to clear legal agreement draft after publish"
            );
        }

        Ok(new_version)
    }
}

fn validate_external_url(value: Option<&str>) -> Result<(), CoreError> {
    let raw = value
        .filter(|value| value.len() <= 2048)
        .ok_or(LegalError::InvalidExternalUrl)?;
    let parsed = url::Url::parse(raw).map_err(|_| LegalError::InvalidExternalUrl)?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(LegalError::InvalidExternalUrl.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{ActorType, AuditEvent, AuditEventFilters, PaginatedAuditEvents};
    use crate::legal::entities::{AgreementMode, AgreementSource, UserAgreementConsent};
    use chrono::Utc;
    use std::sync::Mutex;

    // ---- Hand-rolled mock ports -------------------------------------------------
    // The port traits return `impl Future`, so `mockall::automock` cannot mock
    // them. We use minimal in-memory fakes instead.

    #[derive(Clone, Debug, Default)]
    struct MockLegalRepo {
        effective: std::collections::HashMap<(String, String), LegalAgreementVersion>,
        default: std::collections::HashMap<String, LegalAgreementVersion>,
        history: Vec<LegalAgreementVersion>,
        published: Arc<Mutex<Vec<LegalAgreementVersion>>>,
        drafts: Arc<
            Mutex<
                std::collections::HashMap<
                    (String, String),
                    crate::legal::entities::LegalAgreementDraft,
                >,
            >,
        >,
    }

    impl LegalAgreementRepository for MockLegalRepo {
        fn current_effective(
            &self,
            realm_id: &str,
            agreement_type: AgreementType,
        ) -> impl Future<Output = Result<Option<LegalAgreementVersion>, CoreError>> + Send {
            let v = self
                .effective
                .get(&(realm_id.to_string(), agreement_type.as_ref().to_string()))
                .cloned();
            async move { Ok(v) }
        }
        fn current_default(
            &self,
            agreement_type: AgreementType,
        ) -> impl Future<Output = Result<Option<LegalAgreementVersion>, CoreError>> + Send {
            let v = self.default.get(agreement_type.as_ref()).cloned();
            async move { Ok(v) }
        }
        fn list_history(
            &self,
            _realm_id: &str,
            _agreement_type: AgreementType,
            _limit: u64,
        ) -> impl Future<Output = Result<Vec<LegalAgreementVersion>, CoreError>> + Send {
            let v = self.history.clone();
            async move { Ok(v) }
        }
        fn get_version_by_id(
            &self,
            version_id: Uuid,
        ) -> impl Future<Output = Result<Option<LegalAgreementVersion>, CoreError>> + Send {
            let published = self.published.clone();
            async move {
                let g = published.lock().unwrap();
                Ok(g.iter().find(|v| v.id == version_id).cloned())
            }
        }
        fn publish_custom_version(
            &self,
            _realm_id: &str,
            agreement_type: AgreementType,
            content: serde_json::Value,
            label: Option<String>,
            _published_by: &str,
        ) -> impl Future<Output = Result<LegalAgreementVersion, CoreError>> + Send {
            let published = self.published.clone();
            async move {
                let next_no = published.lock().unwrap().len() as i32 + 100;
                let version = LegalAgreementVersion {
                    id: Uuid::now_v7(),
                    realm_id: Some("r".to_string()),
                    agreement_type,
                    version_no: next_no,
                    version_label: label,
                    content,
                    source: AgreementSource::Custom,
                    mode: AgreementMode::FullText,
                    external_url: None,
                    published_at: Utc::now(),
                    published_by: None,
                };
                published.lock().unwrap().push(version.clone());
                Ok(version)
            }
        }
        fn publish_default_follow_marker(
            &self,
            _realm_id: &str,
            agreement_type: AgreementType,
            content: serde_json::Value,
            label: Option<String>,
            _published_by: &str,
        ) -> impl Future<Output = Result<LegalAgreementVersion, CoreError>> + Send {
            let published = self.published.clone();
            async move {
                let version = LegalAgreementVersion {
                    id: Uuid::now_v7(),
                    realm_id: Some("r".to_string()),
                    agreement_type,
                    version_no: published.lock().unwrap().len() as i32 + 100,
                    version_label: label,
                    content,
                    source: AgreementSource::Default,
                    mode: AgreementMode::FullText,
                    external_url: None,
                    published_at: Utc::now(),
                    published_by: None,
                };
                published.lock().unwrap().push(version.clone());
                Ok(version)
            }
        }
        fn publish_link_version(
            &self,
            _realm_id: &str,
            agreement_type: AgreementType,
            external_url: String,
            label: Option<String>,
            _published_by: &str,
        ) -> impl Future<Output = Result<LegalAgreementVersion, CoreError>> + Send {
            let published = self.published.clone();
            async move {
                let version = LegalAgreementVersion {
                    id: Uuid::now_v7(),
                    realm_id: Some("r".to_string()),
                    agreement_type,
                    version_no: published.lock().unwrap().len() as i32 + 100,
                    version_label: label,
                    content: serde_json::json!({}),
                    source: AgreementSource::Custom,
                    mode: AgreementMode::Link,
                    external_url: Some(external_url),
                    published_at: Utc::now(),
                    published_by: None,
                };
                published.lock().unwrap().push(version.clone());
                Ok(version)
            }
        }
        // Service `has_custom` is a thin pass-through and never branches on the
        // result, so the mock hardcodes `true`; no test under this module needs
        // the `false` branch (the infra/repo scenario tests cover both).
        async fn has_custom(
            &self,
            _realm_id: &str,
            _agreement_type: AgreementType,
        ) -> Result<bool, CoreError> {
            Ok(true)
        }
        fn get_draft(
            &self,
            realm_id: &str,
            agreement_type: AgreementType,
        ) -> impl Future<Output = Result<Option<LegalAgreementDraft>, CoreError>> + Send {
            let drafts = self.drafts.clone();
            let key = (realm_id.to_string(), agreement_type.as_ref().to_string());
            async move { Ok(drafts.lock().unwrap().get(&key).cloned()) }
        }
        fn upsert_draft(
            &self,
            realm_id: &str,
            agreement_type: AgreementType,
            content: serde_json::Value,
            version_label: Option<String>,
            mode: AgreementMode,
            external_url: Option<String>,
            updated_by: &str,
        ) -> impl Future<Output = Result<LegalAgreementDraft, CoreError>> + Send {
            let drafts = self.drafts.clone();
            let key = (realm_id.to_string(), agreement_type.as_ref().to_string());
            async move {
                let draft = LegalAgreementDraft {
                    id: Uuid::now_v7(),
                    realm_id: realm_id.to_string(),
                    agreement_type,
                    content,
                    version_label,
                    mode,
                    external_url,
                    updated_at: Utc::now(),
                    updated_by: Some(updated_by.to_string()),
                };
                drafts.lock().unwrap().insert(key, draft.clone());
                Ok(draft)
            }
        }
        fn delete_draft(
            &self,
            realm_id: &str,
            agreement_type: AgreementType,
        ) -> impl Future<Output = Result<(), CoreError>> + Send {
            let drafts = self.drafts.clone();
            let key = (realm_id.to_string(), agreement_type.as_ref().to_string());
            async move {
                drafts.lock().unwrap().remove(&key);
                Ok(())
            }
        }
    }

    #[derive(Clone, Debug, Default)]
    struct MockConsentRepo {
        consents: Arc<Mutex<std::collections::HashMap<(Uuid, String), UserAgreementConsent>>>,
    }

    impl UserConsentRepository for MockConsentRepo {
        fn upsert_consent(
            &self,
            user_id: Uuid,
            realm_id: &str,
            agreement_type: AgreementType,
            version_id: Uuid,
        ) -> impl Future<Output = Result<(), CoreError>> + Send {
            let consents = self.consents.clone();
            let realm_id = realm_id.to_string();
            let key = agreement_type.as_ref().to_string();
            async move {
                let mut g = consents.lock().unwrap();
                g.insert(
                    (user_id, key),
                    UserAgreementConsent {
                        id: Uuid::now_v7(),
                        user_id,
                        realm_id,
                        agreement_type,
                        consented_version_id: version_id,
                        consented_at: Utc::now(),
                    },
                );
                Ok(())
            }
        }
        fn get_consent(
            &self,
            user_id: Uuid,
            agreement_type: AgreementType,
        ) -> impl Future<Output = Result<Option<UserAgreementConsent>, CoreError>> + Send {
            let consents = self.consents.clone();
            let key = agreement_type.as_ref().to_string();
            async move { Ok(consents.lock().unwrap().get(&(user_id, key)).cloned()) }
        }
    }

    #[derive(Clone, Debug, Default)]
    struct MockAuditRepo {
        events: Arc<Mutex<Vec<NewAuditEvent>>>,
    }

    impl AuditEventRepository for MockAuditRepo {
        fn create(
            &self,
            event: NewAuditEvent,
        ) -> impl Future<Output = Result<AuditEvent, CoreError>> + Send {
            let events = self.events.clone();
            async move {
                events.lock().unwrap().push(event.clone());
                Ok(AuditEvent {
                    id: Uuid::now_v7(),
                    realm_id: event.realm_id,
                    category: event.category,
                    action: event.action,
                    actor_id: event.actor_id,
                    actor_type: event.actor_type,
                    actor_name: event.actor_name,
                    target_type: event.target_type,
                    target_id: event.target_id,
                    target_name: event.target_name,
                    result: event.result,
                    details: event.details,
                    ip_address: event.ip_address,
                    user_agent: event.user_agent,
                    trace_id: event.trace_id,
                    created_at: Utc::now(),
                })
            }
        }
        async fn list_paginated(
            &self,
            _realm_id: &str,
            _filters: AuditEventFilters,
        ) -> Result<PaginatedAuditEvents, CoreError> {
            Ok(PaginatedAuditEvents {
                items: vec![],
                page: 0,
                page_size: 0,
                total: 0,
            })
        }
        async fn find_by_id(
            &self,
            _realm_id: &str,
            _event_id: Uuid,
        ) -> Result<Option<AuditEvent>, CoreError> {
            Ok(None)
        }
    }

    fn actor() -> AuditContext {
        AuditContext {
            actor_id: "u1".to_string(),
            actor_type: Some(ActorType::User),
            actor_name: None,
            ip_address: None,
            user_agent: None,
            trace_id: None,
        }
    }

    fn version(agreement_type: AgreementType, id: Uuid) -> LegalAgreementVersion {
        LegalAgreementVersion {
            id,
            realm_id: Some("r".to_string()),
            agreement_type,
            version_no: 1,
            version_label: None,
            content: serde_json::json!({"en": "body"}),
            source: AgreementSource::Custom,
            mode: AgreementMode::FullText,
            external_url: None,
            published_at: Utc::now(),
            published_by: None,
        }
    }

    fn make_service(
        legal: MockLegalRepo,
        consent: MockConsentRepo,
        audit: MockAuditRepo,
    ) -> LegalService<MockLegalRepo, MockConsentRepo, MockAuditRepo> {
        LegalService::new(Arc::new(legal), Arc::new(consent), Arc::new(audit))
    }

    #[tokio::test]
    async fn link_draft_rejects_non_http_url_because_user_links_must_be_safe() {
        let svc = make_service(
            MockLegalRepo::default(),
            MockConsentRepo::default(),
            MockAuditRepo::default(),
        );
        let error = svc
            .save_draft_with_mode(
                "r",
                AgreementType::TermsOfService,
                serde_json::json!({}),
                None,
                AgreementMode::Link,
                Some("javascript:alert(1)".to_string()),
                "admin",
            )
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::BadRequest(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn publishing_link_draft_creates_link_version_and_audit() {
        let legal = MockLegalRepo::default();
        let audit = MockAuditRepo::default();
        let svc = make_service(legal.clone(), MockConsentRepo::default(), audit.clone());
        svc.save_draft_with_mode(
            "r",
            AgreementType::PrivacyPolicy,
            serde_json::json!({}),
            Some("2026-07".into()),
            AgreementMode::Link,
            Some("https://example.com/privacy".into()),
            "admin",
        )
        .await
        .unwrap();
        let version = svc
            .publish_from_draft("r", AgreementType::PrivacyPolicy, None, "admin", actor())
            .await
            .unwrap();
        assert_eq!(version.mode, AgreementMode::Link);
        assert_eq!(
            version.external_url.as_deref(),
            Some("https://example.com/privacy")
        );
        let events = audit.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].details.as_ref().and_then(|d| d["mode"].as_str()),
            Some("link")
        );
    }

    #[tokio::test]
    async fn consent_status_needs_reconsent_when_no_consent_recorded() {
        let id = Uuid::now_v7();
        let mut legal = MockLegalRepo::default();
        legal.effective.insert(
            ("r".to_string(), "terms_of_service".to_string()),
            version(AgreementType::TermsOfService, id),
        );
        let svc = make_service(legal, MockConsentRepo::default(), MockAuditRepo::default());

        let status = svc.consent_status(Uuid::now_v7(), "r").await.unwrap();
        let tos = status
            .iter()
            .find(|s| s.agreement_type == AgreementType::TermsOfService)
            .unwrap();
        assert!(tos.needs_reconsent);
        assert_eq!(tos.consented_version_id, None);
    }

    #[tokio::test]
    async fn consent_status_no_reconsent_when_versions_match() {
        let user = Uuid::now_v7();
        let id = Uuid::now_v7();
        let mut legal = MockLegalRepo::default();
        legal.effective.insert(
            ("r".to_string(), "terms_of_service".to_string()),
            version(AgreementType::TermsOfService, id),
        );
        let consent = MockConsentRepo::default();
        consent.consents.lock().unwrap().insert(
            (user, "terms_of_service".to_string()),
            UserAgreementConsent {
                id: Uuid::now_v7(),
                user_id: user,
                realm_id: "r".to_string(),
                agreement_type: AgreementType::TermsOfService,
                consented_version_id: id,
                consented_at: Utc::now(),
            },
        );
        let svc = make_service(legal, consent, MockAuditRepo::default());

        let status = svc.consent_status(user, "r").await.unwrap();
        let tos = status
            .iter()
            .find(|s| s.agreement_type == AgreementType::TermsOfService)
            .unwrap();
        assert!(!tos.needs_reconsent);
    }

    #[tokio::test]
    async fn record_consent_rejects_stale_version() {
        let current_id = Uuid::now_v7();
        let mut legal = MockLegalRepo::default();
        legal.effective.insert(
            ("r".to_string(), "terms_of_service".to_string()),
            version(AgreementType::TermsOfService, current_id),
        );
        let svc = make_service(legal, MockConsentRepo::default(), MockAuditRepo::default());

        let stale = Uuid::now_v7();
        let err = svc
            .record_consent(
                Uuid::now_v7(),
                "r",
                vec![(AgreementType::TermsOfService, stale)],
                ConsentSource::Explicit,
                actor(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Conflict(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn record_consent_idempotent_on_repeat_current_version() {
        let user = Uuid::now_v7();
        let current_id = Uuid::now_v7();
        let mut legal = MockLegalRepo::default();
        legal.effective.insert(
            ("r".to_string(), "terms_of_service".to_string()),
            version(AgreementType::TermsOfService, current_id),
        );
        let consent = MockConsentRepo::default();
        let audit = MockAuditRepo::default();
        let svc = make_service(legal, consent.clone(), audit.clone());

        svc.record_consent(
            user,
            "r",
            vec![(AgreementType::TermsOfService, current_id)],
            ConsentSource::Register,
            actor(),
        )
        .await
        .unwrap();
        // Repeat same version — no error.
        svc.record_consent(
            user,
            "r",
            vec![(AgreementType::TermsOfService, current_id)],
            ConsentSource::Register,
            actor(),
        )
        .await
        .unwrap();
        assert_eq!(consent.consents.lock().unwrap().len(), 1);
        assert_eq!(audit.events.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn revert_to_default_publishes_live_default_marker() {
        let default_id = Uuid::now_v7();
        let mut legal = MockLegalRepo::default();
        legal.default.insert(
            "terms_of_service".to_string(),
            LegalAgreementVersion {
                id: default_id,
                realm_id: None,
                agreement_type: AgreementType::TermsOfService,
                version_no: 1,
                version_label: Some("default".to_string()),
                content: serde_json::json!({"en": "default body"}),
                source: AgreementSource::Default,
                mode: AgreementMode::FullText,
                external_url: None,
                published_at: Utc::now(),
                published_by: None,
            },
        );
        let audit = MockAuditRepo::default();
        let svc = make_service(legal, MockConsentRepo::default(), audit.clone());

        let new_version = svc
            .revert_to_default("r", AgreementType::TermsOfService, "admin", actor())
            .await
            .unwrap();
        // Revert remains an append-only realm event but its source marks live
        // default following instead of a permanently detached custom snapshot.
        assert_ne!(new_version.id, default_id);
        assert_eq!(new_version.source, AgreementSource::Default);
        assert_eq!(
            new_version.content,
            serde_json::json!({"en": "default body"})
        );
        // Audit recorded as reverted with reverted_from_custom=true.
        let events = audit.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, AuditAction::AgreementReverted);
        let details = events[0].details.as_ref().unwrap();
        assert_eq!(details["reverted_from_custom"], true);
    }

    #[tokio::test]
    async fn publish_custom_rejects_empty_content() {
        let svc = make_service(
            MockLegalRepo::default(),
            MockConsentRepo::default(),
            MockAuditRepo::default(),
        );
        let err = svc
            .publish_custom(
                "r",
                AgreementType::TermsOfService,
                serde_json::json!({}),
                None,
                "admin",
                actor(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::BadRequest(_)));
    }
}
