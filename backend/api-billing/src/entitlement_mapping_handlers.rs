use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use serde::Serialize;
use uuid::Uuid;
use validator::Validate;

use crate::credit_bucket_handlers::require_points_manage_permission;
use crate::handlers::require_billing_permission;
use crate::types::{
    BatchUpdateEntitlementMappingsRequest, BatchUpdateEntitlementMappingsResponse,
    CreateEntitlementMappingRequest, EntitlementMappingListResponse, EntitlementMappingQuery,
    EntitlementMappingResponse, EntitlementQuotaWindowResponse, OneTimeMappingItem,
    OneTimeMappingListResponse, PartialSyncError, PointDistributionRuleResponse,
    PointDistributionRuleWrite, ProviderProductInfo, SyncProviderRequest, SyncProviderResponse,
    UpdateEntitlementMappingRequest,
};
use herald_api_base::application::http::server::api_entities::{
    ApiError, DistributionRuleErrorResponse, distribution_rule_validation_error,
};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::billing::CreateEntitlementMappingInput;
use herald_core::domain::billing::entities::EntitlementMapping;
use herald_core::domain::billing::{
    BatchMappingError, BillingRepository, SyncStatus, validate_granted_role_ids,
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::points::derive_window_key;
use herald_core::domain::points::entities::QuotaWindow;
use herald_core::domain::points::{
    DistributionPolicy, DistributionRuleOwner, DistributionTrigger, PointsDistributionRule,
    RuleUpsert, validate_rule_for_owner,
};

/// 409 `mapping_in_use` body for a batch save blocked by the active-subscription
/// lock. The whole batch transaction is rolled back.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MappingActiveSubscriptionLockErrorBody {
    pub code: &'static str,
    pub active_subscriptions: i64,
}

/// 400 `role_not_in_realm` body for a batch save where a `grantedRoleIds`
/// whole batch transaction is rolled back.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MappingRoleNotInRealmErrorBody {
    pub code: &'static str,
    pub role_id: Uuid,
    pub realm_id: String,
}

/// Convert a domain distribution rule into the read-side API DTO.
pub(crate) fn rule_to_response(rule: PointsDistributionRule) -> PointDistributionRuleResponse {
    let trigger_sources: Vec<String> = rule.trigger_sources.iter().map(|t| t.to_string()).collect();
    let grant_mode = rule.policy.grant_mode().to_string();
    let (points_amount, validity_days, grant_period_type, quota_windows) = match rule.policy {
        DistributionPolicy::Fixed {
            amount,
            validity_days,
            grant_period_type,
        } => (
            Some(amount),
            Some(validity_days),
            grant_period_type.map(|t| t.to_string()),
            None,
        ),
        DistributionPolicy::Quota { windows } => {
            let qw = Some(
                windows
                    .into_iter()
                    .map(|w| EntitlementQuotaWindowResponse {
                        window_seconds: w.window_seconds,
                        limit: w.limit,
                        key: w.key,
                    })
                    .collect(),
            );
            (None, None, None, qw)
        }
    };
    PointDistributionRuleResponse {
        id: rule.id,
        bucket_id: rule.bucket_id,
        trigger_sources,
        grant_mode,
        points_amount,
        validity_days,
        grant_period_type,
        quota_windows,
        enabled: rule.enabled,
        display_order: rule.display_order,
    }
}

/// Convert a write-side rule DTO into a domain [`RuleUpsert`], validating the
/// trigger / policy shape. Returns a stable 400 on an illegal combination.
fn distribution_rule_error(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    rule_id: Option<Uuid>,
    field: Option<&str>,
) -> ApiError {
    ApiError::distribution_rule_error(
        status,
        DistributionRuleErrorResponse {
            code: code.to_string(),
            message: message.into(),
            rule_id,
            field: field.map(str::to_string),
        },
    )
}

fn invalid_rule_field(
    code: &'static str,
    message: impl Into<String>,
    rule_id: Option<Uuid>,
    field: &'static str,
) -> ApiError {
    distribution_rule_error(StatusCode::BAD_REQUEST, code, message, rule_id, Some(field))
}

