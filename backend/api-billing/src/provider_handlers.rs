//! Payment provider directory handlers.
//!
//! Lists all configured payment providers for a realm (Stripe / Creem /
//! Apple IAP / Google Play Billing). These handlers are provider-agnostic;
//! per-provider CRUD lives in the dedicated `<provider>_config_handlers`
//! modules.

use axum::{
    Json,
    extract::{Extension, Path, State},
};

use crate::provider_common_types::{PaymentProviderInfo, PaymentProvidersResponse};
use herald_api_base::application::http::common::auth_utils::{
    require_authenticated_user_in_realm_with_token, require_token_scope,
};
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialScope, Identity, TokenCredentialContext};
use herald_core::domain::realm_config::RealmConfigRepository;

#[utoipa::path(
    get,
    path = "/api/third/pay/{realmId}/providers",
    params(
        ("realmId" = String, Path, description = "Realm UUID")
    ),
    responses(
        (status = 200, description = "Payment providers retrieved successfully. Stripe entries include the non-secret publishableKey (pk_...) for mobile wallet SDK initialization; other providers omit it.", body = PaymentProvidersResponse),
        (status = 401, description = "Unauthorized - No valid authentication token"),
        (status = 403, description = "Forbidden - User does not have access to this realm"),
        (status = 404, description = "Realm not found")
    ),
    tag = "billing.payment-providers",
    operation_id = "list_payment_providers"
)]
pub async fn list_payment_providers(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
) -> Result<Json<PaymentProvidersResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PurchaseRead)?;
    let _user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "list payment providers",
    )?;

    // The provider-config lookups are independent; run them concurrently
    // rather than as sequential DB roundtrips. A provider appears in the
    // directory iff its "configured-signal" key is present in realm_config:
    //   stripe → publishable_key, creem → api_key,
    //   apple  → issuer_id,        google → service_account_json,
    //   wechat → mch_id.
    let (stripe, creem, apple, google, wechat) = tokio::try_join!(
        load_configured_provider(
            &state,
            &realm_id,
            "stripe",
            "publishable_key",
            Some("Stripe webhooks configured".to_string()),
        ),
        load_configured_provider(
            &state,
            &realm_id,
            "creem",
            "api_key",
            Some("Creem webhooks configured".to_string()),
        ),
        load_configured_provider(
            &state,
            &realm_id,
            "apple",
            "issuer_id",
            Some(format!("/api/third/pay/{}/apple/webhooks", realm_id)),
        ),
        load_configured_provider(&state, &realm_id, "google", "service_account_json", None,),
        load_configured_provider(
            &state,
            &realm_id,
            "wechat",
            "mch_id",
            Some(format!("/api/third/pay/{}/wechat/webhooks", realm_id)),
        ),
    )?;
    let providers: Vec<PaymentProviderInfo> = [stripe, creem, apple, google, wechat]
        .into_iter()
        .flatten()
        .collect();

    Ok(Json(PaymentProvidersResponse { providers }))
}

/// Load a single provider's directory entry. Returns `None` when the realm has
/// no `config_type = <provider>` rows or the configured-signal key is absent.
///
/// `webhook_endpoint` is the human-readable / URL hint surfaced in the
/// directory: Stripe/Creem get a static "configured" label, Apple gets its SSV
/// V2 webhook URL, Google gets `None` (lifecycle is driven by polling, design
/// support-iap §5.7).
async fn load_configured_provider(
    state: &AppState,
    realm_id: &str,
    provider: &str,
    configured_key: &str,
    webhook_endpoint: Option<String>,
) -> Result<Option<PaymentProviderInfo>, ApiError> {
    let configs = state
        .realm_config_repository
        .get_by_type(realm_id.to_string(), provider.to_string())
        .await
        .map_err(|e| {
            tracing::error!("Failed to load {provider} configuration: {e}");
            ApiError::internal(format!("Database error: {e}"))
        })?;

    if !configs.iter().any(|rc| rc.config_key == configured_key) {
        return Ok(None);
    }

    let last_updated = configs.iter().map(|rc| rc.updated_at).max();

    // Stripe exposes its non-secret publishable key so a mobile app's Stripe
    // SDK can initialize for the PaymentIntent wallet flow (the
    // stripe-payment PRD allows client exposure of the publishable key
    // only). Other providers expose no key material.
    let publishable_key = if provider == "stripe" {
        configs
            .iter()
            .find(|rc| rc.config_key == "publishable_key")
            .map(|rc| rc.config_value.clone())
    } else {
        None
    };

    Ok(Some(PaymentProviderInfo {
        platform: provider.to_string(),
        webhook_endpoint,
        last_updated: last_updated.map(|dt| dt.to_rfc3339()),
        publishable_key,
        ..Default::default()
    }))
}
