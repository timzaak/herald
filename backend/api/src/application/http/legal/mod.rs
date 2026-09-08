// Legal agreements (public view) + self-service consent gate.

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode, header::USER_AGENT},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use herald_api_base::application::http::auth::util::{ClientIp, require_permission};
use herald_core::domain::audit::AuditContext;
use herald_core::domain::authentication::Identity;
use herald_core::domain::legal::ConsentSource;
use herald_core::domain::legal::entities::{
    AgreementMode, AgreementSource, AgreementType, ConsentStatusItem, LegalAgreementSummary,
    LegalAgreementVersion,
};

use crate::application::http::server::api_entities::{ApiError, ErrorResponse};
use crate::application::http::state::AppState;

/// Query params shared by the public agreement endpoints. `locale` selects
/// which body is returned in `LegalAgreementDetail.content`; missing/unknown
/// locale falls back to the agreement's default locale (first map key).
#[derive(Debug, Deserialize)]
pub struct LocaleQuery {
    pub locale: Option<String>,
}

/// Full agreement detail (public GET /agreements/{type}).
///
/// Mirrors [`LegalAgreementSummary`] plus the localized `content` body and the
/// optional `version_label`. Summary fields (`title`/`summary`) are always
/// `None` today — the domain `LegalAgreementVersion` has no such columns —
/// but they remain in the schema so the public list/detail shapes line up
/// (and can be populated later without a breaking change).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LegalAgreementDetail {
    pub agreement_type: String,
    pub version_id: Uuid,
    pub version_no: i32,
    pub effective_at: chrono::DateTime<chrono::Utc>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub content: serde_json::Value,
    pub version_label: Option<String>,
    pub mode: AgreementMode,
    pub external_url: Option<String>,
}

/// GET /agreements response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgreementsResponse {
    pub agreements: Vec<LegalAgreementSummary>,
}

/// GET /consent/status response. Reuses the domain [`ConsentStatusItem`] as
/// the response item so the OpenAPI shape stays in lockstep with the
/// reconsent verdict computed by the service.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ConsentStatusResponse {
    pub items: Vec<ConsentStatusItem>,
}

/// POST /consent request body.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RecordConsentRequest {
    pub agreements: Vec<RecordConsentItem>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RecordConsentItem {
    pub agreement_type: String,
    pub version_id: Uuid,
}

fn to_summary(v: &LegalAgreementVersion) -> LegalAgreementSummary {
    LegalAgreementSummary {
        agreement_type: v.agreement_type.as_str().to_string(),
        version_id: v.id,
        version_no: v.version_no,
        effective_at: v.published_at,
        title: None,
        summary: None,
        mode: v.mode,
        external_url: v.external_url.clone(),
    }
}

fn to_detail(v: &LegalAgreementVersion, locale: Option<&str>) -> LegalAgreementDetail {
    LegalAgreementDetail {
        agreement_type: v.agreement_type.as_str().to_string(),
        version_id: v.id,
        version_no: v.version_no,
        effective_at: v.published_at,
        title: None,
        summary: None,
        content: pick_locale(&v.content, locale),
        version_label: v.version_label.clone(),
        mode: v.mode,
        external_url: v.external_url.clone(),
    }
}

/// Select a single locale body out of the locale→body map. When `locale` is
/// `None` or absent from the map, fall back to the map's first key (the
/// publisher's default locale). Non-object content is returned as-is so a
/// stray row never panics the serializer.
fn pick_locale(content: &serde_json::Value, locale: Option<&str>) -> serde_json::Value {
    let Some(map) = content.as_object() else {
        return content.clone();
    };
    if let Some(loc) = locale
        && let Some(body) = map.get(loc)
    {
        return body.clone();
    }
    // Fall back to the first entry (default locale), or the original map if empty.
    map.iter()
        .next()
        .map(|(_, body)| body.clone())
        .unwrap_or_else(|| content.clone())
}