fn rule_write_to_upsert(write: PointDistributionRuleWrite) -> Result<RuleUpsert, ApiError> {
    let rule_id = write.id;
    if write.trigger_sources.is_empty() {
        return Err(invalid_rule_field(
            "invalid_distribution_rule",
            "trigger_sources must be non-empty",
            rule_id,
            "triggerSources",
        ));
    }
    let mut trigger_sources = Vec::with_capacity(write.trigger_sources.len());
    for s in &write.trigger_sources {
        let t = s.parse::<DistributionTrigger>().map_err(|_| {
            invalid_rule_field(
                "invalid_distribution_trigger",
                format!("invalid distribution trigger: {s}"),
                rule_id,
                "triggerSources",
            )
        })?;
        trigger_sources.push(t);
    }
    let policy = match write.grant_mode.as_str() {
        "fixed" => {
            let amount = write.points_amount.ok_or_else(|| {
                invalid_rule_field(
                    "invalid_distribution_policy",
                    "points_amount is required for fixed grant_mode",
                    rule_id,
                    "pointsAmount",
                )
            })?;
            if amount <= 0 {
                return Err(invalid_rule_field(
                    "invalid_distribution_policy",
                    "points_amount must be > 0",
                    rule_id,
                    "pointsAmount",
                ));
            }
            let validity_days = write.validity_days.unwrap_or(0);
            if validity_days < 0 {
                return Err(invalid_rule_field(
                    "invalid_distribution_policy",
                    "validity_days must be >= 0",
                    rule_id,
                    "validityDays",
                ));
            }
            let grant_period_type = write
                .grant_period_type
                .as_deref()
                .map(|s| {
                    s.parse().map_err(|_| {
                        invalid_rule_field(
                            "invalid_distribution_policy",
                            format!("invalid grant_period_type: {s}"),
                            rule_id,
                            "grantPeriodType",
                        )
                    })
                })
                .transpose()?;
            DistributionPolicy::Fixed {
                amount,
                validity_days,
                grant_period_type,
            }
        }
        "quota" => {
            let windows_in = write.quota_windows.ok_or_else(|| {
                invalid_rule_field(
                    "invalid_distribution_policy",
                    "quota_windows is required for quota grant_mode",
                    rule_id,
                    "quotaWindows",
                )
            })?;
            const QUOTA_WINDOWS_MAX: usize = 8;
            if windows_in.len() > QUOTA_WINDOWS_MAX {
                return Err(invalid_rule_field(
                    "invalid_distribution_policy",
                    format!(
                        "quota_windows may have at most {} windows, got {}",
                        QUOTA_WINDOWS_MAX,
                        windows_in.len()
                    ),
                    rule_id,
                    "quotaWindows",
                ));
            }
            let mut windows = Vec::with_capacity(windows_in.len());
            for w in windows_in {
                if w.window_seconds <= 0 {
                    return Err(invalid_rule_field(
                        "invalid_distribution_policy",
                        "quota_windows.windowSeconds must be > 0",
                        rule_id,
                        "quotaWindows",
                    ));
                }
                if w.limit < 0 {
                    return Err(invalid_rule_field(
                        "invalid_distribution_policy",
                        "quota_windows.limit must be >= 0",
                        rule_id,
                        "quotaWindows",
                    ));
                }
                windows.push(QuotaWindow {
                    window_seconds: w.window_seconds,
                    limit: w.limit,
                    key: derive_window_key(w.window_seconds),
                });
            }
            DistributionPolicy::Quota { windows }
        }
        other => {
            return Err(invalid_rule_field(
                "invalid_distribution_policy",
                format!("invalid grant_mode: {other} (expected 'fixed' or 'quota')"),
                rule_id,
                "grantMode",
            ));
        }
    };
    Ok(RuleUpsert {
        id: write.id,
        bucket_id: write.bucket_id,
        trigger_sources,
        policy,
        enabled: write.enabled,
        display_order: write.display_order,
    })
}

