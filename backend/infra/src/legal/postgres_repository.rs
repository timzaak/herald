use std::convert::TryFrom;

use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, DbErr, EntityTrait,
    IntoActiveModel, QueryFilter, QueryOrder, RuntimeErr, Set,
};
use uuid::Uuid;

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::legal::UserAgreementConsent;
use herald_domain::legal::entities::{
    AgreementMode, AgreementSource, AgreementType, LegalAgreementDraft, LegalAgreementVersion,
};
use herald_domain::legal::error::LegalError;
use herald_domain::legal::ports::{LegalAgreementRepository, UserConsentRepository};
use herald_entity::{legal_agreement_draft, legal_agreement_version, user_agreement_consent};

/// PostgreSQL implementation of [`LegalAgreementRepository`].
///
/// Holds a SeaORM `DatabaseConnection` (same constructor/shape as
/// `PostgresBillingRepository`). All row ↔ domain mapping happens here so the
/// service layer (BE-D04) only handles domain types.
pub struct PostgresLegalAgreementRepository {
    db: DatabaseConnection,
}

impl PostgresLegalAgreementRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Map a `legal_agreement_version` row to the domain entity.
    ///
    /// An `agreement_type` parse failure (a stray column value) is surfaced as
    /// `CoreError::InternalServerError` rather than silently corrupting
    /// resolution — the column is constrained to two values in practice, so a
    /// third means schema/operator drift that callers must not mask.
    fn to_domain(
        model: legal_agreement_version::Model,
    ) -> Result<LegalAgreementVersion, CoreError> {
        Ok(LegalAgreementVersion {
            id: model.id,
            realm_id: model.realm_id,
            agreement_type: AgreementType::try_from(model.agreement_type.as_str())
                .map_err(CoreError::InternalServerError)?,
            version_no: model.version_no,
            version_label: model.version_label,
            content: model.content,
            source: AgreementSource::from(model.source.as_str()),
            mode: AgreementMode::from(model.mode.as_str()),
            external_url: model.external_url,
            published_at: chrono::DateTime::<chrono::Utc>::from(model.published_at),
            published_by: model.published_by,
        })
    }

    /// Map a `legal_agreement_draft` row to the domain entity. An
    /// `agreement_type` parse failure is surfaced as `InternalServerError`
    /// (same policy as the version mapping above).
    fn to_draft_domain(
        model: legal_agreement_draft::Model,
    ) -> Result<LegalAgreementDraft, CoreError> {
        Ok(LegalAgreementDraft {
            id: model.id,
            realm_id: model.realm_id,
            agreement_type: AgreementType::try_from(model.agreement_type.as_str())
                .map_err(CoreError::InternalServerError)?,
            content: model.content,
            version_label: model.version_label,
            mode: AgreementMode::from(model.mode.as_str()),
            external_url: model.external_url,
            updated_at: chrono::DateTime::<chrono::Utc>::from(model.updated_at),
            updated_by: model.updated_by,
        })
    }

    /// Compute the next `version_no` for a custom (per-realm) publish:
    /// `max(version_no)` over the realm's custom rows + 1, or 1 when the realm
    /// has no custom version yet. Scoped to the realm's own rows only — the
    /// platform default rows (`realm_id IS NULL`) never participate, so a realm
    /// publishing for the first time starts at version_no = 1 alongside the
    /// default seed (they live in different scopes of the unique index).
    async fn next_custom_version_no(
        &self,
        realm_id: &str,
        agreement_type: &AgreementType,
    ) -> Result<i32, CoreError> {
        let row = legal_agreement_version::Entity::find()
            .filter(legal_agreement_version::Column::RealmId.eq(realm_id))
            .filter(legal_agreement_version::Column::AgreementType.eq(agreement_type.as_str()))
            .order_by_desc(legal_agreement_version::Column::VersionNo)
            .one(&self.db)
            .await?;
        Ok(row.map(|m| m.version_no + 1).unwrap_or(1))
    }

    /// Shared insert path behind `publish_custom_version` and the
    /// default-follow marker: identical version_no race handling, differing
    /// only in the `source` column, so the marker never exists as a `custom`
    /// row awaiting correction.
    async fn publish_version_with_source(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        content: serde_json::Value,
        label: Option<String>,
        published_by: &str,
        source: &str,
    ) -> Result<LegalAgreementVersion, CoreError> {
        // Pre-extract owned/copied captures so the insert closure borrows no
        // value that the retry path must also move. `as_str()` is `&'static str`
        // (Copy); the owned strings are cloned once up front.
        let type_str = agreement_type.as_str();
        let realm_owned = realm_id.to_string();
        let by_owned = published_by.to_string();
        let source_owned = source.to_string();
        let db = &self.db;

        let attempt = |vno: i32| {
            let active = legal_agreement_version::ActiveModel {
                id: NotSet,
                realm_id: Set(Some(realm_owned.clone())),
                agreement_type: Set(type_str.to_string()),
                version_no: Set(vno),
                version_label: Set(label.clone()),
                content: Set(content.clone()),
                source: Set(source_owned.clone()),
                mode: Set("full_text".to_string()),
                external_url: Set(None),
                published_at: NotSet,
                published_by: Set(Some(by_owned.clone())),
            };
            active.insert(db)
        };

        let version_no = self
            .next_custom_version_no(realm_id, &agreement_type)
            .await?;

        match attempt(version_no).await {
            Ok(model) => Self::to_domain(model),
            Err(err)
                if is_unique_violation(
                    &err,
                    "legal_agreement_version_scope_type_version_unique",
                ) =>
            {
                // Recompute under the now-corrected max and retry once.
                let next = self
                    .next_custom_version_no(realm_id, &agreement_type)
                    .await?;
                match attempt(next).await {
                    Ok(model) => Self::to_domain(model),
                    // Still colliding — a concurrent publish raced ahead twice;
                    // surface a conflict so the caller re-reads current effective.
                    Err(err2)
                        if is_unique_violation(
                            &err2,
                            "legal_agreement_version_scope_type_version_unique",
                        ) =>
                    {
                        Err(LegalError::StaleVersion.into())
                    }
                    Err(other) => Err(CoreError::from(other)),
                }
            }
            Err(other) => Err(CoreError::from(other)),
        }
    }
}