/// List the current effective agreements for a realm (Terms + Privacy).
///
/// Public — no Bearer identity. Each agreement type resolves to its current
/// effective version (realm custom if present, otherwise platform default);
/// missing types are skipped. Returns 404 when no type resolves, which signals
/// a seed-missing deployment anomaly (the platform default templates should
/// always be present).
#[utoipa::path(
    get,
    path = "/api/legal/{realmId}/agreements",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("locale" = Option<String>, Query, description = "Preferred locale for the returned body (falls back to default locale)")
    ),
    responses(
        (status = 200, description = "Current effective agreement summaries", body = AgreementsResponse),
        (status = 404, description = "No effective agreement deployed for this realm", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn list_agreements(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Query(query): Query<LocaleQuery>,
) -> Result<Json<AgreementsResponse>, ApiError> {
    let _ = query; // summaries don't carry bodies; locale only matters for detail
    let service = &state.legal_service;

    let mut agreements = Vec::new();
    for agreement_type in [AgreementType::TermsOfService, AgreementType::PrivacyPolicy] {
        if let Some(version) = service.current_effective(&realm_id, agreement_type).await? {
            agreements.push(to_summary(&version));
        }
    }

    if agreements.is_empty() {
        return Err(ApiError::not_found(
            "No effective legal agreement deployed for this realm",
        ));
    }

    Ok(Json(AgreementsResponse { agreements }))
}

/// Get the full localized detail of a single agreement type for a realm.
///
/// Public — no Bearer identity. Unknown `agreementType` → 400; no effective
/// version deployed → 404.
#[utoipa::path(
    get,
    path = "/api/legal/{realmId}/agreements/{agreementType}",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("agreementType" = String, Path, description = "Agreement type: terms_of_service | privacy_policy"),
        ("locale" = Option<String>, Query, description = "Preferred locale (falls back to default locale)")
    ),
    responses(
        (status = 200, description = "Agreement detail with localized body", body = LegalAgreementDetail),
        (status = 400, description = "Unknown agreement type", body = ErrorResponse),
        (status = 404, description = "No effective agreement deployed for this type", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn get_agreement(
    State(state): State<AppState>,
    Path((realm_id, agreement_type)): Path<(String, String)>,
    Query(query): Query<LocaleQuery>,
) -> Result<Json<LegalAgreementDetail>, ApiError> {
    let agreement_type =
        AgreementType::try_from(agreement_type.as_str()).map_err(ApiError::bad_request)?;

    let version = state
        .legal_service
        .current_effective(&realm_id, agreement_type)
        .await?
        .ok_or_else(|| {
            ApiError::not_found("No effective legal agreement deployed for this type")
        })?;

    Ok(Json(to_detail(&version, query.locale.as_deref())))
}

/// Reconsent gate verdict for the calling user across both agreement types.
///
/// Self-service — requires Bearer identity. Cross-realm access is rejected
/// with 403 (each user may only inspect their own realm's consent state).
#[utoipa::path(
    get,
    path = "/api/legal/{realmId}/consent/status",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Per-agreement reconsent verdict", body = ConsentStatusResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Identity does not belong to this realm", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn get_consent_status(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Result<Json<ConsentStatusResponse>, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Identity does not belong to this realm",
        ));
    }

    let user_id = parse_user_id(identity.user_id())?;
    let items = state
        .legal_service
        .consent_status(user_id, &realm_id)
        .await?;
    Ok(Json(ConsentStatusResponse { items }))
}