/// Convert domain EntitlementMapping + its rules to API response
fn mapping_to_response(
    m: EntitlementMapping,
    rules: Vec<PointsDistributionRule>,
) -> EntitlementMappingResponse {
    EntitlementMappingResponse {
        id: m.id,
        payment_provider: m.payment_provider,
        external_product_id: m.external_product_id,
        external_price_id: m.external_price_id,
        entitlement_key: m.entitlement_key,
        billing_type: m.billing_type.map(|bt| bt.as_str().to_string()),
        billing_period: m.billing_period,
        service_duration_days: m.service_duration_days,
        enabled: m.enabled,
        provider_product_info: to_provider_product_info(m.provider_product_info),
        point_rules: rules.into_iter().map(rule_to_response).collect(),
        granted_role_ids: m.granted_role_ids,
        synced_at: m.synced_at.map(|dt| dt.to_rfc3339()),
        created_at: m.created_at.to_rfc3339(),
        updated_at: m.updated_at.to_rfc3339(),
    }
}

/// Leniently deserialize the stored `provider_product_info` JSONB into the
/// typed [`ProviderProductInfo`]. Returns `None` (and logs) if the stored
/// value does not match the shape — e.g. a legacy row whose metadata predates
/// the string→string sync coercion — so a malformed row degrades to "no
/// provider info" instead of failing the whole response.
fn to_provider_product_info(v: Option<serde_json::Value>) -> Option<ProviderProductInfo> {
    let v = v?;
    match serde_json::from_value::<ProviderProductInfo>(v) {
        Ok(info) => Some(info),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "provider_product_info JSONB did not match ProviderProductInfo; dropping to None"
            );
            None
        }
    }
}

/// List entitlement mappings for a realm
#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/entitlement-mappings",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Entitlement mappings listed successfully", body = EntitlementMappingListResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_entitlement_mappings(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Query(query): Query<EntitlementMappingQuery>,
) -> Result<Json<EntitlementMappingListResponse>, ApiError> {
    tracing::info!("Listing entitlement mappings for realm: {}", realm_id);

    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let (mappings, total) = state
        .billing_repository
        .list_entitlement_mappings(
            &realm_id,
            query.payment_provider.as_deref(),
            query.enabled,
            query.page,
            query.page_size,
        )
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                error = %e,
                "Failed to list entitlement mappings"
            );
            ApiError::internal("Failed to list entitlement mappings".to_string())
        })?;

    let mut items = Vec::with_capacity(mappings.len());
    for m in mappings {
        let rules = state
            .billing_repository
            .find_mapping_rules(&realm_id, m.id)
            .await
            .map_err(|e| {
                tracing::error!(
                    realm_id = %realm_id,
                    mapping_id = %m.id,
                    error = %e,
                    "Failed to load mapping rules"
                );
                ApiError::internal("Failed to list entitlement mappings".to_string())
            })?;
        items.push(mapping_to_response(m, rules));
    }

    Ok(Json(EntitlementMappingListResponse {
        items,
        total: total as i64,
    }))
}

/// Get a single entitlement mapping
#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/entitlement-mappings/{mappingId}",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("mappingId" = Uuid, Path, description = "Mapping ID")
    ),
    responses(
        (status = 200, description = "Entitlement mapping found", body = EntitlementMappingResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Mapping not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_entitlement_mapping(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, mapping_id)): Path<(String, Uuid)>,
) -> Result<Json<EntitlementMappingResponse>, ApiError> {
    tracing::info!(
        "Getting entitlement mapping {} for realm: {}",
        mapping_id,
        realm_id
    );

    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let mapping = state
        .billing_repository
        .find_entitlement_mapping_by_id(mapping_id)
        .await
        .map_err(|e| {
            tracing::error!(
                mapping_id = %mapping_id,
                error = %e,
                "Failed to get entitlement mapping"
            );
            ApiError::internal("Failed to get entitlement mapping".to_string())
        })?
        .ok_or_else(|| ApiError::not_found("Mapping not found"))?;

    if mapping.realm_id != realm_id {
        return Err(ApiError::not_found("Mapping not found"));
    }

    let rules = state
        .billing_repository
        .find_mapping_rules(&realm_id, mapping.id)
        .await
        .map_err(|e| {
            tracing::error!(
                mapping_id = %mapping.id,
                error = %e,
                "Failed to load mapping rules"
            );
            ApiError::internal("Failed to get entitlement mapping".to_string())
        })?;

    Ok(Json(mapping_to_response(mapping, rules)))
}

