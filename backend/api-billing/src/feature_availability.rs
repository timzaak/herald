use axum::{
    Json,
    extract::{Extension, Path, State},
};
use herald_api_base::application::http::common::auth_utils::{
    require_authenticated_user_in_realm, require_token_scope,
};
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialScope, Identity, TokenCredentialContext};
use herald_core::domain::authorization::PermissionService;
use herald_core::domain::billing::BillingRepository;
use serde::Serialize;
use uuid::Uuid;

use crate::invoice_eligibility::{InvoiceEligibilitySummary, evaluate_realm_invoice_eligibility};
use herald_core::domain::realm_config::ConfigType;

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeatureAvailabilityResponse {
    pub admin: AdminFeatureAvailability,
    pub user: UserFeatureAvailability,
    pub facts: FeatureAvailabilityFacts,
    /// Realm-level invoice eligibility. Consumed by regular users to gate
    /// Create/Apply invoice buttons before submit; reuses the seller-config
    /// fact already loaded above (no second seller-config query).
    pub invoice_eligibility: InvoiceEligibilitySummary,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserFeatureAvailabilityResponse {
    pub user: UserFeatureAvailability,
    pub invoice_eligibility: InvoiceEligibilitySummary,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminFeatureAvailability {
    pub billing_visible: bool,
    pub billing_config_visible: bool,
    pub entitlement_mappings_visible: bool,
    pub invoices_visible: bool,
    pub subscription_history_visible: bool,
    pub points_visible: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserFeatureAvailability {
    pub points_visible: bool,
    pub subscription_visible: bool,
    pub invoices_visible: bool,
    /// Whether Passkey authentication is enabled for this realm. Consumed by
    /// the user security page (to gate the passkey tab) and the login page
    /// (to gate the passkey entry) before any passkey endpoint is called.
    pub passkey_enabled: bool,
    /// Whether TOTP is enabled for this realm. Consumed by the user security
    /// page to gate the TOTP tab before any TOTP endpoint is called.
    pub totp_enabled: bool,
    /// Whether the realm forces TOTP (`force_enabled`). Exposed so the user
    /// security page can render the "set up TOTP" guidance banner the PRD's
    /// force mode requires (totp.md §5.2) — force mode never blocks login,
    /// it only guides.
    pub totp_force_enabled: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeatureAvailabilityFacts {
    pub has_payment_providers: bool,
    pub has_entitlement_mappings: bool,
    pub has_enabled_mappings: bool,
    pub has_one_time_mappings: bool,
    pub has_recurring_mappings: bool,
    pub has_invoice_seller_config: bool,
    pub has_invoices: bool,
    pub has_subscription_history: bool,
}

#[derive(Debug, Clone)]
struct FeatureFacts {
    has_payment_providers: bool,
    has_entitlement_mappings: bool,
    has_enabled_mappings: bool,
    has_one_time_mappings: bool,
    has_recurring_mappings: bool,
    has_invoice_seller_config: bool,
    has_invoices: bool,
    has_subscription_history: bool,
}

#[utoipa::path(
    get,
    path = "/api/realms/{realmId}/feature-availability",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Feature availability summary", body = FeatureAvailabilityResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden")
    ),
    tag = "realms",
    operation_id = "get_feature_availability"
)]
pub async fn get_feature_availability(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
) -> Result<Json<FeatureAvailabilityResponse>, ApiError> {
    let user_id =
        require_authenticated_user_in_realm(&identity, &realm_id, "feature availability")?;
    let (can_view_billing, can_view_points) = tokio::try_join!(
        has_permission(&state, &realm_id, &user_id, "billing", "view"),
        has_permission(&state, &realm_id, &user_id, "points", "view"),
    )?;
    if !can_view_billing && !can_view_points {
        return Err(ApiError::forbidden("Management permission required"));
    }
    let facts = load_feature_facts(&state, &realm_id).await?;

    let admin_billing_visible = can_view_billing;
    let admin_points_visible = can_view_points;
    let user_subscription_visible = facts.has_enabled_mappings;
    // The points area (balance page, purchase page, purchase history, inline
    // purchase CTA) is visible whenever the realm has any enabled entitlement
    // mapping. US-CB-005 scenario 2 requires the points page to surface
    // registration/free/system-granted credit, which is independent of any
    // one_time mapping; a subscription-only realm still grants points to its
    // users, so the gate follows `has_enabled_mappings` (same as
    // `subscription_visible`) rather than `has_one_time_mappings`.
    let user_points_visible = points_area_visible(&facts);
    let user_invoices_visible = facts.has_invoice_seller_config;
    let passkey_enabled = read_realm_passkey_enabled(&state, &realm_id).await;
    let (totp_enabled, totp_force_enabled) = read_realm_totp_flags(&state, &realm_id).await;

    // Realm-level invoice eligibility: reuse the already-loaded seller-config
    // fact so we do not issue a second seller-config query here.
    let invoice_eligibility =
        evaluate_realm_invoice_eligibility(&state, &realm_id, facts.has_invoice_seller_config)
            .await?;

    Ok(Json(FeatureAvailabilityResponse {
        admin: AdminFeatureAvailability {
            billing_visible: admin_billing_visible,
            billing_config_visible: admin_billing_visible,
            entitlement_mappings_visible: admin_billing_visible,
            invoices_visible: admin_billing_visible,
            subscription_history_visible: admin_billing_visible,
            points_visible: admin_points_visible,
        },
        user: UserFeatureAvailability {
            points_visible: user_points_visible,
            subscription_visible: user_subscription_visible,
            invoices_visible: user_invoices_visible,
            passkey_enabled,
            totp_enabled,
            totp_force_enabled,
        },
        facts: FeatureAvailabilityFacts {
            has_payment_providers: facts.has_payment_providers,
            has_entitlement_mappings: facts.has_entitlement_mappings,
            has_enabled_mappings: facts.has_enabled_mappings,
            has_one_time_mappings: facts.has_one_time_mappings,
            has_recurring_mappings: facts.has_recurring_mappings,
            has_invoice_seller_config: facts.has_invoice_seller_config,
            has_invoices: facts.has_invoices,
            has_subscription_history: facts.has_subscription_history,
        },
        invoice_eligibility,
    }))
}