/// Record the user's explicit consent to one or more agreement versions.
///
/// Self-service — requires Bearer identity. Each `version_id` must equal the
/// current effective version for its type (enforced in the service layer,
/// BE-D04); a stale version surfaces as 409 so the client re-reads the
/// effective version. Returns 204 on success. The upsert is idempotent on a
/// repeat of the same version.
#[utoipa::path(
    post,
    path = "/api/legal/{realmId}/consent",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = RecordConsentRequest,
    responses(
        (status = 204, description = "Consent recorded"),
        (status = 400, description = "Unknown agreement type", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Identity does not belong to this realm", body = ErrorResponse),
        (status = 409, description = "version_id is not the current effective version", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn record_consent(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(payload): Json<RecordConsentRequest>,
) -> Result<StatusCode, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Identity does not belong to this realm",
        ));
    }

    let user_id = parse_user_id(identity.user_id())?;

    let mut items = Vec::with_capacity(payload.agreements.len());
    for item in payload.agreements {
        let agreement_type =
            AgreementType::try_from(item.agreement_type.as_str()).map_err(ApiError::bad_request)?;
        items.push((agreement_type, item.version_id));
    }

    let user_agent = headers
        .get(USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let ctx = AuditContext::user(&identity, ip, user_agent);

    state
        .legal_service
        .record_consent(user_id, &realm_id, items, ConsentSource::Explicit, ctx)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// `identity.user_id()` is a `String`; the legal service takes a `Uuid`. A
/// malformed id means the identity was constructed without a valid user row —
/// surface as 500 rather than silently failing the consent write.
fn parse_user_id(raw: String) -> Result<Uuid, ApiError> {
    Uuid::parse_str(&raw)
        .map_err(|e| ApiError::internal(format!("identity user_id is not a valid uuid: {e}")))
}

// ----------------------------------------------------------------------------
// Admin DTOs
// ----------------------------------------------------------------------------

/// History entry for the admin view: the version-stable fields without the
/// (potentially large) localized `content` body.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LegalAgreementVersionSummary {
    pub version_id: Uuid,
    pub version_no: i32,
    pub effective_at: chrono::DateTime<chrono::Utc>,
    pub source: AgreementSource,
    pub version_label: Option<String>,
    pub mode: AgreementMode,
    pub external_url: Option<String>,
}

/// Single version detail (admin GET by version id). Carries the full localized
/// `content` map (same shape as `LegalAgreementDraftResponse.content`) so the
/// admin "view past version" dialog can read `content.en` exactly like the draft
/// preview. `effective_at` is the version's `published_at`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LegalAgreementVersionDetailResponse {
    pub agreement_type: AgreementType,
    pub version_no: i32,
    pub version_label: Option<String>,
    pub content: serde_json::Value,
    pub effective_at: chrono::DateTime<chrono::Utc>,
    pub mode: AgreementMode,
    pub external_url: Option<String>,
}

/// Admin agreement view. `source` reflects whether a realm has any custom
/// version for this type (Custom) or only the platform default (Default).
/// `current_version` is the resolved effective summary; `history` lists prior
/// versions (custom-first, then the default fallback).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AdminAgreementView {
    pub agreement_type: AgreementType,
    pub source: AgreementSource,
    pub current_version: LegalAgreementSummary,
    pub history: Vec<LegalAgreementVersionSummary>,
}

/// GET admin agreements response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AdminAgreementsResponse {
    pub agreements: Vec<AdminAgreementView>,
}

/// PUT publish-custom request body. `content` is a locale → body map and must
/// contain at least one entry (validated by the service layer).
#[derive(Debug, Deserialize, ToSchema)]
pub struct PublishCustomRequest {
    pub content: serde_json::Value,
    pub version_label: Option<String>,
    pub mode: Option<AgreementMode>,
    pub external_url: Option<String>,
}

/// PUT publish / DELETE revert response: the newly published version's stable
/// identifiers. For revert this is a brand-new snapshot version (new `version_id`),
/// so clients can assert the id changed to trigger user reconsent.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublishVersionResponse {
    pub version_id: Uuid,
    pub version_no: i32,
    pub effective_at: chrono::DateTime<chrono::Utc>,
    pub mode: AgreementMode,
    pub external_url: Option<String>,
}

/// GET/PUT draft response: the staged draft's stable fields plus the localized
/// `content` body. Mirrors the locale map shape of a published version so the
/// admin form can render/edit it the same way.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LegalAgreementDraftResponse {
    pub agreement_type: AgreementType,
    pub content: serde_json::Value,
    pub version_label: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub mode: AgreementMode,
    pub external_url: Option<String>,
    pub pending_version_no: i32,
}

/// PUT draft request body. `content` is a locale → body map and must contain at
/// least one entry (validated by the service layer, same rule as publish).
#[derive(Debug, Deserialize, ToSchema)]
pub struct SaveDraftRequest {
    pub content: serde_json::Value,
    pub version_label: Option<String>,
    pub mode: Option<AgreementMode>,
    pub external_url: Option<String>,
}