impl LegalAgreementRepository for PostgresLegalAgreementRepository {
    /// Effective resolution: latest realm-scoped custom row wins; if none
    /// exists, fall back to the latest platform-default row. Returns
    /// `Ok(None)` only when neither exists (caller decides 404 / deploy fault).
    async fn current_effective(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> Result<Option<LegalAgreementVersion>, CoreError> {
        let custom = legal_agreement_version::Entity::find()
            .filter(legal_agreement_version::Column::RealmId.eq(realm_id))
            .filter(legal_agreement_version::Column::AgreementType.eq(agreement_type.as_str()))
            .order_by_desc(legal_agreement_version::Column::VersionNo)
            .one(&self.db)
            .await?;
        if let Some(model) = custom
            && AgreementSource::from(model.source.as_str()) == AgreementSource::Custom
        {
            return Self::to_domain(model).map(Some);
        }
        self.current_default(agreement_type).await
    }

    /// Latest platform-default row (`realm_id IS NULL`) for the type.
    async fn current_default(
        &self,
        agreement_type: AgreementType,
    ) -> Result<Option<LegalAgreementVersion>, CoreError> {
        let row = legal_agreement_version::Entity::find()
            .filter(legal_agreement_version::Column::RealmId.is_null())
            .filter(legal_agreement_version::Column::AgreementType.eq(agreement_type.as_str()))
            .order_by_desc(legal_agreement_version::Column::VersionNo)
            .one(&self.db)
            .await?;
        row.map(Self::to_domain).transpose()
    }

    /// History for the admin view: custom rows (version_no desc) first, then
    /// platform-default rows (version_no desc), truncated to `limit`. This
    /// ordering makes the realm's own evolution the dominant view and the
    /// platform baseline available as a trailing reference.
    async fn list_history(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        limit: u64,
    ) -> Result<Vec<LegalAgreementVersion>, CoreError> {
        let custom_rows = legal_agreement_version::Entity::find()
            .filter(legal_agreement_version::Column::RealmId.eq(realm_id))
            .filter(legal_agreement_version::Column::AgreementType.eq(agreement_type.as_str()))
            .order_by_desc(legal_agreement_version::Column::VersionNo)
            .all(&self.db)
            .await?;
        let default_rows = legal_agreement_version::Entity::find()
            .filter(legal_agreement_version::Column::RealmId.is_null())
            .filter(legal_agreement_version::Column::AgreementType.eq(agreement_type.as_str()))
            .order_by_desc(legal_agreement_version::Column::VersionNo)
            .all(&self.db)
            .await?;

        let mut combined: Vec<LegalAgreementVersion> = Vec::with_capacity(custom_rows.len());
        for m in custom_rows {
            combined.push(Self::to_domain(m)?);
        }
        for m in default_rows {
            combined.push(Self::to_domain(m)?);
        }
        if combined.len() > limit as usize {
            combined.truncate(limit as usize);
        }
        Ok(combined)
    }

    /// Fetch a single version by primary key (with full content body).
    async fn get_version_by_id(
        &self,
        version_id: Uuid,
    ) -> Result<Option<LegalAgreementVersion>, CoreError> {
        let row = legal_agreement_version::Entity::find_by_id(version_id)
            .one(&self.db)
            .await?;
        row.map(Self::to_domain).transpose()
    }

    /// Whether the realm has any custom version for the type.
    async fn has_custom(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> Result<bool, CoreError> {
        let latest = legal_agreement_version::Entity::find()
            .filter(legal_agreement_version::Column::RealmId.eq(realm_id))
            .filter(legal_agreement_version::Column::AgreementType.eq(agreement_type.as_str()))
            .order_by_desc(legal_agreement_version::Column::VersionNo)
            .one(&self.db)
            .await?;
        Ok(latest.is_some_and(|row| {
            AgreementSource::from(row.source.as_str()) == AgreementSource::Custom
        }))
    }

    /// Get the staged draft for `(realm_id, agreement_type)`, if any. Drafts
    /// live in a separate table and never feed into version resolution.
    async fn get_draft(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> Result<Option<LegalAgreementDraft>, CoreError> {
        let row = legal_agreement_draft::Entity::find()
            .filter(legal_agreement_draft::Column::RealmId.eq(realm_id))
            .filter(legal_agreement_draft::Column::AgreementType.eq(agreement_type.as_str()))
            .one(&self.db)
            .await?;
        row.map(Self::to_draft_domain).transpose()
    }

    /// Upsert the draft. `INSERT ... ON CONFLICT (realm_id, agreement_type) DO
    /// UPDATE` keeps exactly one draft per (realm, type); a repeat save
    /// overwrites `content` / `version_label` / `updated_at` / `updated_by`
    /// (last-write-wins). `id` is let-default on insert; on conflict the
    /// existing id is retained.
    async fn upsert_draft(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        content: serde_json::Value,
        version_label: Option<String>,
        mode: AgreementMode,
        external_url: Option<String>,
        updated_by: &str,
    ) -> Result<LegalAgreementDraft, CoreError> {
        // Pre-extract owned/copied captures so the insert closure borrows no
        // value the conflict-retry path must also move (mirrors
        // `publish_custom_version`). `as_str()` is `&'static str` (Copy).
        let type_str = agreement_type.as_str();
        let realm_owned = realm_id.to_string();
        let by_owned = updated_by.to_string();
        let db = &self.db;

        // Find-then-insert/update. The (realm_id, agreement_type) unique index
        // is the arbiter; the whole row is small (one per scope) so the
        // read-modify-write is cheap and matches the consent upsert pattern.
        let existing = legal_agreement_draft::Entity::find()
            .filter(legal_agreement_draft::Column::RealmId.eq(realm_id))
            .filter(legal_agreement_draft::Column::AgreementType.eq(type_str))
            .one(db)
            .await?;

        if let Some(model) = existing {
            let mut active: legal_agreement_draft::ActiveModel = model.into_active_model();
            active.content = Set(content);
            active.version_label = Set(version_label);
            active.mode = Set(mode.as_str().to_string());
            active.external_url = Set(external_url);
            active.updated_at = Set(chrono::Utc::now().into());
            active.updated_by = Set(Some(by_owned));
            let updated = active.update(db).await?;
            return Self::to_draft_domain(updated);
        }

        let active = legal_agreement_draft::ActiveModel {
            id: NotSet,
            realm_id: Set(realm_owned.clone()),
            agreement_type: Set(type_str.to_string()),
            content: Set(content.clone()),
            version_label: Set(version_label.clone()),
            mode: Set(mode.as_str().to_string()),
            external_url: Set(external_url.clone()),
            updated_at: NotSet,
            updated_by: Set(Some(by_owned.clone())),
        };
        match active.insert(db).await {
            Ok(model) => Self::to_draft_domain(model),
            Err(err) if is_unique_violation(&err, "legal_agreement_draft_realm_type_unique") => {
                // Lost the find-then-insert race: re-fetch and update in place.
                let existing = legal_agreement_draft::Entity::find()
                    .filter(legal_agreement_draft::Column::RealmId.eq(&realm_owned))
                    .filter(legal_agreement_draft::Column::AgreementType.eq(type_str))
                    .one(db)
                    .await?
                    .ok_or_else(|| {
                        CoreError::DatabaseError(
                            "draft row vanished between insert conflict and update retry"
                                .to_string(),
                        )
                    })?;
                let mut active: legal_agreement_draft::ActiveModel = existing.into_active_model();
                active.content = Set(content);
                active.version_label = Set(version_label);
                active.mode = Set(mode.as_str().to_string());
                active.external_url = Set(external_url);
                active.updated_at = Set(chrono::Utc::now().into());
                active.updated_by = Set(Some(by_owned));
                Self::to_draft_domain(active.update(db).await?)
            }
            Err(other) => Err(CoreError::from(other)),
        }
    }

    /// Delete the draft. Idempotent: deleting a missing draft is a no-op.
    async fn delete_draft(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
    ) -> Result<(), CoreError> {
        legal_agreement_draft::Entity::delete_many()
            .filter(legal_agreement_draft::Column::RealmId.eq(realm_id))
            .filter(legal_agreement_draft::Column::AgreementType.eq(agreement_type.as_str()))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    /// Publish a per-realm custom version.
    ///
    /// `version_no = max(version_no of realm's custom rows) + 1`. The
    /// `(COALESCE(realm_id,''), agreement_type, version_no)` expression unique
    /// index guards concurrent publishes: on a unique violation we recompute the
    /// next version_no once and retry; a second violation surfaces as
    /// `LegalError::StaleVersion` → `CoreError::Conflict` so the caller can
    /// re-read and decide. `id` / `published_at` rely on the DB column defaults
    /// (`uuidv7()` / `now()`); `source` is explicitly `custom`.
    async fn publish_custom_version(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        content: serde_json::Value,
        label: Option<String>,
        published_by: &str,
    ) -> Result<LegalAgreementVersion, CoreError> {
        self.publish_version_with_source(
            realm_id,
            agreement_type,
            content,
            label,
            published_by,
            AgreementSource::Custom.as_str(),
        )
        .await
    }

    async fn publish_default_follow_marker(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        content: serde_json::Value,
        label: Option<String>,
        published_by: &str,
    ) -> Result<LegalAgreementVersion, CoreError> {
        self.publish_version_with_source(
            realm_id,
            agreement_type,
            content,
            label,
            published_by,
            AgreementSource::Default.as_str(),
        )
        .await
    }

    async fn publish_link_version(
        &self,
        realm_id: &str,
        agreement_type: AgreementType,
        external_url: String,
        label: Option<String>,
        published_by: &str,
    ) -> Result<LegalAgreementVersion, CoreError> {
        let type_str = agreement_type.as_str();
        let realm_owned = realm_id.to_string();
        let by_owned = published_by.to_string();
        let db = &self.db;
        let attempt = |version_no: i32| {
            legal_agreement_version::ActiveModel {
                id: NotSet,
                realm_id: Set(Some(realm_owned.clone())),
                agreement_type: Set(type_str.to_string()),
                version_no: Set(version_no),
                version_label: Set(label.clone()),
                content: Set(serde_json::json!({})),
                source: Set("custom".to_string()),
                mode: Set("link".to_string()),
                external_url: Set(Some(external_url.clone())),
                published_at: NotSet,
                published_by: Set(Some(by_owned.clone())),
            }
            .insert(db)
        };
        let version_no = self
            .next_custom_version_no(realm_id, &agreement_type)
            .await?;
        match attempt(version_no).await {
            Ok(model) => Self::to_domain(model),
            Err(error)
                if is_unique_violation(
                    &error,
                    "legal_agreement_version_scope_type_version_unique",
                ) =>
            {
                let next = self
                    .next_custom_version_no(realm_id, &agreement_type)
                    .await?;
                attempt(next)
                    .await
                    .map_err(CoreError::from)
                    .and_then(Self::to_domain)
            }
            Err(error) => Err(CoreError::from(error)),
        }
    }
}

/// PostgreSQL implementation of [`UserConsentRepository`].
pub struct PostgresUserConsentRepository {
    db: DatabaseConnection,
}

impl PostgresUserConsentRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Map a `user_agreement_consent` row to the domain entity.
    fn to_domain(model: user_agreement_consent::Model) -> Result<UserAgreementConsent, CoreError> {
        Ok(UserAgreementConsent {
            id: model.id,
            user_id: model.user_id,
            realm_id: model.realm_id,
            agreement_type: AgreementType::try_from(model.agreement_type.as_str())
                .map_err(CoreError::InternalServerError)?,
            consented_version_id: model.consented_version_id,
            consented_at: chrono::DateTime::<chrono::Utc>::from(model.consented_at),
        })
    }
}

impl UserConsentRepository for PostgresUserConsentRepository {
    /// Idempotent upsert on `(user_id, agreement_type)`. An existing row is
    /// updated in place (refresh `consented_version_id`, `consented_at`,
    /// `realm_id`); a missing row is inserted. The unique index
    /// `user_agreement_consent_user_type_unique` is the final arbiter under
    /// concurrent consent for the same user — a collision during the
    /// find-then-insert window is retried as an update.
    async fn upsert_consent(
        &self,
        user_id: Uuid,
        realm_id: &str,
        agreement_type: AgreementType,
        version_id: Uuid,
    ) -> Result<(), CoreError> {
        let existing = user_agreement_consent::Entity::find()
            .filter(user_agreement_consent::Column::UserId.eq(user_id))
            .filter(user_agreement_consent::Column::AgreementType.eq(agreement_type.as_str()))
            .one(&self.db)
            .await?;

        if let Some(model) = existing {
            let mut active: user_agreement_consent::ActiveModel = model.into_active_model();
            active.realm_id = Set(realm_id.to_string());
            active.consented_version_id = Set(version_id);
            // Re-consent is observable: refresh the timestamp explicitly rather
            // than relying on the DB `now()` default (SeaORM omits NotSet from
            // UPDATE, so the old value would otherwise persist).
            active.consented_at = Set(chrono::Utc::now().into());
            active.update(&self.db).await?;
            return Ok(());
        }

        let active = user_agreement_consent::ActiveModel {
            id: NotSet,
            user_id: Set(user_id),
            realm_id: Set(realm_id.to_string()),
            agreement_type: Set(agreement_type.as_str().to_string()),
            consented_version_id: Set(version_id),
            consented_at: NotSet,
        };
        match active.insert(&self.db).await {
            Ok(_) => Ok(()),
            Err(err) if is_unique_violation(&err, "user_agreement_consent_user_type_unique") => {
                // Lost the find-then-insert race against another concurrent
                // consent for the same user/type: retry as an update.
                let existing = user_agreement_consent::Entity::find()
                    .filter(user_agreement_consent::Column::UserId.eq(user_id))
                    .filter(
                        user_agreement_consent::Column::AgreementType.eq(agreement_type.as_str()),
                    )
                    .one(&self.db)
                    .await?
                    .ok_or_else(|| {
                        CoreError::DatabaseError(
                            "consent row vanished between insert conflict and update retry"
                                .to_string(),
                        )
                    })?;
                let mut active: user_agreement_consent::ActiveModel = existing.into_active_model();
                active.realm_id = Set(realm_id.to_string());
                active.consented_version_id = Set(version_id);
                active.consented_at = Set(chrono::Utc::now().into());
                active.update(&self.db).await?;
                Ok(())
            }
            Err(other) => Err(CoreError::from(other)),
        }
    }

    async fn get_consent(
        &self,
        user_id: Uuid,
        agreement_type: AgreementType,
    ) -> Result<Option<UserAgreementConsent>, CoreError> {
        let row = user_agreement_consent::Entity::find()
            .filter(user_agreement_consent::Column::UserId.eq(user_id))
            .filter(user_agreement_consent::Column::AgreementType.eq(agreement_type.as_str()))
            .one(&self.db)
            .await?;
        row.map(Self::to_domain).transpose()
    }
}

/// Detect a Postgres unique-violation (SQLSTATE 23505) for `constraint`.
///
/// sea-orm 1.1 surfaces a Postgres `23505` as a `Query`/`Exec` wrapping
/// `RuntimeErr::SqlxError`; `PgDatabaseError::code()` returns the SQLSTATE.
/// When the SQLSTATE is unavailable (driver/version drift), fall back to
/// matching the explicit `constraint` name or a generic `duplicate key` token
/// in the message — mirroring the billing repo's `classify_from_message`
/// resilience. Each call site touches only one table, so the constraint name
/// is the message-level discriminator, not a real branch at runtime.
fn is_unique_violation(err: &DbErr, constraint: &str) -> bool {
    if let Some(sqlx_err) = sqlx_error(err)
        && sqlx_err.code() == "23505"
    {
        return true;
    }
    let msg = err.to_string();
    msg.contains(constraint) || msg.contains("duplicate key value")
}

/// Unwrap the underlying sqlx `PgDatabaseError` from a sea-orm `DbErr`, if any.
fn sqlx_error(err: &DbErr) -> Option<&sqlx::postgres::PgDatabaseError> {
    let runtime = match err {
        DbErr::Query(r) | DbErr::Exec(r) | DbErr::Conn(r) => r,
        _ => return None,
    };
    match runtime {
        RuntimeErr::SqlxError(sqlx::error::Error::Database(db)) => db.try_downcast_ref(),
        _ => None,
    }
}