#[utoipa::path(
    get,
    path = "/api/user/feature-availability",
    responses(
        (status = 200, description = "Current user's feature availability", body = UserFeatureAvailabilityResponse),
        (status = 401, description = "Unauthorized")
    ),
    tag = "user",
    security(("bearer_auth" = []))
)]
pub async fn get_user_feature_availability(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
) -> Result<Json<UserFeatureAvailabilityResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::FeatureRead)?;
    let realm_id = identity.realm_id();
    let facts = load_feature_facts(&state, &realm_id).await?;
    let passkey_enabled = read_realm_passkey_enabled(&state, &realm_id).await;
    let (totp_enabled, totp_force_enabled) = read_realm_totp_flags(&state, &realm_id).await;
    let invoice_eligibility =
        evaluate_realm_invoice_eligibility(&state, &realm_id, facts.has_invoice_seller_config)
            .await?;
    Ok(Json(UserFeatureAvailabilityResponse {
        user: UserFeatureAvailability {
            points_visible: points_area_visible(&facts),
            subscription_visible: facts.has_enabled_mappings,
            invoices_visible: facts.has_invoice_seller_config,
            passkey_enabled,
            totp_enabled,
            totp_force_enabled,
        },
        invoice_eligibility,
    }))
}

async fn has_permission(
    state: &AppState,
    realm_id: &str,
    user_id: &Uuid,
    resource: &str,
    action: &str,
) -> Result<bool, ApiError> {
    state
        .permission_checker
        .check_permission(realm_id, &user_id.to_string(), resource, action)
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                user_id = %user_id,
                resource = %resource,
                action = %action,
                error = %e,
                "Failed to check feature availability permission"
            );
            ApiError::internal("Failed to check permission")
        })
}

async fn load_feature_facts(state: &AppState, realm_id: &str) -> Result<FeatureFacts, ApiError> {
    let facts = state
        .billing_repository
        .check_feature_facts(realm_id, &state.pool)
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                error = %e,
                "Failed to load feature availability facts"
            );
            ApiError::internal("Failed to load feature availability")
        })?;

    Ok(FeatureFacts {
        has_payment_providers: facts.has_payment_providers,
        has_entitlement_mappings: facts.has_entitlement_mappings,
        has_enabled_mappings: facts.has_enabled_mappings,
        has_one_time_mappings: facts.has_one_time_mappings,
        has_recurring_mappings: facts.has_recurring_mappings,
        has_invoice_seller_config: facts.has_invoice_seller_config,
        has_invoices: facts.has_invoices,
        has_subscription_history: facts.has_subscription_history,
    })
}