/// POST publish-from-draft request body. The entire body is optional — when
/// omitted, the draft's stored `version_label` is used; when present,
/// `version_label` overrides the draft's label for this publish only.
///
/// Wire contract: the handler takes `Option<Json<Self>>`. Clients SHOULD send
/// an empty object (`{}`) when they have no override — this is what the admin
/// UI does and what the OpenAPI examples show. A completely bodyless POST is
/// also accepted (resolves to `None` → no override), but the `{}` form is the
/// stable, documented call shape so future extractor changes don't break
/// clients.
#[derive(Debug, Deserialize, ToSchema, Default)]
pub struct PublishFromDraftRequest {
    pub version_label: Option<String>,
}

fn to_version_summary(v: &LegalAgreementVersion) -> LegalAgreementVersionSummary {
    LegalAgreementVersionSummary {
        version_id: v.id,
        version_no: v.version_no,
        effective_at: v.published_at,
        source: v.source.clone(),
        version_label: v.version_label.clone(),
        mode: v.mode,
        external_url: v.external_url.clone(),
    }
}

/// Build the [`AuditContext`] for an admin operation. Admin ops are performed by
/// an authenticated user with `ActorType::Admin` (the realm operator managing
/// legal agreements), mirroring how the consent handler derives its actor.
fn admin_actor(
    identity: &Identity,
    ip: Option<String>,
    user_agent: Option<String>,
) -> AuditContext {
    AuditContext::admin(identity, ip.unwrap_or_default(), user_agent)
}

// ----------------------------------------------------------------------------
// Admin handlers
// ----------------------------------------------------------------------------

/// List both agreement types with their source, current effective version, and
/// version history for the realm.
///
/// Admin — requires first-party Bearer + `settings.view` + `has_access_to_realm`.
/// `source` is derived per-type from `has_custom` (Custom when a realm custom
/// version exists, otherwise Default).
#[utoipa::path(
    get,
    path = "/api/legal/admin/{realmId}/agreements",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Admin agreement views with history", body = AdminAgreementsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden (missing settings.view or cross-realm)", body = ErrorResponse),
        (status = 404, description = "No effective agreement deployed for this realm", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn admin_list_agreements(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Result<Json<AdminAgreementsResponse>, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Identity does not belong to this realm",
        ));
    }
    require_permission(
        &state,
        &realm_id,
        &identity.user_id(),
        "settings",
        "view",
        "settings.view",
    )
    .await?;

    let service = &state.legal_service;
    let mut agreements = Vec::new();
    for agreement_type in [AgreementType::TermsOfService, AgreementType::PrivacyPolicy] {
        let current = match service
            .current_effective(&realm_id, agreement_type.clone())
            .await?
        {
            Some(v) => v,
            None => continue,
        };
        let has_custom = service
            .has_custom(&realm_id, agreement_type.clone())
            .await?;
        let source = if has_custom {
            AgreementSource::Custom
        } else {
            AgreementSource::Default
        };
        let history = service
            .list_history(&realm_id, agreement_type.clone(), 50)
            .await?;
        agreements.push(AdminAgreementView {
            agreement_type,
            source,
            current_version: to_summary(&current),
            history: history.iter().map(to_version_summary).collect(),
        });
    }

    if agreements.is_empty() {
        return Err(ApiError::not_found(
            "No effective legal agreement deployed for this realm",
        ));
    }

    Ok(Json(AdminAgreementsResponse { agreements }))
}

/// Get a single published version's full body by id (admin history view).
///
/// Admin — requires first-party Bearer + `settings.view` + `has_access_to_realm`
/// (same gate as `admin_list_agreements`, since this is a read of the same
/// history the list already exposes). Returns 404 when the id does not resolve.
/// The `realmId` path segment scopes the permission check; the version itself is
/// looked up by primary key (history rows already surfaced by the list endpoint
/// are guaranteed to belong to the realm or be the platform default).
#[utoipa::path(
    get,
    path = "/api/legal/admin/{realmId}/agreements/versions/{versionId}",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("versionId" = Uuid, Path, description = "Agreement version ID")
    ),
    responses(
        (status = 200, description = "Version with full localized body", body = LegalAgreementVersionDetailResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden (missing settings.view or cross-realm)", body = ErrorResponse),
        (status = 404, description = "Version not found", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn admin_get_version(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, version_id)): Path<(String, Uuid)>,
) -> Result<Json<LegalAgreementVersionDetailResponse>, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Identity does not belong to this realm",
        ));
    }
    require_permission(
        &state,
        &realm_id,
        &identity.user_id(),
        "settings",
        "view",
        "settings.view",
    )
    .await?;

    let version = state
        .legal_service
        .get_version_by_id(version_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Agreement version not found"))?;

    // version_id is a client-supplied primary key: the row must belong to the
    // path realm (or be the platform default) — otherwise a realm admin could
    // read another realm's custom agreement body by supplying a foreign id.
    if version.realm_id.as_deref().is_some_and(|id| id != realm_id) {
        return Err(ApiError::not_found("Agreement version not found"));
    }

    Ok(Json(LegalAgreementVersionDetailResponse {
        agreement_type: version.agreement_type,
        version_no: version.version_no,
        version_label: version.version_label,
        content: version.content,
        effective_at: version.published_at,
        mode: version.mode,
        external_url: version.external_url,
    }))
}

