//! Realm registration distribution rules handlers.
//!
//! Registration / free-periodic points routing is expressed as a rule
//! set owned by the Realm's registration config (`owner_type =
//! realm_registration`). Rules are read/written as an array with atomic upsert
//! semantics (DEC-005): rules in the set with `id = None` are created,
//! `id = Some(existing)` are updated, and rules absent from the set are left
//! untouched (disabling requires explicit `enabled = false`).

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use uuid::Uuid;
use validator::Validate;

use crate::types::{
    QuotaWindowInput, RegistrationRuleResponse, RegistrationRuleWrite, RegistrationRulesResponse,
    UpsertRegistrationRulesRequest,
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{
    ApiError, DistributionRuleErrorResponse, ErrorResponse, distribution_rule_validation_error,
};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::billing::BillingRepository;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::points::{
    DistributionPolicy, DistributionRuleOwner, DistributionTrigger, RuleUpsert,
    validate_rule_for_owner,
};

const QUOTA_WINDOWS_MAX: usize = 8;

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

/// Materialize a write-side rule into a domain [`RuleUpsert`], validating the
/// trigger / policy shape and the registration-owner constraints. Returns a
/// stable 400 on an illegal combination.
fn rule_write_to_upsert(write: RegistrationRuleWrite) -> Result<RuleUpsert, ApiError> {
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
                windows.push(herald_core::domain::points::QuotaWindow {
                    window_seconds: w.window_seconds,
                    limit: w.limit,
                    key: herald_core::domain::points::derive_window_key(w.window_seconds),
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
    let rule = RuleUpsert {
        id: write.id,
        bucket_id: write.bucket_id,
        trigger_sources: trigger_sources.clone(),
        policy: policy.clone(),
        enabled: write.enabled,
        display_order: write.display_order,
    }
    .into_rule_for_owner("", DistributionRuleOwner::RealmRegistration);
    // Re-run the domain validator (owner/trigger/policy invariants) before
    // persistence. `validate_rule_for_owner` is the single owner/trigger/policy
    // authority; bucket realm/disabled checks are enforced at the repository.
    validate_rule_for_owner(&rule, None)
        .map_err(|error| distribution_rule_validation_error(error, rule_id))?;
    Ok(RuleUpsert {
        id: write.id,
        bucket_id: write.bucket_id,
        trigger_sources,
        policy,
        enabled: write.enabled,
        display_order: write.display_order,
    })
}

/// Convert a domain rule into the read-side registration-rule DTO.
fn rule_to_response(
    rule: herald_core::domain::points::PointsDistributionRule,
) -> RegistrationRuleResponse {
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
                    .map(|w| QuotaWindowInput {
                        window_seconds: w.window_seconds,
                        limit: w.limit,
                    })
                    .collect(),
            );
            (None, None, None, qw)
        }
    };
    RegistrationRuleResponse {
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

/// Get the Realm's registration distribution rules.
#[utoipa::path(
    get,
    path = "/api/points/{realmId}/registration-rules",
    params(("realmId" = String, Path, description = "Realm ID")),
    responses(
        (status = 200, description = "Registration rules retrieved", body = RegistrationRulesResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "points"
)]
pub async fn get_registration_rules(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Result<Json<RegistrationRulesResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "registration rules")?;
    admin.require_permission(&state, "points", "view").await?;

    let rules = state
        .billing_repository
        .find_registration_rules(&realm_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(RegistrationRulesResponse {
        realm_id,
        rules: rules.into_iter().map(rule_to_response).collect(),
    }))
}

/// Atomically upsert the Realm's registration distribution rule set.
#[utoipa::path(
    put,
    path = "/api/points/{realmId}/registration-rules",
    params(("realmId" = String, Path, description = "Realm ID")),
    request_body = UpsertRegistrationRulesRequest,
    responses(
        (status = 200, description = "Registration rules upserted", body = RegistrationRulesResponse),
        (status = 400, description = "Bad request - invalid distribution rule", body = DistributionRuleErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 409, description = "Distribution rule conflicts with its owner or target bucket", body = DistributionRuleErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "points"
)]
pub async fn upsert_registration_rules(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Json(request): Json<UpsertRegistrationRulesRequest>,
) -> Result<Json<RegistrationRulesResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "registration rules")?;
    admin.require_permission(&state, "points", "manage").await?;

    request
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let rules = request
        .rules
        .into_iter()
        .map(rule_write_to_upsert)
        .collect::<Result<Vec<_>, _>>()?;
    let only_rule_id = (rules.len() == 1).then(|| rules[0].id).flatten();

    let rules = state
        .billing_repository
        .upsert_registration_rules(&realm_id, rules)
        .await
        .map_err(|error| match error {
            CoreError::Conflict(message) => distribution_rule_error(
                StatusCode::CONFLICT,
                "distribution_rule_conflict",
                message,
                only_rule_id,
                None,
            ),
            other => ApiError::from(other),
        })?;
    Ok(Json(RegistrationRulesResponse {
        realm_id,
        rules: rules.into_iter().map(rule_to_response).collect(),
    }))
}
