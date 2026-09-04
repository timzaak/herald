//! Shared mapping from domain distribution-rule validation errors to the API
//! error shape (error code, form field, HTTP status). Every rule-editing
//! surface (api-billing entitlement mappings, api-points registration rules)
//! must return the same code/field/status for the same domain error, so a new
//! `DistributionRuleError` variant only needs wiring here.

use axum::http::StatusCode;
use uuid::Uuid;

use super::{ApiError, DistributionRuleErrorResponse};
use herald_core::domain::points::DistributionRuleError;

/// Map a domain [`DistributionRuleError`] to the API error returned by
/// rule-validation failures, tagging it with the offending rule when known.
pub fn distribution_rule_validation_error(
    error: DistributionRuleError,
    rule_id: Option<Uuid>,
) -> ApiError {
    let (code, field, status) = match &error {
        DistributionRuleError::TriggerNotAllowedForOwner(_) => (
            "invalid_distribution_trigger",
            Some("triggerSources"),
            StatusCode::BAD_REQUEST,
        ),
        DistributionRuleError::PolicyNotAllowedForTrigger => (
            "invalid_distribution_policy",
            Some("grantMode"),
            StatusCode::BAD_REQUEST,
        ),
        DistributionRuleError::InvalidFixedAmount => (
            "invalid_distribution_policy",
            Some("pointsAmount"),
            StatusCode::BAD_REQUEST,
        ),
        DistributionRuleError::InvalidValidity
        | DistributionRuleError::RegistrationMustBePermanent => (
            "invalid_distribution_policy",
            Some("validityDays"),
            StatusCode::BAD_REQUEST,
        ),
        DistributionRuleError::InvalidQuotaWindows => (
            "invalid_distribution_policy",
            Some("quotaWindows"),
            StatusCode::BAD_REQUEST,
        ),
        DistributionRuleError::EmptyTriggerSources => (
            "invalid_distribution_rule",
            Some("triggerSources"),
            StatusCode::BAD_REQUEST,
        ),
        DistributionRuleError::BucketOutsideRealm | DistributionRuleError::BucketDisabled => (
            "distribution_rule_conflict",
            Some("bucketId"),
            StatusCode::CONFLICT,
        ),
        DistributionRuleError::RuleOutsideOwner => (
            "distribution_rule_conflict",
            Some("ruleId"),
            StatusCode::CONFLICT,
        ),
    };
    ApiError::distribution_rule_error(
        status,
        DistributionRuleErrorResponse {
            code: code.to_string(),
            message: error.to_string(),
            rule_id,
            field: field.map(str::to_string),
        },
    )
}