/// Publish a new per-realm custom agreement version.
///
/// Admin — requires first-party Bearer + `settings.manage` + `has_access_to_realm`.
/// Unknown `agreementType` → 400; non-object/empty `content` → 400 (also
/// enforced in the service). On success the service records an
/// `agreement.published` audit event and returns the new version.
#[utoipa::path(
    put,
    path = "/api/legal/admin/{realmId}/agreements/{agreementType}",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("agreementType" = String, Path, description = "Agreement type: terms_of_service | privacy_policy")
    ),
    request_body = PublishCustomRequest,
    responses(
        (status = 200, description = "Newly published version", body = PublishVersionResponse),
        (status = 400, description = "Unknown agreement type or invalid content", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden (missing settings.manage or cross-realm)", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn admin_publish_custom(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, agreement_type)): Path<(String, String)>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(payload): Json<PublishCustomRequest>,
) -> Result<Json<PublishVersionResponse>, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Identity does not belong to this realm",
        ));
    }
    require_permission(
        &state,
        &realm_id,
        &identity.user_id(),
        "settings",
        "manage",
        "settings.manage",
    )
    .await?;

    let agreement_type =
        AgreementType::try_from(agreement_type.as_str()).map_err(ApiError::bad_request)?;

    let mode = payload.mode.unwrap_or(AgreementMode::FullText);
    if mode == AgreementMode::FullText
        && (!payload.content.is_object()
            || payload
                .content
                .as_object()
                .map(|m| m.is_empty())
                .unwrap_or(true))
    {
        return Err(ApiError::bad_request(
            "content must be a non-empty locale map",
        ));
    }

    let user_agent = headers
        .get(USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let actor = admin_actor(&identity, Some(ip), user_agent);

    let new_version = if mode == AgreementMode::Link {
        state
            .legal_service
            .save_draft_with_mode(
                &realm_id,
                agreement_type.clone(),
                serde_json::json!({}),
                payload.version_label.clone(),
                mode,
                payload.external_url,
                &identity.user_id(),
            )
            .await?;
        state
            .legal_service
            .publish_from_draft(
                &realm_id,
                agreement_type,
                payload.version_label,
                &identity.user_id(),
                actor,
            )
            .await?
    } else {
        state
            .legal_service
            .publish_custom(
                &realm_id,
                agreement_type,
                payload.content,
                payload.version_label,
                &identity.user_id(),
                actor,
            )
            .await?
    };

    Ok(Json(PublishVersionResponse {
        version_id: new_version.id,
        version_no: new_version.version_no,
        effective_at: new_version.published_at,
        mode: new_version.mode,
        external_url: new_version.external_url,
    }))
}