/// Pure decision: is the user-facing points area visible for these facts?
///
/// Kept free of I/O so the gating rule can be unit-tested without a database.
/// The points area (balance page, purchase page, purchase history, inline
/// purchase CTA) is visible whenever the realm has any enabled entitlement
/// mapping — a subscription-only realm still grants points to its users
/// (US-CB-005 scenario 2: system-granted / registration credit must be
/// surfaced regardless of whether a one_time pack exists).
fn points_area_visible(facts: &FeatureFacts) -> bool {
    facts.has_enabled_mappings
}

/// Read the realm's passkey `enabled` flag from `realm_config`
/// (`config_type='passkey'`, `config_key='settings'`).
///
/// Mirrors the gating SQL in `ensure_passkey_enabled` (api-auth). Returns
/// `false` when the config row is absent or the inner `enabled` flag is
/// missing/false — i.e. a realm that never configured passkey is treated as
/// disabled. This is the single user-visible signal the frontend uses to gate
/// the passkey tab/login entry without relying on a 404 from a passkey
/// endpoint.
async fn read_realm_passkey_enabled(state: &AppState, realm_id: &str) -> bool {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = $2 AND config_key = 'settings' AND enabled = true",
    )
    .bind(realm_id)
    .bind(ConfigType::Passkey.as_ref())
    .fetch_optional(&state.pool)
    .await;
    match row {
        Ok(Some((value,))) => serde_json::from_str::<serde_json::Value>(&value)
            .ok()
            .and_then(|v| v.get("enabled").and_then(|e| e.as_bool()))
            .unwrap_or(false),
        // Absent config or query failure: treat as disabled. We log query
        // errors but never propagate them — feature-availability must not
        // hard-fail the whole response over a passkey flag lookup.
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(realm_id = %realm_id, error = %e, "Failed to read passkey realm config");
            false
        }
    }
}

/// Read the realm's TOTP `enabled` and `force_enabled` flags from the single
/// `realm_config` row (`config_type='totp'`, `config_key='settings'`) in one
/// query.
///
/// `enabled` mirrors the gating logic in `handle_enable_totp` (api-auth) and
/// `read_realm_passkey_enabled` above: true only when the config row itself
/// is enabled and its inner `enabled` flag is true — the single user-visible
/// signal the frontend uses to gate the TOTP tab. `force_enabled` follows the
/// repository reader (`get_realm_totp_config`, used by `handle_disable_totp`):
/// the inner JSON flag alone, regardless of the row-level `enabled` column —
/// force mode guides but never blocks, so a disabled row still reports force.
/// Best-effort like the other readers: a query failure never fails feature
/// availability.
async fn read_realm_totp_flags(state: &AppState, realm_id: &str) -> (bool, bool) {
    let row = sqlx::query_as::<_, (bool, String)>(
        "SELECT enabled, config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = $2 AND config_key = 'settings'",
    )
    .bind(realm_id)
    .bind(ConfigType::Totp.as_ref())
    .fetch_optional(&state.pool)
    .await;
    match row {
        Ok(Some((row_enabled, value))) => {
            let parsed = serde_json::from_str::<serde_json::Value>(&value).ok();
            let flag = |key: &str| {
                parsed
                    .as_ref()
                    .and_then(|v| v.get(key))
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false)
            };
            (row_enabled && flag("enabled"), flag("force_enabled"))
        }
        Ok(None) => (false, false),
        Err(e) => {
            tracing::warn!(realm_id = %realm_id, error = %e, "Failed to read totp realm config");
            (false, false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_facts() -> FeatureFacts {
        FeatureFacts {
            has_payment_providers: false,
            has_entitlement_mappings: false,
            has_enabled_mappings: false,
            has_one_time_mappings: false,
            has_recurring_mappings: false,
            has_invoice_seller_config: false,
            has_invoices: false,
            has_subscription_history: false,
        }
    }

    #[test]
    fn points_area_hidden_when_no_enabled_mappings() {
        let facts = no_facts();
        assert!(!points_area_visible(&facts));
    }

    #[test]
    fn points_area_visible_when_only_one_time_mappings() {
        let mut facts = no_facts();
        facts.has_enabled_mappings = true;
        facts.has_one_time_mappings = true;
        assert!(points_area_visible(&facts));
    }

    #[test]
    fn points_area_visible_when_only_recurring_mappings() {
        // US-CB-008 / US-CB-005: a realm with recurring prices (month/year)
        // but no one-time packs must still expose the points area — its users
        // hold subscription-granted (and registration) credit.
        let mut facts = no_facts();
        facts.has_enabled_mappings = true;
        facts.has_recurring_mappings = true;
        assert!(points_area_visible(&facts));
    }
}