///
/// Generic over provider (IAP, Stripe, Creem). Required permission:
/// `billing.manage`; distribution-rule fields additionally require `points.manage`
/// (mirrors the batch update permission model). A duplicate
/// `(realm, provider, product, price)` row violates
/// `uq_pem_realm_provider_product_price` and surfaces as HTTP 409.
#[utoipa::path(
    post,
    path = "/api/bill/{realmId}/entitlement-mappings",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = CreateEntitlementMappingRequest,
    responses(
        (status = 201, description = "Entitlement mapping created", body = EntitlementMappingResponse),
        (status = 400, description = "Bad request, including invalid distribution rules", body = DistributionRuleErrorResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - missing billing.manage (or points.manage for credit fields)", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 409, description = "Mapping or distribution-rule conflict", body = DistributionRuleErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_entitlement_mapping(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Json(request): Json<CreateEntitlementMappingRequest>,
) -> Result<(StatusCode, Json<EntitlementMappingResponse>), ApiError> {
    tracing::info!(
        provider = %request.payment_provider,
        external_product_id = %request.external_product_id,
        "Create entitlement mapping for realm {}", realm_id
    );

    // 1. billing.manage (realm boundary + business permission).
    require_billing_permission(&state, &identity, &realm_id, "manage").await?;

    // 2. points.manage if the request carries distribution rules (the credit
    // config dimension). An empty / absent rule set is a valid "no points
    // grant" mapping and does not require points.manage.
    if !request.point_rules.is_empty() {
        require_points_manage_permission(&state, &identity, &realm_id).await?;
    }

    request
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    if !request.granted_role_ids.is_empty() {
        validate_granted_role_ids(
            &realm_id,
            &request.granted_role_ids,
            state.role_policy_repository.as_ref(),
        )
        .await
        .map_err(map_batch_error)?;
    }

    // 3. billing_type parse + billing_period guard.
    let billing_type = request
        .billing_type
        .parse::<herald_core::domain::billing::entities::BillingType>()
        .map_err(|e| ApiError::bad_request(format!("invalid billing_type: {e}")))?;

    // 4. Materialize the rule upsert set (validates trigger/policy shape).
    let point_rules = request
        .point_rules
        .into_iter()
        .map(rule_write_to_upsert)
        .collect::<Result<Vec<_>, _>>()?;
    for rule in &point_rules {
        let resolved = rule.clone().into_rule_for_owner(
            &realm_id,
            DistributionRuleOwner::EntitlementMapping(Uuid::nil()),
        );
        validate_rule_for_owner(&resolved, Some(billing_type.clone()))
            .map_err(|error| distribution_rule_validation_error(error, rule.id))?;
    }
    let only_rule_id = (point_rules.len() == 1)
        .then(|| point_rules[0].id)
        .flatten();

    let mapping = state
        .entitlement_mapping_service
        .create_mapping(
            identity,
            &realm_id,
            CreateEntitlementMappingInput {
                payment_provider: request.payment_provider,
                external_product_id: request.external_product_id,
                external_price_id: request.external_price_id,
                entitlement_key: request.entitlement_key,
                billing_type,
                billing_period: request.billing_period,
                service_duration_days: request.service_duration_days,
                point_rules,
                granted_role_ids: request.granted_role_ids,
                price: request.price,
                currency: request.currency,
                enabled: request.enabled,
            },
        )
        .await
        .map_err(|e| match e {
            CoreError::Conflict(message) if message.starts_with("distribution_rule_conflict:") => {
                distribution_rule_error(
                    StatusCode::CONFLICT,
                    "distribution_rule_conflict",
                    message,
                    only_rule_id,
                    None,
                )
            }
            CoreError::Conflict(_) => {
                ApiError::conflict("mapping already exists for this provider+product+price")
            }
            other => {
                herald_api_base::application::http::common::error_helpers::core_error_to_api_error(
                    other,
                    "create entitlement mapping",
                )
            }
        })?;

    let rules = state
        .billing_repository
        .find_mapping_rules(&realm_id, mapping.id)
        .await
        .map_err(|e| {
            tracing::error!(
                mapping_id = %mapping.id,
                error = %e,
                "Failed to load created mapping rules"
            );
            ApiError::internal("Failed to load created mapping rules".to_string())
        })?;

    Ok((
        StatusCode::CREATED,
        Json(mapping_to_response(mapping, rules)),
    ))
}

/// Update an entitlement mapping
#[utoipa::path(
    patch,
    path = "/api/bill/{realmId}/entitlement-mappings/{mappingId}",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("mappingId" = Uuid, Path, description = "Mapping ID")
    ),
    request_body = UpdateEntitlementMappingRequest,
    responses(
        (status = 200, description = "Entitlement mapping updated successfully", body = EntitlementMappingResponse),
        (status = 400, description = "Bad request, including invalid distribution rules", body = DistributionRuleErrorResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Mapping not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 409, description = "Distribution rule conflicts with its owner or target bucket", body = DistributionRuleErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_entitlement_mapping(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, mapping_id)): Path<(String, Uuid)>,
    Json(request): Json<UpdateEntitlementMappingRequest>,
) -> Result<Json<EntitlementMappingResponse>, ApiError> {
    tracing::info!(
        "Updating entitlement mapping {} for realm: {}",
        mapping_id,
        realm_id
    );

    require_billing_permission(&state, &identity, &realm_id, "manage").await?;

    // points.manage when the PATCH carries distribution rules.
    if request.point_rules.is_some() {
        require_points_manage_permission(&state, &identity, &realm_id).await?;
    }

    if let Some(ref key) = request.entitlement_key {
        if key.is_empty() || key.len() > 64 {
            return Err(ApiError::bad_request(
                "Invalid entitlement key (must be 1-64 characters)".to_string(),
            ));
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(ApiError::bad_request(
                "Invalid entitlement key (must match [a-z0-9-])".to_string(),
            ));
        }
    }

    let existing = state
        .billing_repository
        .find_entitlement_mapping_by_id(mapping_id)
        .await
        .map_err(|e| {
            tracing::error!(
                mapping_id = %mapping_id,
                error = %e,
                "Failed to find entitlement mapping for update"
            );
            ApiError::internal("Failed to find entitlement mapping".to_string())
        })?
        .ok_or_else(|| ApiError::not_found("Mapping not found"))?;

    if existing.realm_id != realm_id {
        return Err(ApiError::not_found("Mapping not found"));
    }

    // billing_type is immutable on PATCH, so the NonRenewing invariant must
    // hold for the *resolved* duration of an existing non_renewing mapping:
    // Some(>=1). For other billing types the field is forced to None
    // (meaningless outside non_renewing). 3-state: None = leave unchanged,
    // Some(None) = clear, Some(Some(n)) = set.
    let is_non_renewing = matches!(
        existing.billing_type,
        Some(herald_core::domain::billing::entities::BillingType::NonRenewing)
    );
    let service_duration_days = request
        .service_duration_days
        .unwrap_or(existing.service_duration_days);
    let service_duration_days = if is_non_renewing {
        match service_duration_days {
            Some(days) if days >= 1 => Some(days),
            _ => {
                return Err(ApiError::bad_request(
                    "service_duration_days is required and must be >= 1 for non_renewing billing_type"
                        .to_string(),
                ));
            }
        }
    } else {
        None
    };

    // Materialize the optional rule upsert set (validates trigger/policy shape).
    let point_rules = match request.point_rules {
        Some(writes) => Some(
            writes
                .into_iter()
                .map(rule_write_to_upsert)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        None => None,
    };
    if let Some(rules) = &point_rules {
        for rule in rules {
            let resolved = rule.clone().into_rule_for_owner(
                &realm_id,
                DistributionRuleOwner::EntitlementMapping(mapping_id),
            );
            validate_rule_for_owner(&resolved, existing.billing_type.clone())
                .map_err(|error| distribution_rule_validation_error(error, rule.id))?;
        }
    }

    // Manual price (WeChat only): PATCH carries price/currency independently
    // (3-state — absent means leave unchanged), so merge into the stored
    // `provider_product_info` instead of replacing it. Every other provider
    // rejects the fields: its price truth is the provider catalog.
    let provider_product_info = if request.price.is_some() || request.currency.is_some() {
        if existing.payment_provider != "wechat" {
            return Err(ApiError::bad_request(
                "price/currency can only be configured for WeChat mappings".to_string(),
            ));
        }
        if let Some(price) = request.price
            && price < 1
        {
            return Err(ApiError::bad_request(
                "price (minor units) must be >= 1".to_string(),
            ));
        }
        if let Some(ref currency) = request.currency {
            herald_core::domain::billing::validate_currency_code(currency)
                .map_err(|e| ApiError::bad_request(format!("{e}")))?;
        }
        let mut info = existing
            .provider_product_info
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        let obj = info.as_object_mut().ok_or_else(|| {
            ApiError::bad_request("stored provider_product_info is malformed".to_string())
        })?;
        if let Some(price) = request.price {
            obj.insert("price".to_string(), serde_json::json!(price));
        }
        if let Some(currency) = request.currency {
            obj.insert("currency".to_string(), serde_json::json!(currency));
        }
        Some(info)
    } else {
        existing.provider_product_info.clone()
    };

    let updated_mapping = EntitlementMapping {
        id: existing.id,
        realm_id: existing.realm_id.clone(),
        payment_provider: existing.payment_provider.clone(),
        external_product_id: existing.external_product_id.clone(),
        external_price_id: existing.external_price_id.clone(),
        entitlement_key: request.entitlement_key.unwrap_or(existing.entitlement_key),
        billing_type: existing.billing_type,
        billing_period: existing.billing_period,
        service_duration_days,
        enabled: request.enabled.unwrap_or(existing.enabled),
        provider_product_info,
        granted_role_ids: existing.granted_role_ids,
        synced_at: existing.synced_at,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now(),
    };

    let updated = if let Some(rules) = point_rules {
        // Atomic upsert of mapping base fields + rule set in one transaction.
        state
            .billing_repository
            .upsert_mapping_with_rules(&realm_id, updated_mapping, rules)
            .await
            .map_err(|e| {
                tracing::error!(
                    mapping_id = %mapping_id,
                    error = %e,
                    "Failed to update entitlement mapping with rules"
                );
                match e {
                    CoreError::Conflict(message) => distribution_rule_error(
                        StatusCode::CONFLICT,
                        "distribution_rule_conflict",
                        message,
                        None,
                        None,
                    ),
                    CoreError::NotFound => {
                        ApiError::not_found("Distribution rule or target bucket not found")
                    }
                    other => herald_api_base::application::http::common::error_helpers::core_error_to_api_error(
                        other,
                        "update entitlement mapping with rules",
                    ),
                }
            })?
    } else {
        state
            .billing_repository
            .update_entitlement_mapping(updated_mapping)
            .await
            .map_err(|e| {
                tracing::error!(
                    mapping_id = %mapping_id,
                    error = %e,
                    "Failed to update entitlement mapping"
                );
                herald_api_base::application::http::common::error_helpers::core_error_to_api_error(
                    e,
                    "update entitlement mapping",
                )
            })?
    };

    let rules = state
        .billing_repository
        .find_mapping_rules(&realm_id, updated.id)
        .await
        .map_err(|e| {
            tracing::error!(
                mapping_id = %updated.id,
                error = %e,
                "Failed to load updated mapping rules"
            );
            ApiError::internal("Failed to load updated mapping rules".to_string())
        })?;

    Ok(Json(mapping_to_response(updated, rules)))
}

/// List enabled one-time entitlement mappings for a realm
#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/one-time-mappings",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "One-time mappings listed successfully", body = OneTimeMappingListResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_one_time_mappings(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Result<Json<OneTimeMappingListResponse>, ApiError> {
    tracing::info!("Listing one-time mappings for realm: {}", realm_id);

    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let mappings = state
        .billing_repository
        .list_one_time_mappings(&realm_id)
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                error = %e,
                "Failed to list one-time mappings"
            );
            ApiError::internal("Failed to list one-time mappings".to_string())
        })?;

    let mut items: Vec<OneTimeMappingItem> = Vec::with_capacity(mappings.len());
    for m in mappings {
        let rules = state
            .billing_repository
            .find_mapping_rules(&realm_id, m.id)
            .await
            .map_err(|e| {
                tracing::error!(
                    realm_id = %realm_id,
                    mapping_id = %m.id,
                    error = %e,
                    "Failed to load one-time mapping rules"
                );
                ApiError::internal("Failed to list one-time mappings".to_string())
            })?;
        items.push(OneTimeMappingItem {
            id: m.id.to_string(),
            entitlement_key: m.entitlement_key,
            provider_product_info: to_provider_product_info(m.provider_product_info),
            point_rules: rules.into_iter().map(rule_to_response).collect(),
            payment_provider: m.payment_provider,
        });
    }

    Ok(Json(OneTimeMappingListResponse { items }))
}