/// Revert a realm's agreement to the platform default.
///
/// Admin — requires first-party Bearer + `settings.manage` + `has_access_to_realm`.
/// Implemented as **default-follow semantics** in the service: a realm-scoped
/// marker version is appended with `source = default` (new `version_id`,
/// monotonic `version_no`), after which effective resolution follows the
/// latest platform default rather than a frozen copy. History remains
/// append-only; no prior rows are deleted. The handler is a thin pass-through.
#[utoipa::path(
    delete,
    path = "/api/legal/admin/{realmId}/agreements/{agreementType}/custom",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("agreementType" = String, Path, description = "Agreement type: terms_of_service | privacy_policy")
    ),
    responses(
        (status = 200, description = "Newly appended default-follow marker version", body = PublishVersionResponse),
        (status = 400, description = "Unknown agreement type", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden (missing settings.manage or cross-realm)", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn admin_revert_to_default(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, agreement_type)): Path<(String, String)>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Result<Json<PublishVersionResponse>, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Identity does not belong to this realm",
        ));
    }
    require_permission(
        &state,
        &realm_id,
        &identity.user_id(),
        "settings",
        "manage",
        "settings.manage",
    )
    .await?;

    let agreement_type =
        AgreementType::try_from(agreement_type.as_str()).map_err(ApiError::bad_request)?;

    let user_agent = headers
        .get(USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let actor = admin_actor(&identity, Some(ip), user_agent);

    let new_version = state
        .legal_service
        .revert_to_default(&realm_id, agreement_type, &identity.user_id(), actor)
        .await?;

    Ok(Json(PublishVersionResponse {
        version_id: new_version.id,
        version_no: new_version.version_no,
        effective_at: new_version.published_at,
        mode: new_version.mode,
        external_url: new_version.external_url,
    }))
}

// ----------------------------------------------------------------------------
// Admin draft handlers
// ----------------------------------------------------------------------------

/// Get the staged draft for an agreement type.
///
/// Admin — requires first-party Bearer + `settings.manage` + `has_access_to_realm`.
/// Returns 404 when no draft exists for the type (the admin form treats this as
/// "start a new draft").
#[utoipa::path(
    get,
    path = "/api/legal/admin/{realmId}/agreements/{agreementType}/draft",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("agreementType" = String, Path, description = "Agreement type: terms_of_service | privacy_policy")
    ),
    responses(
        (status = 200, description = "Staged draft with localized body", body = LegalAgreementDraftResponse),
        (status = 400, description = "Unknown agreement type", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden (missing settings.manage or cross-realm)", body = ErrorResponse),
        (status = 404, description = "No draft saved for this type", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn admin_get_draft(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, agreement_type)): Path<(String, String)>,
) -> Result<Json<LegalAgreementDraftResponse>, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Identity does not belong to this realm",
        ));
    }
    require_permission(
        &state,
        &realm_id,
        &identity.user_id(),
        "settings",
        "manage",
        "settings.manage",
    )
    .await?;

    let agreement_type =
        AgreementType::try_from(agreement_type.as_str()).map_err(ApiError::bad_request)?;

    let draft = state
        .legal_service
        .get_draft(&realm_id, agreement_type.clone())
        .await?
        .ok_or_else(|| ApiError::not_found("No draft saved for this agreement type"))?;
    let pending_version_no = state
        .legal_service
        .current_effective(&realm_id, agreement_type)
        .await?
        .map_or(1, |version| version.version_no + 1);

    Ok(Json(LegalAgreementDraftResponse {
        agreement_type: draft.agreement_type,
        content: draft.content,
        version_label: draft.version_label,
        updated_at: draft.updated_at,
        mode: draft.mode,
        external_url: draft.external_url,
        pending_version_no,
    }))
}

/// Save (upsert) a draft for an agreement type.
///
/// Admin — requires first-party Bearer + `settings.manage` + `has_access_to_realm`.
/// A repeat save overwrites the prior draft. Does NOT publish — the agreement
/// stays unchanged for end users until POST `/publish`.
#[utoipa::path(
    put,
    path = "/api/legal/admin/{realmId}/agreements/{agreementType}/draft",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("agreementType" = String, Path, description = "Agreement type: terms_of_service | privacy_policy")
    ),
    request_body = SaveDraftRequest,
    responses(
        (status = 200, description = "Saved draft", body = LegalAgreementDraftResponse),
        (status = 400, description = "Unknown agreement type or invalid content", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden (missing settings.manage or cross-realm)", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn admin_save_draft(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, agreement_type)): Path<(String, String)>,
    Json(payload): Json<SaveDraftRequest>,
) -> Result<Json<LegalAgreementDraftResponse>, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Identity does not belong to this realm",
        ));
    }
    require_permission(
        &state,
        &realm_id,
        &identity.user_id(),
        "settings",
        "manage",
        "settings.manage",
    )
    .await?;

    let agreement_type =
        AgreementType::try_from(agreement_type.as_str()).map_err(ApiError::bad_request)?;

    let mode = payload.mode.unwrap_or(AgreementMode::FullText);
    if mode == AgreementMode::FullText
        && (!payload.content.is_object()
            || payload
                .content
                .as_object()
                .map(|m| m.is_empty())
                .unwrap_or(true))
    {
        return Err(ApiError::bad_request(
            "content must be a non-empty locale map",
        ));
    }

    let draft = state
        .legal_service
        .save_draft_with_mode(
            &realm_id,
            agreement_type.clone(),
            payload.content,
            payload.version_label,
            mode,
            payload.external_url,
            &identity.user_id(),
        )
        .await?;

    let pending_version_no = state
        .legal_service
        .current_effective(&realm_id, agreement_type)
        .await?
        .map_or(1, |version| version.version_no + 1);
    Ok(Json(LegalAgreementDraftResponse {
        agreement_type: draft.agreement_type,
        content: draft.content,
        version_label: draft.version_label,
        updated_at: draft.updated_at,
        mode: draft.mode,
        external_url: draft.external_url,
        pending_version_no,
    }))
}

/// Publish the staged draft as a new effective version.
///
/// Admin — requires first-party Bearer + `settings.manage` + `has_access_to_realm`.
/// Reads the draft, creates a new immutable `legal_agreement_version` row
/// (advancing `version_no`, recording an `agreement.published` audit event),
/// and clears the draft. Returns 404 when no draft exists for the type. This is
/// the only path the admin UI uses to publish — there is no "publish without a
/// draft" entry in the UI (the legacy `PUT /agreements/{type}` handler remains
/// for backward compatibility).
#[utoipa::path(
    post,
    path = "/api/legal/admin/{realmId}/agreements/{agreementType}/publish",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("agreementType" = String, Path, description = "Agreement type: terms_of_service | privacy_policy")
    ),
    request_body = Option<PublishFromDraftRequest>,
    responses(
        (status = 200, description = "Newly published version", body = PublishVersionResponse),
        (status = 400, description = "Unknown agreement type", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden (missing settings.manage or cross-realm)", body = ErrorResponse),
        (status = 404, description = "No draft saved for this type", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn admin_publish_from_draft(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, agreement_type)): Path<(String, String)>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    payload: Option<Json<PublishFromDraftRequest>>,
) -> Result<Json<PublishVersionResponse>, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Identity does not belong to this realm",
        ));
    }
    require_permission(
        &state,
        &realm_id,
        &identity.user_id(),
        "settings",
        "manage",
        "settings.manage",
    )
    .await?;

    let agreement_type =
        AgreementType::try_from(agreement_type.as_str()).map_err(ApiError::bad_request)?;

    let version_label_override = payload.and_then(|Json(p)| p.version_label);

    let user_agent = headers
        .get(USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let actor = admin_actor(&identity, Some(ip), user_agent);

    let new_version = state
        .legal_service
        .publish_from_draft(
            &realm_id,
            agreement_type,
            version_label_override,
            &identity.user_id(),
            actor,
        )
        .await?;

    Ok(Json(PublishVersionResponse {
        version_id: new_version.id,
        version_no: new_version.version_no,
        effective_at: new_version.published_at,
        mode: new_version.mode,
        external_url: new_version.external_url,
    }))
}

/// Discard the staged draft.
///
/// Admin — requires first-party Bearer + `settings.manage` + `has_access_to_realm`.
/// Idempotent: discarding a missing draft returns 204. The published version
/// table is untouched.
#[utoipa::path(
    delete,
    path = "/api/legal/admin/{realmId}/agreements/{agreementType}/draft",
    tag = "legal",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("agreementType" = String, Path, description = "Agreement type: terms_of_service | privacy_policy")
    ),
    responses(
        (status = 204, description = "Draft discarded (idempotent)"),
        (status = 400, description = "Unknown agreement type", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden (missing settings.manage or cross-realm)", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn admin_discard_draft(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, agreement_type)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Identity does not belong to this realm",
        ));
    }
    require_permission(
        &state,
        &realm_id,
        &identity.user_id(),
        "settings",
        "manage",
        "settings.manage",
    )
    .await?;

    let agreement_type =
        AgreementType::try_from(agreement_type.as_str()).map_err(ApiError::bad_request)?;

    state
        .legal_service
        .discard_draft(&realm_id, agreement_type)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}