/// Sync provider products into entitlement mappings
#[utoipa::path(
    post,
    path = "/api/bill/{realmId}/entitlement-mappings/sync",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = SyncProviderRequest,
    responses(
        (status = 200, description = "Provider products synced successfully", body = SyncProviderResponse),
        (status = 400, description = "Bad request - Provider not configured", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn sync_provider_products(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Json(request): Json<SyncProviderRequest>,
) -> Result<Json<SyncProviderResponse>, ApiError> {
    tracing::info!(
        "Syncing provider products for provider '{}' in realm: {}",
        request.payment_provider,
        realm_id
    );

    require_points_manage_permission(&state, &identity, &realm_id).await?;

    let result = state
        .provider_product_sync_service
        .sync_provider_products(identity, &realm_id, &request.payment_provider)
        .await
        .map_err(ApiError::from)?;

    let sync_status = match result.sync_status {
        SyncStatus::Completed => "completed",
        SyncStatus::Partial => "partial",
        SyncStatus::Failed => "failed",
    }
    .to_string();

    Ok(Json(SyncProviderResponse {
        products_synced: result.products_synced as i64,
        prices_synced: result.prices_synced as i64,
        sync_status,
        error: result.error,
        partial_errors: result
            .partial_errors
            .into_iter()
            .map(|error| PartialSyncError {
                external_id: error.external_id,
                reason: error.reason,
            })
            .collect(),
    }))
}

/// Batch-save all price mappings for a product.
///
/// Validation/permission order:
/// 1. `billing.manage` (realm boundary + business permission).
/// 2. If any update row carries a credit-strategy field → `points.manage`.
/// 3. Credit-strategy field validation.
///
/// Then the repository performs a single-transaction upsert of all the
/// product's price rows: any row transitioning enabled true→false while protected by an active
/// subscription rolls back the WHOLE transaction (409 with
/// `{ activeSubscriptions }`). Cross-realm/product `mapping_id` tampering surfaces as 400.
#[utoipa::path(
    put,
    path = "/api/bill/{realmId}/entitlement-mappings/batch",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = BatchUpdateEntitlementMappingsRequest,
    responses(
        (status = 201, description = "Batch saved successfully", body = BatchUpdateEntitlementMappingsResponse),
        (status = 400, description = "Bad request - invalid credit strategy or mapping_id not in this product/realm", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - missing billing.manage (or points.manage for credit fields)", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 409, description = "Conflict - active subscription protects a disabled mapping (whole batch rolled back)", body = MappingActiveSubscriptionLockErrorBody),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn batch_update_entitlement_mappings(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Json(request): Json<BatchUpdateEntitlementMappingsRequest>,
) -> Result<(StatusCode, Json<BatchUpdateEntitlementMappingsResponse>), ApiError> {
    tracing::info!(
        provider = %request.payment_provider,
        product = %request.external_product_id,
        update_count = request.updates.len(),
        "Batch update entitlement mappings for realm {}",
        realm_id
    );

    // 1. billing.manage (realm boundary + business permission).
    require_billing_permission(&state, &identity, &realm_id, "manage").await?;

    // 2. points.manage if any row writes a distribution rule set.
    let touches_credit_fields = request.updates.iter().any(|u| u.point_rules.is_some());
    if touches_credit_fields {
        require_points_manage_permission(&state, &identity, &realm_id).await?;
    }

    let input = herald_core::domain::billing::BatchUpdateMappingsInput {
        realm_id: realm_id.clone(),
        payment_provider: request.payment_provider.clone(),
        external_product_id: request.external_product_id.clone(),
        updates: request
            .updates
            .into_iter()
            .map(|u| {
                let point_rules = match u.point_rules {
                    Some(writes) => Some(
                        writes
                            .into_iter()
                            .map(rule_write_to_upsert)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                    None => None,
                };
                Ok(herald_core::domain::billing::PriceMappingUpdateInput {
                    mapping_id: u.mapping_id,
                    billing_type: u.billing_type,
                    enabled: u.enabled,
                    point_rules,
                    granted_role_ids: u.granted_role_ids,
                })
            })
            .collect::<Result<Vec<_>, ApiError>>()?,
    };

    // any row carrying a non-empty `granted_role_ids` must reference roles that
    // all belong to this realm. Collect the union of provided role IDs across
    // rows (non-empty only) and validate once. Done in the handler (which has
    // `AppState`) so the infra layer stays free of a new role-read dependency.
    // `None`/empty rows are excluded — `None` ⟺ leave unchanged, `Some([])` ⟺
    // clear (no role to validate).
    let all_role_ids: Vec<Uuid> = input
        .updates
        .iter()
        .filter_map(|u| u.granted_role_ids.as_ref())
        .flatten()
        .copied()
        .collect();
    if !all_role_ids.is_empty() {
        validate_granted_role_ids(
            &realm_id,
            &all_role_ids,
            state.role_policy_repository.as_ref(),
        )
        .await
        .map_err(map_batch_error)?;
    }

    let result = state
        .billing_repository
        .batch_update_mappings(input)
        .await
        .map_err(map_batch_error)?;

    let mut prices = Vec::with_capacity(result.prices.len());
    for m in result.prices {
        let rules = state
            .billing_repository
            .find_mapping_rules(&realm_id, m.id)
            .await
            .map_err(|e| {
                tracing::error!(
                    realm_id = %realm_id,
                    mapping_id = %m.id,
                    error = %e,
                    "Failed to load batch-updated mapping rules"
                );
                ApiError::internal("Failed to load batch-updated mapping rules".to_string())
            })?;
        prices.push(mapping_to_response(m, rules));
    }
    Ok((
        StatusCode::CREATED,
        Json(BatchUpdateEntitlementMappingsResponse {
            saved: result.saved,
            prices,
        }),
    ))
}

/// Translate [`BatchMappingError`] into the HTTP error contract.
///
/// - `MappingNotInGroup` → 400 (field-level).
/// - `ActiveSubscriptionLock` → 409 with `{ code, activeSubscriptions }`.
/// - `Other(CoreError)` → preserves the wrapped status (404 / 500 / …).
fn map_batch_error(err: BatchMappingError) -> ApiError {
    match err {
        BatchMappingError::MappingNotInGroup {
            mapping_id,
            provider,
            product,
        } => ApiError::bad_request(format!(
            "mapping {mapping_id} does not belong to provider '{provider}' product '{product}' in this realm"
        )),
        BatchMappingError::ActiveSubscriptionLock {
            active_subscriptions,
            ..
        } => ApiError::conflict_json(MappingActiveSubscriptionLockErrorBody {
            code: "mapping_in_use",
            active_subscriptions,
        }),
        BatchMappingError::RoleNotInRealm { role_id, realm_id } => {
            ApiError::bad_request_json(MappingRoleNotInRealmErrorBody {
                code: "role_not_in_realm",
                role_id,
                realm_id,
            })
        }
        BatchMappingError::Other(CoreError::Conflict(message))
            if message.starts_with("distribution_rule_conflict:") =>
        {
            distribution_rule_error(
                StatusCode::CONFLICT,
                "distribution_rule_conflict",
                message,
                None,
                None,
            )
        }
        BatchMappingError::Other(core) => ApiError::from(core),
    }
}
