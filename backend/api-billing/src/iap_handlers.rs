//! IAP receipt submission + Apple SSV V2 webhook + compensation skeletons
//!
//! This module wires the `herald-infra-iap` Apple verifier / Google Developer
//! API client into the unified purchase lifecycle:
//!
//! - [`submit_iap_receipt`] — the reverse-payment main path (Apple
//!   `jwsRepresentation` / Google `purchaseToken` → verify → resolve mapping →
//!   create attempt → fulfil + Google ack/consume in-tx → idempotent
//! - [`handle_apple_webhook`] — Apple App Store Server Notifications V2
//!   receiver (always 200; JWS verification is the trust root; idempotency key
//! - [`reprocess_apple_event`] / [`reprocess_google_event`] — **compilable
//!   skeletons** with FIXED signatures consumed by `compensation.rs`. The full
//!
//! # Credentials loading
//!
//! shared helper in the codebase today, so this module owns a private
//! [`load_iap_credentials`] implementation that reads `realm_config` rows of
//! existing `realm_config_repository.get_by_type`.

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

use crate::webhook_subscription_helpers::mapping_rule_value;
use herald_api_base::application::http::common::auth_utils::{
    require_authenticated_user_in_realm_with_token, require_token_scope,
};
use herald_api_base::application::http::common::error_helpers::core_error_to_api_error;
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::{CredentialScope, Identity, TokenCredentialContext};
use herald_core::domain::billing::BillingRepository;
use herald_core::domain::billing::entities::{
    BillingType, EntitlementMapping, PaymentEvent, SubscriptionStatus,
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::payment_attempt::{PaymentAttemptStatus, PurchasableTarget};
use herald_core::domain::purchase::services::CreateIapAttemptInput;
use herald_core::domain::realm_config::RealmConfigRepository;
use herald_core::domain::user::UserRepository;
use herald_infra_iap::google::service_account::GoogleServiceAccountAuth;
use herald_infra_iap::{AppleEnvironment, AppleVerifier, GoogleDeveloperClient, IapError};
use validator::Validate;

use crate::shared_fulfillment::fulfill_provider_event;
use crate::webhook_subscription_helpers::{
    SyncSubscriptionInput, resolve_entitlement_mapping, sync_subscription,
};

// ============================================================================
// DTOs
// ============================================================================

/// Mobile App submits this after the store purchase completes.
#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema, validator::Validate)]
#[serde(rename_all = "camelCase")]
pub struct IapReceiptRequest {
    /// `"apple"` or `"google"`.
    #[validate(custom(function = "validate_iap_provider"))]
    pub provider: String,
    /// Apple StoreKit 2 `jwsRepresentation` (JWS) or Google `purchaseToken`.
    pub receipt: String,
    /// Store product id (resolves the local entitlement mapping).
    pub product_id: String,
    /// Always `"entitlement_mapping"`.
    #[validate(custom(function = "validate_iap_target_type"))]
    pub target_type: String,
    /// Entitlement mapping id (mobile App obtains it from the list endpoint).
    pub target_id: Uuid,
}

/// Stripe/Creem branded `PaymentContext`.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IapReceiptResponse {
    pub attempt_id: Uuid,
    /// `"succeeded"` / `"pending"` / `"failed"`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entitlement_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

fn validate_iap_provider(provider: &str) -> Result<(), validator::ValidationError> {
    if matches!(provider, "apple" | "google") {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_provider"))
    }
}

fn validate_iap_target_type(target_type: &str) -> Result<(), validator::ValidationError> {
    if matches!(target_type, "entitlement_mapping") {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_target_type"))
    }
}

// ============================================================================
// ============================================================================

/// Loaded Apple credentials for a realm.
struct AppleCredentials {
    bundle_id: String,
    #[allow(dead_code)]
    issuer_id: String,
    #[allow(dead_code)]
    key_id: String,
    /// `.p8` PEM bytes. Consumed by the Apple Server API client in the
    /// only need the verifier (bundle_id + environment).
    #[allow(dead_code)]
    private_key_p8: Vec<u8>,
    environment: AppleEnvironment,
}

/// Loaded Google credentials for a realm.
struct GoogleCredentials {
    package_name: String,
    /// Parsed service-account JSON.
    service_account: ServiceAccountJson,
    /// Optional per-realm Developer API + OAuth token-endpoint override
    /// (`realm_config.google.base_url`). Mirrors the Stripe/Creem
    /// `base_url` realm-config injection pattern used by
    /// `webhook_compensation_job::compensate_stripe` / `compensate_creem`:
    /// production leaves this `None` (clients hit the real Google endpoints);
    /// tests inject a wiremock URI so both the Developer API and the
    /// `/token` OAuth grant hit the mock. When present, the token URI is
    /// derived as `{base_url}/token` (matches the `infra-iap` developer-api
    /// unit-test convention `format!("{}/token", server.uri())`).
    base_url: Option<String>,
}

#[derive(Deserialize)]
struct ServiceAccountJson {
    client_email: String,
    private_key: String,
}

/// Load IAP credentials for a realm+provider from `realm_config`
/// `IapError::NotConfigured` when a required key is missing or empty.
async fn load_apple_credentials(
    state: &AppState,
    realm_id: &str,
) -> Result<AppleCredentials, IapError> {
    let map = load_config_map(state, realm_id, "apple").await?;
    let get = |key: &str| -> Result<String, IapError> {
        map.get(key)
            .filter(|v| !v.is_empty())
            .cloned()
            .ok_or_else(|| IapError::NotConfigured {
                realm_id: realm_id.to_string(),
                provider: "apple".to_string(),
            })
    };

    let environment = match get("environment")?.as_str() {
        "production" => AppleEnvironment::Production,
        "sandbox" => AppleEnvironment::Sandbox,
        // Treat Xcode / LocalTesting spelling defensively; Production/Sandbox
        // are the only values the admin form writes.
        other => {
            return Err(IapError::NotConfigured {
                realm_id: realm_id.to_string(),
                provider: format!("apple (unsupported environment '{other}')"),
            });
        }
    };

    Ok(AppleCredentials {
        bundle_id: get("bundle_id")?,
        issuer_id: get("issuer_id")?,
        key_id: get("key_id")?,
        private_key_p8: get("private_key_p8")?.into_bytes(),
        environment,
    })
}

async fn load_google_credentials(
    state: &AppState,
    realm_id: &str,
) -> Result<GoogleCredentials, IapError> {
    let map = load_config_map(state, realm_id, "google").await?;
    let package_name = map
        .get("package_name")
        .filter(|v| !v.is_empty())
        .cloned()
        .ok_or_else(|| IapError::NotConfigured {
            realm_id: realm_id.to_string(),
            provider: "google".to_string(),
        })?;
    let raw = map
        .get("service_account_json")
        .filter(|v| !v.is_empty())
        .ok_or_else(|| IapError::NotConfigured {
            realm_id: realm_id.to_string(),
            provider: "google".to_string(),
        })?;

    let service_account: ServiceAccountJson =
        serde_json::from_str(raw).map_err(|e| IapError::NotConfigured {
            realm_id: realm_id.to_string(),
            provider: format!("google (invalid service_account_json: {e})"),
        })?;

    // Optional `base_url` override (Stripe/Creem `base_url` realm-config
    // pattern). Empty / absent → `None` → production Google endpoints.
    let base_url = map.get("base_url").filter(|v| !v.is_empty()).cloned();

    Ok(GoogleCredentials {
        package_name,
        service_account,
        base_url,
    })
}

/// Read all `realm_config` rows for `config_type` into a key→value map.
async fn load_config_map(
    state: &AppState,
    realm_id: &str,
    config_type: &str,
) -> Result<HashMap<String, String>, IapError> {
    let rows = state
        .realm_config_repository
        .get_by_type(realm_id.to_string(), config_type.to_string())
        .await
        .map_err(|e| IapError::NotConfigured {
            realm_id: realm_id.to_string(),
            provider: format!("{config_type} (config read failed: {e})"),
        })?;
    Ok(rows
        .into_iter()
        .map(|rc| (rc.config_key, rc.config_value))
        .collect())
}

/// Build the Apple JWS verifier rooted at the bundled Apple Root CA - G3.
fn apple_verifier_for(creds: &AppleCredentials) -> AppleVerifier {
    AppleVerifier::new(creds.bundle_id.clone(), creds.environment.clone())
}

/// Build the per-realm Google Developer API client. When `creds.base_url` is
/// set (test injection / wiremock), the client is rooted at that base;
/// otherwise it uses the production Play Developer API base (behaviour
/// unchanged for production realms with no `base_url` config row).
///
/// Takes the `AppState` so the shared `http_client` (a `reqwest::Client` whose
/// type is not directly named in this crate's dependencies) flows in by field
/// access; the `infra-iap` constructors infer the concrete type.
fn google_developer_client_for(
    state: &AppState,
    creds: &GoogleCredentials,
) -> GoogleDeveloperClient {
    let http = state.http_client.clone();
    match creds.base_url.as_ref() {
        Some(base) if !base.is_empty() => GoogleDeveloperClient::with_base_url(http, base.clone()),
        _ => GoogleDeveloperClient::new(http),
    }
}

/// Build the per-realm Google service-account authorizer. When
/// `creds.base_url` is set, the OAuth token endpoint is derived as
/// `{base_url}/token` so a wiremock `/token` stub is reachable; otherwise the
/// authorizer uses the production Google token URI (behaviour unchanged for
/// production realms).
fn google_service_account_auth_for(creds: &GoogleCredentials) -> GoogleServiceAccountAuth {
    match creds.base_url.as_ref() {
        Some(base) if !base.is_empty() => GoogleServiceAccountAuth::with_token_uri(
            creds.service_account.client_email.clone(),
            creds.service_account.private_key.clone().into_bytes(),
            format!("{}/token", base.trim_end_matches('/')),
        ),
        _ => GoogleServiceAccountAuth::new(
            creds.service_account.client_email.clone(),
            creds.service_account.private_key.clone().into_bytes(),
        ),
    }
}

/// Best-effort idempotent `payment_event` insert shared by the IAP receipt path
/// and the Apple SSV V2 webhook path. A duplicate insert is benign — the unique
/// constraint on `(realm_id, external_event_id, payment_provider)` guards it,
/// and a later resubmit returns the existing attempt.
async fn record_idempotent_payment_event(
    state: &AppState,
    realm_id: &str,
    external_event_id: &str,
    provider: &str,
    event_type: String,
    payload: serde_json::Value,
) {
    let _ = state
        .billing_repository
        .create_payment_event(PaymentEvent {
            id: Uuid::now_v7(),
            realm_id: realm_id.to_string(),
            external_event_id: external_event_id.to_string(),
            payment_provider: provider.to_string(),
            event_type,
            subscription_id: None,
            payload,
            processed: true,
            processing_started_at: Some(Utc::now()),
            created_at: Utc::now(),
        })
        .await;
}

/// Best-effort IAP operation audit (PRD support-iap.md §5.2: all IAP purchase /
/// fulfillment / lifecycle operations are audited). An audit write failure
/// never fails the payment path.
async fn record_iap_audit(
    state: &AppState,
    realm_id: &str,
    action: AuditAction,
    actor_id: Option<String>,
    actor_type: Option<ActorType>,
    target_id: String,
    details: serde_json::Value,
) {
    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.to_string(),
            category: AuditCategory::Billing,
            action,
            actor_id: actor_id.unwrap_or_else(|| "system".to_string()),
            actor_type,
            actor_name: None,
            target_type: AuditTargetType::Payment,
            target_id,
            target_name: None,
            result: AuditResult::Success,
            details: Some(details),
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record IAP audit event");
    }
}

/// Stable string form of an Apple V2 notification type — used both in
/// synthetic idempotency keys and in event/audit details. Falls back to a
/// literal `"UNKNOWN"` marker when serde cannot serialize the enum.
fn apple_notification_type_str(
    notification_type: &herald_infra_iap::apple::models::NotificationTypeV2,
) -> String {
    serde_json::to_string(notification_type).unwrap_or_else(|_| "\"UNKNOWN\"".to_string())
}

/// Shared idempotency probe for Apple lifecycle notifications: true when a
/// payment event with this synthetic external id already exists, meaning the
/// notification was already processed and the caller must skip
/// re-fulfillment.
async fn apple_event_already_processed(
    state: &AppState,
    realm_id: &str,
    synthetic_event_id: &str,
) -> Result<bool, CoreError> {
    let already = state
        .billing_repository
        .find_payment_event_by_external_id(realm_id, synthetic_event_id, "apple")
        .await?
        .is_some();
    if already {
        tracing::info!(
            realm_id = %realm_id,
            external_id = %synthetic_event_id,
            "apple notification already processed -- skipping"
        );
    }
    Ok(already)
}

// ============================================================================
// ============================================================================

#[utoipa::path(
    post,
    path = "/api/bill/{realmId}/purchase/iap/receipt",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = IapReceiptRequest,
    responses(
        (status = 200, description = "Receipt processed", body = IapReceiptResponse),
        (status = 400, description = "Invalid request (provider / receipt / productId / targetId missing)"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Mapping not found or IAP credentials not configured"),
        (status = 409, description = "ownership_mismatch / type_mismatch / no_billing_type"),
        (status = 422, description = "verification_failed / already_consumed")
    ),
    security(("bearer_auth" = []))
)]
pub async fn submit_iap_receipt(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Path(realm_id): Path<String>,
    Json(input): Json<IapReceiptRequest>,
) -> Result<Json<IapReceiptResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PurchaseInitiate)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "iap receipt",
    )?;
    input
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    // Steps 1-2: resolve the entitlement mapping. `resolve_entitlement_mapping`
    // is the existing price-aware resolver; IAP is price-less (external_price_id
    // = None, no metadata key), so it falls through to the single-row
    // (provider, product) lookup. Missing → 404 no_mapping; disabled → 409
    // mapping_disabled (the client surface must not submit purchases against
    // a disabled product; server notifications keep the projection-only path).
    let resolved = resolve_entitlement_mapping(
        &state,
        &realm_id,
        &input.provider,
        &input.product_id,
        None,
        None,
    )
    .await
    .map_err(|e| ApiError::not_found(format!("no_mapping: {e}")))?;
    if !resolved.mapping.enabled {
        return Err(ApiError::conflict(
            "mapping_disabled: this product mapping is disabled".to_string(),
        ));
    }

    // The receipt is verified against `input.product_id`, so the fulfilled
    // mapping must be the one that product resolved to. The client-supplied
    // target_id is otherwise an arbitrary grant selector (buy cheap product A,
    // submit its valid receipt against expensive mapping B).
    if input.target_id != resolved.mapping.id {
        return Err(ApiError::conflict("type_mismatch".to_string()));
    }

    let billing_type = resolved
        .mapping
        .billing_type
        .ok_or_else(|| ApiError::conflict("no_billing_type".to_string()))?;

    // Step 3: load IAP credentials (404 NotConfigured when missing).
    // Step 4: provider-specific verification + ownership check + external id.
    //
    // Google verify (here, step 4) and ack/consume (step 7) both need the
    // realm's credentials + clients, so load them once and reuse across both
    // steps — avoids a second realm_config read, service-account JSON parse,
    // and OAuth token grant per receipt. Apple has no ack step, so its
    // credentials stay scoped to the verify arm below.
    let google_ready = if input.provider == "google" {
        let creds = load_google_credentials(&state, &realm_id)
            .await
            .map_err(iap_error_to_api_error)?;
        let developer = google_developer_client_for(&state, &creds);
        let auth = google_service_account_auth_for(&creds);
        Some((creds, developer, auth))
    } else {
        None
    };
    let external_txn_id = match input.provider.as_str() {
        "apple" => {
            let creds = load_apple_credentials(&state, &realm_id).await;
            let creds = creds.map_err(iap_error_to_api_error)?;
            let verifier = apple_verifier_for(&creds);
            let txn = verifier
                .verify_signed_transaction(&input.receipt)
                .map_err(iap_error_to_api_error)?;

            // Ownership: appAccountToken (a UUID set by the client) must match
            // the requesting user id. If absent we fail closed — the client is
            if txn.app_account_token != Some(user_id) {
                return Err(iap_error_to_api_error(IapError::OwnershipMismatch {
                    user_id,
                }));
            }

            // Product id sanity: the verified transaction's productId must
            // match what the client claimed (defence against submitting a
            // receipt for product A against mapping B).
            if let Some(ref txn_product) = txn.product_id
                && txn_product != &input.product_id
            {
                return Err(ApiError::conflict("type_mismatch".to_string()));
            }

            // The store-side product type (Non-Consumable /
            // Non-Renewing / Auto-Renewable) is a diagnostic only — the
            // mapping.billing_type is the fulfillment routing authority. On
            // mismatch we still fulfill per the mapping (the user already
            // paid) but emit a warn log so config errors surface in ops and
            // recon.
            if let Some(ref txn_type) = txn.r#type
                && !apple_txn_type_matches_billing_type(txn_type, &billing_type)
            {
                tracing::warn!(
                    realm_id = %realm_id,
                    product_id = %input.product_id,
                    apple_product_type = ?txn_type,
                    mapping_billing_type = %billing_type.as_str(),
                    "Apple product type does not match mapping billing_type — fulfilling per mapping (user already paid)"
                );
            }

            txn.original_transaction_id
                .unwrap_or_else(|| input.receipt.clone())
        }
        "google" => {
            let google = google_ready
                .as_ref()
                .expect("populated above when provider == google");
            let (creds, developer, auth) = (&google.0, &google.1, &google.2);

            let external_txn_id = input.receipt.clone();
            match billing_type {
                // Recurring and NonRenewing both verify through the
                // as subscription base plans, not as one-time products.
                BillingType::Recurring | BillingType::NonRenewing => {
                    let sub = developer
                        .get_subscription(auth, &creds.package_name, &input.receipt)
                        .await
                        .map_err(iap_error_to_api_error)?;
                    // Ownership: obfuscatedExternalAccountId must equal user id.
                    if sub.obfuscated_external_account_id.as_deref() != Some(&user_id.to_string()) {
                        return Err(iap_error_to_api_error(IapError::OwnershipMismatch {
                            user_id,
                        }));
                    }
                    // Product id sanity: at least one line item must carry the
                    // claimed product id (defence against submitting a token
                    // for product A against mapping B).
                    let product_matches = sub
                        .line_items
                        .iter()
                        .any(|li| li.product_id.as_deref() == Some(&input.product_id));
                    if !product_matches {
                        return Err(ApiError::conflict("type_mismatch".to_string()));
                    }
                }
                BillingType::OneTime => {
                    let product = developer
                        .get_product(auth, &creds.package_name, &input.product_id, &input.receipt)
                        .await
                        .map_err(iap_error_to_api_error)?;
                    if product.obfuscated_external_account_id.as_deref()
                        != Some(&user_id.to_string())
                    {
                        return Err(iap_error_to_api_error(IapError::OwnershipMismatch {
                            user_id,
                        }));
                    }
                    // consumptionState 1 == already consumed → 422 already_consumed.
                    if product.consumption_state == Some(1) {
                        return Err(iap_error_to_api_error(IapError::AlreadyConsumed));
                    }
                }
            }
            external_txn_id
        }
        other => {
            return Err(ApiError::bad_request(format!(
                "unsupported IAP provider: {other}"
            )));
        }
    };

    // Step 5: idempotency. If a payment_event already exists for this
    // external id + provider, return the existing attempt's status without
    // re-fulfilling (US-IAP-003 scenario 4).
    if let Some(existing) = state
        .billing_repository
        .find_payment_event_by_external_id(&realm_id, &external_txn_id, &input.provider)
        .await
        .map_err(|e| core_error_to_api_error(e, "iap receipt idempotency lookup"))?
    {
        return Ok(Json(
            iap_response_for_existing_event(&state, &realm_id, &input.provider, &existing).await?,
        ));
    }

    // Step 6: create the IAP payment attempt (Pending; provider_reference =
    // external_txn_id). Reuses resolve_target + row creation, skips
    let target_type = input
        .target_type
        .parse::<PurchasableTarget>()
        .map_err(|e| ApiError::bad_request(format!("invalid target_type: {e}")))?;
    let attempt = state
        .purchase_service
        .create_iap_payment_attempt(CreateIapAttemptInput {
            realm_id: realm_id.clone(),
            user_id,
            payment_provider: input.provider.clone(),
            target_type,
            target_id: input.target_id,
            provider_reference: external_txn_id.clone(),
            metadata: None,
        })
        .await
        .map_err(|e| core_error_to_api_error(e, "iap create attempt"))?;

    // Step 7: fulfillment transaction. complete_succeeded marks the attempt
    // Succeeded and fulfils (one_time → TopupCredit, recurring → Subscription).
    // A failure here rolls the attempt back to non-succeeded.
    if let Some((creds, developer, auth)) = google_ready.as_ref() {
        let is_consumable_points_pack = mapping_rule_value(&state, &realm_id, resolved.mapping.id)
            .await
            .map_err(|e| core_error_to_api_error(e, "iap mapping rules"))?
            > 0;
        google_ack_or_consume_in_tx(
            developer,
            auth,
            &creds.package_name,
            &input,
            &billing_type,
            is_consumable_points_pack,
        )
        .await
        .map_err(iap_error_to_api_error)?;
    }

    let billing_type_str = billing_type.as_str().to_string();
    let fulfill_result = fulfill_provider_event(
        &state,
        &realm_id,
        attempt.id,
        &input.provider,
        "succeeded",
        external_txn_id.clone(),
        Utc::now(),
        Some(billing_type),
    )
    .await;

    let status = match fulfill_result {
        Ok(()) => {
            // Step 7 cont.: record payment_event for idempotency. Best-effort:
            // a duplicate-insert here is benign (the unique constraint guards
            // it; a later resubmit returns the existing attempt).
            record_idempotent_payment_event(
                &state,
                &realm_id,
                &external_txn_id,
                &input.provider,
                format!("iap_{billing_type_str}"),
                serde_json::json!({
                    "provider": input.provider,
                    "productId": input.product_id,
                    "targetId": input.target_id,
                }),
            )
            .await;
            "succeeded"
        }
        Err(e) => {
            tracing::warn!(
                realm_id = %realm_id,
                attempt_id = %attempt.id,
                provider = %input.provider,
                error = %e,
                "IAP fulfillment failed -- attempt left non-succeeded"
            );
            "failed"
        }
    };

    record_iap_audit(
        &state,
        &realm_id,
        AuditAction::IapReceiptSubmit,
        Some(user_id.to_string()),
        Some(ActorType::User),
        attempt.id.to_string(),
        serde_json::json!({
            "provider": input.provider,
            "productId": input.product_id,
            "billingType": billing_type_str,
            "status": status,
        }),
    )
    .await;

    Ok(Json(IapReceiptResponse {
        attempt_id: attempt.id,
        status: status.to_string(),
        entitlement_key: Some(resolved.entitlement_key.clone()),
        billing_type: Some(billing_type_str),
        failure_reason: if status == "failed" {
            Some("verification_failed".to_string())
        } else {
            None
        },
    }))
}

/// Google acknowledge (recurring/non_renewing) / consume-or-acknowledge
/// marked succeeded (the caller maps the error to 422). Receives the clients
/// built during receipt verification (step 4) so the
/// `GoogleServiceAccountAuth` access-token cache is reused rather than
/// re-granted.
///
/// an enabled fixed grant rule makes the purchase a consumable points pack;
/// a one-time mapping with no points (buyout / non-consumable) →
/// `acknowledge_product` so a later "restore purchases" can still see the
/// owned entitlement.
async fn google_ack_or_consume_in_tx(
    developer: &GoogleDeveloperClient,
    auth: &GoogleServiceAccountAuth,
    package_name: &str,
    input: &IapReceiptRequest,
    billing_type: &BillingType,
    is_consumable_points_pack: bool,
) -> Result<(), IapError> {
    match billing_type {
        // Recurring and NonRenewing both use the subscriptionsv2 acknowledge
        // One_time is the only consume path.
        BillingType::Recurring | BillingType::NonRenewing => {
            developer
                .acknowledge_subscription(auth, package_name, &input.receipt)
                .await
        }
        BillingType::OneTime => {
            match google_one_time_ack_action(is_consumable_points_pack) {
                GoogleOneTimeAckAction::Consume => {
                    // Consumable points pack: consume so it can be re-purchased.
                    developer
                        .consume_product(auth, package_name, &input.product_id, &input.receipt)
                        .await
                }
                GoogleOneTimeAckAction::Acknowledge => {
                    // Buyout / non-consumable: acknowledge only (no consume) so
                    // "restore purchases" still sees the owned entitlement and
                    // Google does not auto-refund after 3 days.
                    developer
                        .acknowledge_product(auth, package_name, &input.product_id, &input.receipt)
                        .await
                }
            }
        }
    }
}

/// Cross-check the Apple store-side `ProductType` against the mapping's
/// semantically aligned. This is a *diagnostic only* — on mismatch the caller
/// still fulfills per the mapping (the user already paid) and emits a warn log.
///
/// Alignment table:
/// - `AutoRenewableSubscription` ↔ `Recurring`
/// - `NonRenewingSubscription`    ↔ `NonRenewing`
/// - `NonConsumable` / `Consumable` ↔ `OneTime`
fn apple_txn_type_matches_billing_type(
    txn_type: &herald_infra_iap::apple::models::ProductType,
    billing_type: &BillingType,
) -> bool {
    use herald_infra_iap::apple::models::ProductType;
    matches!(
        (txn_type, billing_type),
        (
            ProductType::AutoRenewableSubscription,
            BillingType::Recurring
        ) | (
            ProductType::NonRenewingSubscription,
            BillingType::NonRenewing
        ) | (
            ProductType::NonConsumable | ProductType::Consumable,
            BillingType::OneTime
        )
    )
}

/// Resolve the existing attempt status for an idempotent re-submission. The
/// event is already processed, so we look up the attempt by provider_reference
/// (= external_event_id) and provider.
async fn iap_response_for_existing_event(
    state: &AppState,
    realm_id: &str,
    provider: &str,
    event: &PaymentEvent,
) -> Result<IapReceiptResponse, ApiError> {
    // Best-effort: re-resolve the mapping to enrich entitlement_key. If the
    // mapping has since been deleted, fall back to a bare status response.
    let entitlement_key = state
        .billing_repository
        .find_entitlement_mapping_by_provider_product_price(
            realm_id,
            provider,
            event
                .payload
                .get("productId")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            None,
        )
        .await
        .ok()
        .flatten()
        .map(|m| m.entitlement_key);

    let attempt = state
        .payment_attempt_service
        .get_payment_attempt_by_provider_reference(provider, &event.external_event_id)
        .await
        .map_err(|e| core_error_to_api_error(e, "iap attempt idempotency lookup"))?
        .filter(|attempt| attempt.realm_id == realm_id)
        .ok_or_else(|| {
            ApiError::internal(
                "IAP idempotency event exists without its payment attempt".to_string(),
            )
        })?;
    let status = match attempt.status {
        PaymentAttemptStatus::Succeeded => "succeeded",
        PaymentAttemptStatus::Pending | PaymentAttemptStatus::RequiresAction => "pending",
        PaymentAttemptStatus::Failed
        | PaymentAttemptStatus::Cancelled
        | PaymentAttemptStatus::Expired => "failed",
    };

    Ok(IapReceiptResponse {
        attempt_id: attempt.id,
        status: status.to_string(),
        entitlement_key,
        billing_type: None,
        failure_reason: (status == "failed").then(|| {
            attempt
                .provider_status
                .unwrap_or_else(|| "payment_failed".to_string())
        }),
    })
}

// ============================================================================
// ============================================================================

#[utoipa::path(
    post,
    path = "/api/third/pay/{realmId}/apple/webhooks",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        // Apple does not consume 4xx; we always return 200. Verification /
        (status = 200, description = "Notification received (always 200)")
    )
)]
pub async fn handle_apple_webhook(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    body: String,
) -> StatusCode {
    // Always 200 to avoid Apple's retry storm. Failures are logged.
    if let Err(e) = process_apple_notification(&state, &realm_id, &body).await {
        tracing::warn!(
            realm_id = %realm_id,
            error = %e,
            "apple webhook processing failed"
        );
    }
    StatusCode::OK
}

async fn process_apple_notification(
    state: &AppState,
    realm_id: &str,
    body: &str,
) -> Result<(), CoreError> {
    let creds = load_apple_credentials(state, realm_id)
        .await
        .map_err(iap_error_to_core_error)?;
    let verifier = apple_verifier_for(&creds);

    let notification = verifier
        .verify_and_decode_notification(body)
        .map_err(iap_error_to_core_error)?;

    // The transaction detail is a *separate* signed JWS embedded in
    // data.signedTransactionInfo. Verify + decode it through the same verifier.
    let signed_txn = notification
        .data
        .as_ref()
        .and_then(|d| d.signed_transaction_info.as_deref())
        .ok_or_else(|| {
            CoreError::BadRequest("apple notification missing signedTransactionInfo".to_string())
        })?;
    let txn = verifier
        .verify_signed_transaction(signed_txn)
        .map_err(iap_error_to_core_error)?;

    process_apple_notification_decoded(state, realm_id, &verifier, &notification, &txn).await
}

/// Post-verification core of [`process_apple_notification`]: everything after
/// the JWS chain check (mapping resolution, lifecycle dispatch, subscription
/// projection, points grants, idempotency).
///
/// The verify-decode step lives in the caller so tests can drive the full
/// lifecycle behaviour against a real database with decoded payloads instead
/// of forging an Apple-trusted JWS chain — the cryptographic layer stays
/// covered by the `infra-iap` verifier unit tests, and this function is the
/// seam the HTTP-layer happy-path scenario tests call directly.
pub async fn process_apple_notification_decoded(
    state: &AppState,
    realm_id: &str,
    verifier: &AppleVerifier,
    notification: &herald_infra_iap::apple::models::ResponseBodyV2DecodedPayload,
    txn: &herald_infra_iap::apple::models::JWSTransactionDecodedPayload,
) -> Result<(), CoreError> {
    let product_id = txn.product_id.clone().ok_or_else(|| {
        CoreError::BadRequest("apple notification transaction missing productId".to_string())
    })?;
    let original_transaction_id = txn.original_transaction_id.clone().ok_or_else(|| {
        CoreError::BadRequest(
            "apple notification transaction missing originalTransactionId".to_string(),
        )
    })?;

    let resolved = resolve_entitlement_mapping(state, realm_id, "apple", &product_id, None, None)
        .await
        .map_err(|e| CoreError::BadRequest(e.to_string()))?;
    let billing_type = resolved.mapping.billing_type.clone().ok_or_else(|| {
        CoreError::BadRequest(format!(
            "apple mapping '{}' has no billing_type",
            resolved.mapping.id
        ))
    })?;

    // REFUND / REVOKE notifications are revocation events, not first-purchase
    // the "create attempt + fulfill" path. They key their own payment_event
    // idempotency on `{originalTransactionId}:{notificationType}` so they are
    // NOT deduped against (and do NOT dedupe) the original purchase event.
    use herald_infra_iap::apple::models::NotificationTypeV2;
    let notification_type = notification.notification_type.clone();
    if matches!(
        notification_type,
        NotificationTypeV2::Refund | NotificationTypeV2::Revoke
    ) {
        return process_apple_refund_or_revoke(
            state,
            realm_id,
            &original_transaction_id,
            &billing_type,
            &notification_type,
            &product_id,
        )
        .await;
    }

    // Lifecycle notifications (PRD support-iap.md §3.2/§5.1: Apple server
    // notifications drive renewal / cancellation / expiry). They MUST be
    // dispatched before the first-purchase idempotency skip below, which
    // would otherwise swallow every post-purchase notification against the
    // original purchase's payment_event.
    match notification_type {
        NotificationTypeV2::DidRenew => {
            return process_apple_renewal(
                state,
                realm_id,
                notification,
                txn,
                &resolved.mapping,
                &billing_type,
                &original_transaction_id,
                &product_id,
            )
            .await;
        }
        NotificationTypeV2::Expired | NotificationTypeV2::GracePeriodExpired => {
            return process_apple_expiration(
                state,
                realm_id,
                notification,
                &original_transaction_id,
                &product_id,
            )
            .await;
        }
        NotificationTypeV2::DidFailToRenew => {
            return process_apple_renewal_failure(
                state,
                realm_id,
                verifier,
                notification,
                &original_transaction_id,
                &product_id,
            )
            .await;
        }
        NotificationTypeV2::DidChangeRenewalStatus => {
            return process_apple_renewal_status_change(
                state,
                realm_id,
                verifier,
                notification,
                &original_transaction_id,
                &product_id,
            )
            .await;
        }
        // Subscribed / one-time events and informational notifications fall
        // through to the first-purchase path below (which idempotency-skips
        // already-processed originals).
        _ => {}
    }

    // Idempotency: payment_event keyed by originalTransactionId.
    if state
        .billing_repository
        .find_payment_event_by_external_id(realm_id, &original_transaction_id, "apple")
        .await?
        .is_some()
    {
        tracing::info!(
            realm_id = %realm_id,
            original_transaction_id = %original_transaction_id,
            "apple notification already processed -- skipping"
        );
        return Ok(());
    }

    // Attribute the attempt to the real purchaser where possible. The Apple
    // notification path has no client user_id (the webhook is unauthenticated),
    // but Herald's client receipt path REQUIRES appAccountToken == user id, so
    // webhook-only transactions can usually recover the owner from the same
    // verified field; otherwise fall back to the existing subscription's
    // owner. Refund/REVOKE clawbacks revoke by attempt.user_id — with the old
    // mapping-id placeholder they silently no-oped and the buyer kept refunded
    // entitlements.
    let mut attributed_user_id: Option<Uuid> = match txn.app_account_token {
        Some(uid) => match state.user_repository.get_user_by_id(uid).await {
            Ok(user) if user.realm_id == realm_id => Some(uid),
            _ => None,
        },
        None => None,
    };
    if attributed_user_id.is_none()
        && let Ok(Some(subscription)) = state
            .billing_repository
            .find_by_external_subscription_id(&original_transaction_id, "apple")
            .await
        && subscription.realm_id == realm_id
    {
        attributed_user_id = Some(subscription.user_id);
    }

    let attributed_user_id = attributed_user_id.ok_or_else(|| {
        CoreError::BadRequest(format!(
            "apple notification purchaser could not be attributed for transaction {original_transaction_id}"
        ))
    })?;

    let attempt = state
        .purchase_service
        .create_iap_payment_attempt(CreateIapAttemptInput {
            realm_id: realm_id.to_string(),
            user_id: attributed_user_id,
            payment_provider: "apple".to_string(),
            target_type: PurchasableTarget::EntitlementMapping,
            target_id: resolved.mapping.id,
            provider_reference: original_transaction_id.clone(),
            metadata: None,
        })
        .await?;

    fulfill_provider_event(
        state,
        realm_id,
        attempt.id,
        "apple",
        "succeeded",
        original_transaction_id.clone(),
        Utc::now(),
        Some(billing_type),
    )
    .await?;

    // Record payment_event for idempotency (best-effort).
    let notification_type_str = apple_notification_type_str(&notification.notification_type);
    record_idempotent_payment_event(
        state,
        realm_id,
        &original_transaction_id,
        "apple",
        format!("apple_{notification_type_str}"),
        serde_json::json!({
            "notificationType": notification_type_str,
            "productId": product_id,
        }),
    )
    .await;

    record_iap_audit(
        state,
        realm_id,
        AuditAction::IapNotification,
        None,
        Some(ActorType::System),
        original_transaction_id.to_string(),
        serde_json::json!({
            "provider": "apple",
            "notificationType": notification_type_str,
            "productId": product_id,
            "outcome": "purchase_fulfilled",
        }),
    )
    .await;

    Ok(())
}

/// payment_attempt (idempotency key = originalTransactionId), then routed by
/// the mapping's `billing_type`:
///
/// `OneTime` revokes permanent payment roles (source_id = attempt.id) plus any
/// topup credits granted from that attempt. `NonRenewing` sets the subscription
/// to Expired and revokes its payment roles (source_id = subscription.id).
/// `Recurring` is a no-op — recurring refund/cancel flows through the existing
///
/// Idempotency: a dedicated payment_event keyed on
/// `{originalTransactionId}:{notificationType}` so a replay does not re-revoke
/// (and is not deduped against the original purchase event).
async fn process_apple_refund_or_revoke(
    state: &AppState,
    realm_id: &str,
    original_transaction_id: &str,
    billing_type: &BillingType,
    notification_type: &herald_infra_iap::apple::models::NotificationTypeV2,
    product_id: &str,
) -> Result<(), CoreError> {
    let notification_type_str = apple_notification_type_str(notification_type);

    // Idempotency on a per-notification-type key so a replay of this REFUND /
    // REVOKE is a no-op, AND so it does not collide with the original purchase
    // event (which is keyed on the bare originalTransactionId).
    let synthetic_event_id = format!("apple:{original_transaction_id}:{notification_type_str}");
    if apple_event_already_processed(state, realm_id, &synthetic_event_id).await? {
        return Ok(());
    }

    // Look up the originating payment_attempt by provider_reference
    // (= originalTransactionId, the idempotency key used at submit_iap_receipt).
    let attempt = state
        .payment_attempt_service
        .get_payment_attempt_by_provider_reference("apple", original_transaction_id)
        .await
        .map_err(|e| {
            CoreError::InternalServerError(format!(
                "apple REFUND/REVOKE attempt lookup failed: {e}"
            ))
        })?;
    let attempt = match attempt {
        Some(a) if a.realm_id == realm_id => a,
        Some(a) => {
            // The provider-reference lookup is realm-free; a JWS verified for
            // this realm must not revoke another realm's attempt.
            tracing::warn!(
                realm_id = %realm_id,
                attempt_id = %a.id,
                attempt_realm_id = %a.realm_id,
                notification_type = %notification_type_str,
                "apple REFUND/REVOKE: attempt belongs to a different realm — skipping"
            );
            record_idempotent_payment_event(
                state,
                realm_id,
                &synthetic_event_id,
                "apple",
                format!("apple_{notification_type_str}"),
                serde_json::json!({
                    "notificationType": notification_type_str,
                    "productId": product_id,
                    "outcome": "foreign_realm_attempt",
                }),
            )
            .await;
            return Ok(());
        }
        None => {
            tracing::warn!(
                realm_id = %realm_id,
                original_transaction_id = %original_transaction_id,
                notification_type = %notification_type_str,
                "apple REFUND/REVOKE: no payment_attempt found for originalTransactionId — \
                 cannot revoke; recording event to prevent retry storms"
            );
            record_idempotent_payment_event(
                state,
                realm_id,
                &synthetic_event_id,
                "apple",
                format!("apple_{notification_type_str}"),
                serde_json::json!({
                    "notificationType": notification_type_str,
                    "productId": product_id,
                    "outcome": "no_attempt_found",
                }),
            )
            .await;
            return Ok(());
        }
    };

    match billing_type {
        BillingType::OneTime => {
            // Revoke topup credits granted from this attempt (source_id =
            // attempt.id, same as the grant). Best-effort: a missing ledger
            // (buyout mapping with no points) is a no-op.
            for bucket_id in crate::webhook_common::captured_bucket_ids(state, &attempt).await? {
                if let Err(e) = state
                    .points_service
                    .revoke_points_by_source_id(
                        realm_id,
                        attempt.user_id,
                        bucket_id,
                        &attempt.id.to_string(),
                        herald_core::domain::points::entities::RevocationType::RefundRevoke,
                        format!("Apple {notification_type_str} for attempt {}", attempt.id),
                    )
                    .await
                {
                    tracing::warn!(
                        realm_id = %realm_id,
                        attempt_id = %attempt.id,
                        bucket_id = %bucket_id,
                        error = %e,
                        "apple REFUND/REVOKE: topup points revoke failed (best-effort)"
                    );
                }
            }

            crate::webhook_common::revoke_payment_roles_for_source(
                state,
                realm_id,
                attempt.user_id,
                &attempt.id.to_string(),
            )
            .await;
        }
        BillingType::NonRenewing => {
            // Locate the non-renewing subscription by external_subscription_id
            // (= originalTransactionId at fulfillment time) and mark it Expired.
            let subscription = state
                .billing_repository
                .find_by_external_subscription_id(original_transaction_id, "apple")
                .await?;
            if let Some(mut sub) = subscription {
                // Capture identity before `sub` is moved into update_subscription.
                let sub_id = sub.id;
                let sub_user_id = sub.user_id;
                if sub.status != SubscriptionStatus::Expired {
                    sub.status = SubscriptionStatus::Expired;
                    sub.synced_at = Some(Utc::now());
                    sub.updated_at = Utc::now();
                    if let Err(e) = state.billing_repository.update_subscription(sub).await {
                        tracing::warn!(
                            realm_id = %realm_id,
                            original_transaction_id = %original_transaction_id,
                            error = %e,
                            "apple REFUND/REVOKE: failed to set non-renewing subscription Expired (best-effort)"
                        );
                    }
                }
                // Revoke the subscription's payment roles regardless of the
                // update outcome (source_id = subscription.id).
                crate::webhook_common::revoke_payment_roles_for_source(
                    state,
                    realm_id,
                    sub_user_id,
                    &sub_id.to_string(),
                )
                .await;
            } else {
                tracing::warn!(
                    realm_id = %realm_id,
                    original_transaction_id = %original_transaction_id,
                    "apple REFUND/REVOKE: no non-renewing subscription found; revoking roles by attempt only"
                );
                crate::webhook_common::revoke_payment_roles_for_source(
                    state,
                    realm_id,
                    attempt.user_id,
                    &attempt.id.to_string(),
                )
                .await;
            }
        }
        BillingType::Recurring => {
            // "Recurring => 维持现有行为"). Recurring refund/cancel flows through
            // the existing subscription sync path. Log and treat as processed.
            tracing::info!(
                realm_id = %realm_id,
                original_transaction_id = %original_transaction_id,
                notification_type = %notification_type_str,
                "apple REFUND/REVOKE for recurring billing_type — recurring flows through existing sync path"
            );
        }
    }

    // Record the synthetic payment_event so a replay is deduped.
    record_idempotent_payment_event(
        state,
        realm_id,
        &synthetic_event_id,
        "apple",
        format!("apple_{notification_type_str}"),
        serde_json::json!({
            "notificationType": notification_type_str,
            "productId": product_id,
            "originalTransactionId": original_transaction_id,
        }),
    )
    .await;

    record_iap_audit(
        state,
        realm_id,
        AuditAction::IapNotification,
        None,
        Some(ActorType::System),
        original_transaction_id.to_string(),
        serde_json::json!({
            "provider": "apple",
            "notificationType": notification_type_str,
            "productId": product_id,
            "outcome": "refund_or_revoke",
        }),
    )
    .await;

    Ok(())
}

/// Load the Apple subscription for a lifecycle notification, enforcing the
/// realm boundary. Returns `Ok(None)` after recording the idempotency event
/// when no subscription exists in this realm (a lifecycle event for a
/// purchase Herald never fulfilled must not error the webhook — Apple would
/// only retry the same dead payload).
async fn apple_subscription_for_lifecycle(
    state: &AppState,
    realm_id: &str,
    original_transaction_id: &str,
    synthetic_event_id: &str,
    notification_type_str: &str,
    product_id: &str,
    outcome: &str,
) -> Result<Option<herald_core::domain::billing::entities::Subscription>, CoreError> {
    let subscription = state
        .billing_repository
        .find_by_external_subscription_id(original_transaction_id, "apple")
        .await?;
    match subscription {
        Some(sub) if sub.realm_id == realm_id => Ok(Some(sub)),
        other => {
            tracing::warn!(
                realm_id = %realm_id,
                original_transaction_id = %original_transaction_id,
                notification_type = %notification_type_str,
                found_realm_id = ?other.map(|s| s.realm_id),
                "apple lifecycle notification: no matching subscription in realm — skipping"
            );
            record_idempotent_payment_event(
                state,
                realm_id,
                synthetic_event_id,
                "apple",
                format!("apple_{notification_type_str}"),
                serde_json::json!({
                    "notificationType": notification_type_str,
                    "productId": product_id,
                    "originalTransactionId": original_transaction_id,
                    "outcome": outcome,
                }),
            )
            .await;
            Ok(None)
        }
    }
}

/// DID_RENEW — advance the subscription period and grant the renewal points
/// through the same `handle_subscription_paid(is_renewal=true)` path the
/// Stripe invoice.payment_succeeded handler uses. The renewal period comes
/// from the signed transaction itself (`purchaseDate` → `expiresDate`), so
/// the grant's period-anchored event key is stable across replays.
///
/// Idempotency: keyed on the renewal's own `transactionId` (unique per
/// renewal), not the bare originalTransactionId, so successive renewals each
/// fulfill exactly once while notification replays no-op.
#[allow(clippy::too_many_arguments)]
async fn process_apple_renewal(
    state: &AppState,
    realm_id: &str,
    notification: &herald_infra_iap::apple::models::ResponseBodyV2DecodedPayload,
    txn: &herald_infra_iap::apple::models::JWSTransactionDecodedPayload,
    mapping: &herald_core::domain::billing::entities::EntitlementMapping,
    billing_type: &BillingType,
    original_transaction_id: &str,
    product_id: &str,
) -> Result<(), CoreError> {
    let notification_type_str = apple_notification_type_str(&notification.notification_type);

    if billing_type != &BillingType::Recurring {
        // Renewal notifications only exist for auto-renewable subscriptions;
        // any other mapping shape is a provider/model mismatch — record and
        // stop rather than fulfilling through the wrong billing shape.
        tracing::warn!(
            realm_id = %realm_id,
            original_transaction_id = %original_transaction_id,
            billing_type = ?billing_type,
            "apple DID_RENEW for non-recurring mapping — skipping renewal fulfillment"
        );
        record_idempotent_payment_event(
            state,
            realm_id,
            &format!("apple:{original_transaction_id}:{notification_type_str}"),
            "apple",
            format!("apple_{notification_type_str}"),
            serde_json::json!({
                "notificationType": notification_type_str,
                "productId": product_id,
                "outcome": "non_recurring_mapping",
            }),
        )
        .await;
        return Ok(());
    }

    // A renewal transaction carries its own transactionId; fall back to the
    // original only for degenerate Apple payloads (keeps the key well-defined).
    let transaction_id = txn
        .transaction_id
        .clone()
        .unwrap_or_else(|| original_transaction_id.to_string());
    let synthetic_event_id = format!("apple:{original_transaction_id}:renew:{transaction_id}");
    if apple_event_already_processed(state, realm_id, &synthetic_event_id).await? {
        return Ok(());
    }

    let Some(mut subscription) = apple_subscription_for_lifecycle(
        state,
        realm_id,
        original_transaction_id,
        &synthetic_event_id,
        &notification_type_str,
        product_id,
        "no_subscription",
    )
    .await?
    else {
        return Ok(());
    };

    let period_end = txn.expires_date.ok_or_else(|| {
        CoreError::BadRequest("apple DID_RENEW transaction missing expiresDate".to_string())
    })?;
    let period_start = txn.purchase_date.ok_or_else(|| {
        CoreError::BadRequest("apple DID_RENEW transaction missing purchaseDate".to_string())
    })?;
    if period_start >= period_end {
        return Err(CoreError::BadRequest(
            "apple DID_RENEW transaction has a degenerate (start >= end) period".to_string(),
        ));
    }

    let user_id = subscription.user_id;
    subscription.status = SubscriptionStatus::Active;
    subscription.current_period_start = Some(period_start);
    subscription.current_period_end = Some(period_end);
    subscription.cancel_at_period_end = false;
    subscription.cancel_at = None;
    subscription.synced_at = Some(Utc::now());
    subscription.updated_at = Utc::now();
    let subscription_id = subscription.id;
    if let Err(e) = state
        .billing_repository
        .update_subscription(subscription)
        .await
    {
        tracing::warn!(
            realm_id = %realm_id,
            subscription_id = %subscription_id,
            error = %e,
            "apple DID_RENEW: failed to advance subscription period (best-effort; grant continues)"
        );
    }

    // Disabled mapping: the projection above already landed; per PRD
    // support-iap §4.1 a disabled mapping must not grant points or roles —
    // re-enabling resumes grants at the next renewal.
    if !mapping.enabled {
        tracing::info!(
            realm_id = %realm_id,
            subscription_id = %subscription_id,
            original_transaction_id = %original_transaction_id,
            "apple DID_RENEW for disabled mapping — projection advanced, grants skipped"
        );
        record_idempotent_payment_event(
            state,
            realm_id,
            &synthetic_event_id,
            "apple",
            format!("apple_{notification_type_str}"),
            serde_json::json!({
                "notificationType": notification_type_str,
                "productId": product_id,
                "originalTransactionId": original_transaction_id,
                "transactionId": transaction_id,
                "outcome": "mapping_disabled",
            }),
        )
        .await;
        return Ok(());
    }

    state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            realm_id,
            mapping,
            true,
            period_start,
            period_end,
            notification.notification_uuid.clone(),
        )
        .await?;

    tracing::info!(
        realm_id = %realm_id,
        user_id = %user_id,
        subscription_id = %subscription_id,
        original_transaction_id = %original_transaction_id,
        transaction_id = %transaction_id,
        period_start = %period_start,
        period_end = %period_end,
        "apple DID_RENEW: subscription renewed and renewal grant executed"
    );

    record_idempotent_payment_event(
        state,
        realm_id,
        &synthetic_event_id,
        "apple",
        format!("apple_{notification_type_str}"),
        serde_json::json!({
            "notificationType": notification_type_str,
            "productId": product_id,
            "originalTransactionId": original_transaction_id,
            "transactionId": transaction_id,
        }),
    )
    .await;

    record_iap_audit(
        state,
        realm_id,
        AuditAction::IapNotification,
        None,
        Some(ActorType::System),
        original_transaction_id.to_string(),
        serde_json::json!({
            "provider": "apple",
            "notificationType": notification_type_str,
            "productId": product_id,
            "transactionId": transaction_id,
            "outcome": "renewed",
        }),
    )
    .await;

    Ok(())
}

/// EXPIRED / GRACE_PERIOD_EXPIRED — the subscription no longer grants
/// access. Marks the subscription Expired and routes the same
/// immediate-cancel revocation the Stripe customer.subscription.deleted
/// handler uses (revoke subscription-sourced points + payment roles).
async fn process_apple_expiration(
    state: &AppState,
    realm_id: &str,
    notification: &herald_infra_iap::apple::models::ResponseBodyV2DecodedPayload,
    original_transaction_id: &str,
    product_id: &str,
) -> Result<(), CoreError> {
    let notification_type_str = apple_notification_type_str(&notification.notification_type);
    let synthetic_event_id = format!("apple:{original_transaction_id}:{notification_type_str}");
    if apple_event_already_processed(state, realm_id, &synthetic_event_id).await? {
        return Ok(());
    }

    let Some(mut subscription) = apple_subscription_for_lifecycle(
        state,
        realm_id,
        original_transaction_id,
        &synthetic_event_id,
        &notification_type_str,
        product_id,
        "no_subscription",
    )
    .await?
    else {
        return Ok(());
    };

    let user_id = subscription.user_id;
    let subscription_id = subscription.id;
    let entitlement_key = subscription.entitlement_key.clone();

    if subscription.status != SubscriptionStatus::Expired {
        subscription.status = SubscriptionStatus::Expired;
        subscription.synced_at = Some(Utc::now());
        subscription.updated_at = Utc::now();
        if let Err(e) = state
            .billing_repository
            .update_subscription(subscription)
            .await
        {
            tracing::warn!(
                realm_id = %realm_id,
                original_transaction_id = %original_transaction_id,
                error = %e,
                "apple expiration: failed to mark subscription Expired"
            );
        }
    }

    state
        .subscription_service
        .handle_subscription_cancel(
            user_id,
            realm_id,
            subscription_id,
            herald_core::domain::points::subscription_service::CancelMode::ImmediateCancel,
            None,
            Some(&entitlement_key),
        )
        .await?;

    record_idempotent_payment_event(
        state,
        realm_id,
        &synthetic_event_id,
        "apple",
        format!("apple_{notification_type_str}"),
        serde_json::json!({
            "notificationType": notification_type_str,
            "productId": product_id,
            "originalTransactionId": original_transaction_id,
        }),
    )
    .await;

    record_iap_audit(
        state,
        realm_id,
        AuditAction::IapNotification,
        None,
        Some(ActorType::System),
        original_transaction_id.to_string(),
        serde_json::json!({
            "provider": "apple",
            "notificationType": notification_type_str,
            "productId": product_id,
            "outcome": "expired",
        }),
    )
    .await;

    Ok(())
}

/// DID_FAIL_TO_RENEW — a renewal charge failed. With the GRACE_PERIOD subtype
/// Apple keeps granting access until the grace expiration (carried on the
/// verified renewal info): the period end is stretched to that date while the
/// subscription stays Active. Without grace (BILLING_RETRY) the subscription
/// moves to PastDue with no revoke — mirroring the Stripe payment_failed
/// posture (recovery flows through DID_RENEW, final failure through EXPIRED).
#[allow(clippy::too_many_arguments)]
async fn process_apple_renewal_failure(
    state: &AppState,
    realm_id: &str,
    verifier: &AppleVerifier,
    notification: &herald_infra_iap::apple::models::ResponseBodyV2DecodedPayload,
    original_transaction_id: &str,
    product_id: &str,
) -> Result<(), CoreError> {
    use herald_infra_iap::apple::models::Subtype;

    let notification_type_str = apple_notification_type_str(&notification.notification_type);
    let in_grace = notification.subtype == Some(Subtype::GracePeriod);
    // A billing-retry sequence may fire several times; key on the state
    // transition (grace vs retry) so repeats are deduped but a later
    // transition still lands.
    let outcome = if in_grace {
        "grace_period"
    } else {
        "billing_retry"
    };
    let synthetic_event_id =
        format!("apple:{original_transaction_id}:{notification_type_str}:{outcome}");
    if apple_event_already_processed(state, realm_id, &synthetic_event_id).await? {
        return Ok(());
    }

    let Some(mut subscription) = apple_subscription_for_lifecycle(
        state,
        realm_id,
        original_transaction_id,
        &synthetic_event_id,
        &notification_type_str,
        product_id,
        "no_subscription",
    )
    .await?
    else {
        return Ok(());
    };

    if in_grace {
        // Verified renewal info carries the grace expiration. Absent info or
        // a verification failure leaves the current period untouched (fail
        // closed on data, not on access).
        let signed_renewal_info = notification
            .data
            .as_ref()
            .and_then(|d| d.signed_renewal_info.as_deref());
        let grace_until = match signed_renewal_info {
            Some(jws) => {
                verifier
                    .verify_signed_renewal_info(jws)
                    .map_err(iap_error_to_core_error)?
                    .grace_period_expires_date
            }
            None => None,
        };
        if let Some(grace_until) = grace_until {
            if subscription
                .current_period_end
                .is_none_or(|end| end < grace_until)
            {
                subscription.current_period_end = Some(grace_until);
                subscription.synced_at = Some(Utc::now());
                subscription.updated_at = Utc::now();
                if let Err(e) = state
                    .billing_repository
                    .update_subscription(subscription)
                    .await
                {
                    tracing::warn!(
                        realm_id = %realm_id,
                        original_transaction_id = %original_transaction_id,
                        error = %e,
                        "apple grace period: failed to extend period end"
                    );
                }
            }
            tracing::info!(
                realm_id = %realm_id,
                original_transaction_id = %original_transaction_id,
                grace_until = %grace_until,
                "apple DID_FAIL_TO_RENEW in grace: access extended to grace expiration"
            );
        }
    } else if subscription.status == SubscriptionStatus::Active {
        subscription.status = SubscriptionStatus::PastDue;
        subscription.synced_at = Some(Utc::now());
        subscription.updated_at = Utc::now();
        if let Err(e) = state
            .billing_repository
            .update_subscription(subscription)
            .await
        {
            tracing::warn!(
                realm_id = %realm_id,
                original_transaction_id = %original_transaction_id,
                error = %e,
                "apple billing retry: failed to mark subscription PastDue"
            );
        }
    }

    record_idempotent_payment_event(
        state,
        realm_id,
        &synthetic_event_id,
        "apple",
        format!("apple_{notification_type_str}"),
        serde_json::json!({
            "notificationType": notification_type_str,
            "productId": product_id,
            "originalTransactionId": original_transaction_id,
            "outcome": outcome,
        }),
    )
    .await;

    record_iap_audit(
        state,
        realm_id,
        AuditAction::IapNotification,
        None,
        Some(ActorType::System),
        original_transaction_id.to_string(),
        serde_json::json!({
            "provider": "apple",
            "notificationType": notification_type_str,
            "productId": product_id,
            "outcome": outcome,
        }),
    )
    .await;

    Ok(())
}

/// DID_CHANGE_RENEWAL_STATUS — the user flipped auto-renew off/on. Off →
/// schedule the cancel at the current period end (Stripe's
/// ScheduledCancel posture, access continues); on → clear the schedule.
/// No points action either way.
async fn process_apple_renewal_status_change(
    state: &AppState,
    realm_id: &str,
    verifier: &AppleVerifier,
    notification: &herald_infra_iap::apple::models::ResponseBodyV2DecodedPayload,
    original_transaction_id: &str,
    product_id: &str,
) -> Result<(), CoreError> {
    use herald_infra_iap::apple::models::AutoRenewStatus;

    let notification_type_str = apple_notification_type_str(&notification.notification_type);

    // The auto-renew flag lives on the verified renewal info; without it the
    // notification carries no actionable state.
    let signed_renewal_info = notification
        .data
        .as_ref()
        .and_then(|d| d.signed_renewal_info.as_deref());
    let Some(signed_renewal_info) = signed_renewal_info else {
        tracing::warn!(
            realm_id = %realm_id,
            original_transaction_id = %original_transaction_id,
            "apple DID_CHANGE_RENEWAL_STATUS missing signedRenewalInfo — skipping"
        );
        return Ok(());
    };
    let auto_renew_on = verifier
        .verify_signed_renewal_info(signed_renewal_info)
        .map_err(iap_error_to_core_error)?
        .auto_renew_status
        == Some(AutoRenewStatus::On);

    // Key on the resulting state (on/off), not the notification occurrence:
    // repeated off→off notifications dedupe while a later on flips it back.
    let outcome = if auto_renew_on {
        "auto_renew_on"
    } else {
        "auto_renew_off"
    };
    let synthetic_event_id =
        format!("apple:{original_transaction_id}:{notification_type_str}:{outcome}");
    if apple_event_already_processed(state, realm_id, &synthetic_event_id).await? {
        return Ok(());
    }

    let Some(mut subscription) = apple_subscription_for_lifecycle(
        state,
        realm_id,
        original_transaction_id,
        &synthetic_event_id,
        &notification_type_str,
        product_id,
        "no_subscription",
    )
    .await?
    else {
        return Ok(());
    };

    if auto_renew_on {
        subscription.cancel_at_period_end = false;
        subscription.cancel_at = None;
        if subscription.status == SubscriptionStatus::ScheduledCancel {
            subscription.status = SubscriptionStatus::Active;
        }
    } else {
        subscription.cancel_at_period_end = true;
        subscription.cancel_at = subscription.current_period_end;
    }
    subscription.synced_at = Some(Utc::now());
    subscription.updated_at = Utc::now();
    if let Err(e) = state
        .billing_repository
        .update_subscription(subscription)
        .await
    {
        tracing::warn!(
            realm_id = %realm_id,
            original_transaction_id = %original_transaction_id,
            error = %e,
            "apple DID_CHANGE_RENEWAL_STATUS: failed to update subscription"
        );
    }

    tracing::info!(
        realm_id = %realm_id,
        original_transaction_id = %original_transaction_id,
        auto_renew_on,
        "apple DID_CHANGE_RENEWAL_STATUS: subscription cancel schedule updated"
    );

    record_idempotent_payment_event(
        state,
        realm_id,
        &synthetic_event_id,
        "apple",
        format!("apple_{notification_type_str}"),
        serde_json::json!({
            "notificationType": notification_type_str,
            "productId": product_id,
            "originalTransactionId": original_transaction_id,
            "outcome": outcome,
        }),
    )
    .await;

    record_iap_audit(
        state,
        realm_id,
        AuditAction::IapNotification,
        None,
        Some(ActorType::System),
        original_transaction_id.to_string(),
        serde_json::json!({
            "provider": "apple",
            "notificationType": notification_type_str,
            "productId": product_id,
            "outcome": outcome,
        }),
    )
    .await;

    Ok(())
}

// ============================================================================
// ============================================================================
//
// The signatures below are the frozen contract that `compensation.rs` (via
// `WebhookEventProcessorImpl::reprocess_event`) and the worker
// skeleton with the full "lookup + replay" implementation; the argument list
// and return type MUST NOT change.
//
// replay provider event *streams* (events API). Apple/Google replay *current
// state* fetched from the provider — the job constructs a synthetic payload
// from the provider API response and hands it here. The same
// `payment_event`-based idempotency guards dedup; the domain handling reuses
// `fulfill_provider_event`, `sync_subscription`).

/// **SIGNATURE CONTRACT**: `compensation.rs` and the worker reconciliation job
/// the full "lookup + replay" implementation; the signature MUST NOT change.
///
/// The worker job hands a payload of shape `{ "signedPayload": <JWS> }`
/// (the raw JWS Apple attempted to deliver, pulled from
/// `getNotificationHistory`). We verify + decode + fulfil it through the exact
/// same path as a live Apple SSV V2 webhook (`process_apple_notification`),
/// which is already idempotent on `originalTransactionId`. This keeps
/// compensation byte-for-byte consistent with the live notification path
pub async fn reprocess_apple_event(
    state: AppState,
    realm_id: String,
    payload: Value,
    _event_type: String,
) -> Result<(), CoreError> {
    let signed_payload = payload
        .get("signedPayload")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            CoreError::BadRequest("apple reprocess payload missing signedPayload field".to_string())
        })?;

    // Delegate to the live-notification domain path. process_apple_notification
    // already: loads Apple creds, verifies the JWS, resolves the entitlement
    // mapping (fail loud on no_mapping), checks payment_event idempotency on
    // originalTransactionId, and fulfils via fulfill_provider_event. A
    // duplicate replay (event already processed) short-circuits inside it
    // without error.
    process_apple_notification(&state, &realm_id, signed_payload).await
}

/// internal implementation; the signature is frozen here.
///
/// The worker job hands a payload of shape
/// `{ "purchaseToken": ..., "subscriptionState": ..., "heraldStatus": ...,
///    "previousStatus": ..., "productId": ..., "expiryTime": ... }`
/// (subscription lifecycle) or
/// `{ "purchaseToken": ..., "purchaseType": ..., "voidedTimeMillis": ...,
///    "orderId": ... }` (voided purchase refund). Both paths are idempotent on
/// the `purchaseToken` (the external_event_id). The idempotency check is
/// built on `payment_event` to stay symmetric with Stripe/Creem/Apple.
pub async fn reprocess_google_event(
    state: AppState,
    realm_id: String,
    payload: Value,
    event_type: String,
) -> Result<(), CoreError> {
    let purchase_token = payload
        .get("purchaseToken")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            CoreError::BadRequest("google reprocess payload missing purchaseToken".to_string())
        })?;

    // Idempotency: a payment_event keyed by purchaseToken + provider means the
    // job already replayed this token for this event_type. Skip without error
    // (symmetric with the Stripe/Creem compensation idempotency guard). Note
    // that the event_type is part of the synthetic event id so a state
    // transition (renewed → expired) is NOT incorrectly deduped against the
    // earlier renewed replay.
    let synthetic_event_id = format!("google:{purchase_token}:{event_type}");
    if state
        .billing_repository
        .find_payment_event_by_external_id(&realm_id, &synthetic_event_id, "google")
        .await?
        .is_some()
    {
        tracing::info!(
            realm_id = %realm_id,
            event_type = %event_type,
            "google reprocess: event already processed, skipping"
        );
        return Ok(());
    }

    // Record the synthetic payment_event up-front (processed=false) so a
    // concurrent worker / webhook replay can't double-process. On success we
    // flip processed=true below.
    let saved_event = match state
        .billing_repository
        .create_payment_event(PaymentEvent {
            id: Uuid::now_v7(),
            realm_id: realm_id.clone(),
            external_event_id: synthetic_event_id.clone(),
            payment_provider: "google".to_string(),
            event_type: event_type.clone(),
            subscription_id: None,
            payload: payload.clone(),
            processed: false,
            processing_started_at: Some(Utc::now()),
            created_at: Utc::now(),
        })
        .await
    {
        Ok(event) => event,
        Err(CoreError::DatabaseError(ref msg))
            if msg.contains("unique constraint") || msg.contains("duplicate key") =>
        {
            // Concurrent replay already inserted this event — treat as already
            // handled.
            tracing::info!(
                realm_id = %realm_id,
                event_type = %event_type,
                "google reprocess: concurrent insert detected, event already handled"
            );
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    // Domain replay. For a state transition the mapped heraldStatus lives on
    // the payload; for a refund we transition to the Canceled state ("refund
    // does not change subscription status, only triggers points clawback" —
    // here we record the lifecycle change; the clawback itself
    // runs via the points service when the subscription reaches a terminal
    // state).
    let herald_status = payload
        .get("heraldStatus")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| match event_type.as_str() {
            "subscription.renewed" => "active".to_string(),
            "subscription.expired" => "expired".to_string(),
            "subscription.past_due" => "past_due".to_string(),
            "subscription.refund" => "canceled".to_string(),
            _ => "active".to_string(),
        });
    let new_status = herald_status
        .parse::<SubscriptionStatus>()
        .unwrap_or(SubscriptionStatus::Active);

    let product_id = payload
        .get("productId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let expiry_time = payload
        .get("expiryTime")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    // Resolve the entitlement mapping so we know the bucket_id / entitlement_key;
    // the admin must fix the mapping, and the next sweep will retry.
    let resolved =
        resolve_entitlement_mapping(&state, &realm_id, "google", product_id, None, None).await?;

    // mapping's billing_type is the fulfillment routing authority. OneTime
    // purchases (consumable points packs and buyouts) never create a
    // subscription row, so they are handled by an attempt-keyed revoke.
    // NonRenewing creates a subscription row and is expired in place. Recurring
    // continues to use the subscription sync path (unchanged).
    let mapping_billing_type = resolved.mapping.billing_type.clone();
    match mapping_billing_type {
        Some(BillingType::OneTime) => {
            reprocess_google_one_time_revoke(&state, &realm_id, purchase_token, &event_type)
                .await?;
        }
        Some(BillingType::NonRenewing) => {
            reprocess_google_non_renewing_revoke(
                &state,
                &realm_id,
                purchase_token,
                &event_type,
                &resolved.entitlement_key,
            )
            .await?;
        }
        _ => {
            // Recurring (or mapping with no billing_type — falls back to the
            // recurring subscription sync path for backwards compatibility).
            reprocess_google_recurring_sync(
                &state,
                &realm_id,
                purchase_token,
                product_id,
                &resolved.mapping,
                &event_type,
                new_status,
                expiry_time,
                payload.clone(),
            )
            .await?;
        }
    }

    // Mark the synthetic payment_event processed so the next sweep dedups it.
    if let Err(e) = state
        .billing_repository
        .mark_payment_event_processed(saved_event.id)
        .await
    {
        tracing::error!(
            realm_id = %realm_id,
            event_type = %event_type,
            error = %e,
            "google reprocess: sync succeeded but failed to mark payment_event processed — may reprocess next sweep"
        );
    }

    record_iap_audit(
        &state,
        &realm_id,
        AuditAction::IapNotification,
        None,
        Some(ActorType::System),
        purchase_token.to_string(),
        serde_json::json!({
            "provider": "google",
            "eventType": event_type,
            "productId": product_id,
            "outcome": "reconciled",
        }),
    )
    .await;

    Ok(())
}

/// Google one-time revocation (voided purchase / refund): revoke topup credits
/// and permanent payment roles keyed on the originating payment_attempt
/// ledger is a no-op, not an error.
async fn reprocess_google_one_time_revoke(
    state: &AppState,
    realm_id: &str,
    purchase_token: &str,
    event_type: &str,
) -> Result<(), CoreError> {
    // The Google purchaseToken is the provider_reference written at
    // create_iap_payment_attempt time.
    let attempt = state
        .payment_attempt_service
        .get_payment_attempt_by_provider_reference("google", purchase_token)
        .await
        .map_err(|e| {
            CoreError::InternalServerError(format!(
                "google one-time revoke: attempt lookup failed: {e}"
            ))
        })?;
    let attempt = match attempt {
        Some(a) if a.realm_id == realm_id => a,
        // The provider-reference lookup is realm-free; an event verified for
        // this realm must not revoke another realm's attempt.
        Some(a) => {
            tracing::warn!(
                realm_id = %realm_id,
                attempt_id = %a.id,
                attempt_realm_id = %a.realm_id,
                event_type = %event_type,
                "google one-time revoke: attempt belongs to a different realm — skipping"
            );
            return Ok(());
        }
        None => {
            tracing::warn!(
                realm_id = %realm_id,
                event_type = %event_type,
                "google one-time revoke: no payment_attempt found — nothing to revoke"
            );
            return Ok(());
        }
    };

    // Revoke topup credits (source_id = attempt.id). Best-effort.
    for bucket_id in crate::webhook_common::captured_bucket_ids(state, &attempt).await? {
        if let Err(e) = state
            .points_service
            .revoke_points_by_source_id(
                realm_id,
                attempt.user_id,
                bucket_id,
                &attempt.id.to_string(),
                herald_core::domain::points::entities::RevocationType::RefundRevoke,
                format!(
                    "Google {event_type} voided/refund for attempt {}",
                    attempt.id
                ),
            )
            .await
        {
            tracing::warn!(
                realm_id = %realm_id,
                attempt_id = %attempt.id,
                bucket_id = %bucket_id,
                error = %e,
                "google one-time revoke: points revoke failed (best-effort)"
            );
        }
    }

    crate::webhook_common::revoke_payment_roles_for_source(
        state,
        realm_id,
        attempt.user_id,
        &attempt.id.to_string(),
    )
    .await;

    Ok(())
}

/// Google non-renewing revocation (EXPIRED from polling, or voided/refund):
/// set the subscription to Expired and revoke its payment roles. Idempotent.
async fn reprocess_google_non_renewing_revoke(
    state: &AppState,
    realm_id: &str,
    purchase_token: &str,
    event_type: &str,
    entitlement_key: &str,
) -> Result<(), CoreError> {
    let subscription = state
        .billing_repository
        .find_by_external_subscription_id(purchase_token, "google")
        .await?;
    let subscription = match subscription {
        Some(s) => s,
        None => {
            tracing::warn!(
                realm_id = %realm_id,
                event_type = %event_type,
                entitlement_key = %entitlement_key,
                "google non-renewing revoke: no subscription found — nothing to expire/revoke"
            );
            return Ok(());
        }
    };

    // Capture identity before the subscription may be moved into update_subscription.
    let subscription_id = subscription.id;
    let subscription_user_id = subscription.user_id;
    if subscription.status != SubscriptionStatus::Expired {
        let mut sub = subscription;
        sub.status = SubscriptionStatus::Expired;
        sub.synced_at = Some(Utc::now());
        sub.updated_at = Utc::now();
        if let Err(e) = state.billing_repository.update_subscription(sub).await {
            tracing::warn!(
                realm_id = %realm_id,
                error = %e,
                "google non-renewing revoke: failed to set subscription Expired (best-effort)"
            );
        }
    } else {
        tracing::info!(
            realm_id = %realm_id,
            subscription_id = %subscription_id,
            "google non-renewing revoke: subscription already Expired"
        );
    }

    // Revoke the subscription's payment roles (source_id = subscription.id).
    crate::webhook_common::revoke_payment_roles_for_source(
        state,
        realm_id,
        subscription_user_id,
        &subscription_id.to_string(),
    )
    .await;

    Ok(())
}

/// Google recurring state transition: maintain the existing sync_subscription
/// Pulled out of the main `reprocess_google_event` body so the per-billing-type
/// routing reads top-down; the sync call itself is byte-equivalent to the
#[allow(clippy::too_many_arguments)]
async fn reprocess_google_recurring_sync(
    state: &AppState,
    realm_id: &str,
    purchase_token: &str,
    product_id: &str,
    mapping: &EntitlementMapping,
    event_type: &str,
    new_status: SubscriptionStatus,
    expiry_time: Option<DateTime<Utc>>,
    payload: Value,
) -> Result<(), CoreError> {
    // Compute the cancel flags before moving `new_status` into the input
    // struct (SubscriptionStatus is not Copy).
    let is_cancelled = matches!(new_status, SubscriptionStatus::Canceled);
    let is_scheduled_cancel = matches!(
        new_status,
        SubscriptionStatus::ScheduledCancel | SubscriptionStatus::Canceled
    );

    let synced = sync_subscription(
        state,
        SyncSubscriptionInput {
            provider: "google",
            realm_id: realm_id.to_string(),
            user_id: None,
            external_subscription_id: purchase_token.to_string(),
            external_product_id: product_id.to_string(),
            client_app_id: None,
            entitlement_key: mapping.entitlement_key.clone(),
            external_price_id: None,
            provider_metadata: Some(payload),
            status: new_status,
            current_period_start: None,
            current_period_end: expiry_time,
            cancel_at_period_end: is_scheduled_cancel,
            cancel_at: if is_cancelled { Some(Utc::now()) } else { None },
            existing_subscription: None,
        },
    )
    .await?;

    let Some((subscription, previous)) = synced else {
        return Ok(());
    };

    if matches!(
        subscription.status,
        SubscriptionStatus::Canceled | SubscriptionStatus::Expired
    ) {
        state
            .subscription_service
            .handle_subscription_cancel(
                subscription.user_id,
                realm_id,
                subscription.id,
                herald_core::domain::points::subscription_service::CancelMode::ImmediateCancel,
                subscription.current_period_end,
                Some(&subscription.entitlement_key),
            )
            .await?;
    } else if event_type == "subscription.renewed" && mapping.enabled {
        let period_end = subscription.current_period_end.ok_or_else(|| {
            CoreError::BadRequest(
                "google renewal is missing the subscription period end".to_string(),
            )
        })?;
        let period_start = previous
            .as_ref()
            .and_then(|prior| prior.current_period_end)
            .or(subscription.current_period_start)
            .unwrap_or_else(Utc::now);
        if period_start < period_end {
            state
                .subscription_service
                .handle_subscription_paid(
                    subscription.user_id,
                    subscription.id,
                    realm_id,
                    mapping,
                    true,
                    period_start,
                    period_end,
                    format!("google:{purchase_token}:{event_type}"),
                )
                .await?;
        } else {
            tracing::warn!(
                %realm_id,
                subscription_id = %subscription.id,
                %period_start,
                %period_end,
                "google renewal has no advancing period; renewal grant skipped"
            );
        }
    }
    Ok(())
}

// ============================================================================
// Error mapping helpers
// ============================================================================

fn iap_error_to_api_error(e: IapError) -> ApiError {
    match e {
        IapError::NotConfigured { .. } => ApiError::not_found(e.to_string()),
        IapError::OwnershipMismatch { .. } => ApiError::conflict("ownership_mismatch".to_string()),
        IapError::AppleVerification(_) => {
            ApiError::unprocessable_entity("verification_failed".to_string())
        }
        IapError::GoogleApi { .. } => {
            ApiError::unprocessable_entity("verification_failed".to_string())
        }
        IapError::AlreadyConsumed => ApiError::unprocessable_entity("already_consumed".to_string()),
        IapError::ServiceAccountAuth(_) | IapError::Transport(_) | IapError::Json(_) => {
            ApiError::internal(e.to_string())
        }
    }
}

/// Map `IapError` to a `CoreError` for the webhook path (which returns 200 to
/// Apple regardless; this is for internal diagnostics / fail-loud skips).
fn iap_error_to_core_error(e: IapError) -> CoreError {
    match e {
        IapError::NotConfigured { .. } => CoreError::NotFound,
        IapError::OwnershipMismatch { user_id } => {
            CoreError::Conflict(format!("ownership_mismatch: {user_id}"))
        }
        IapError::AppleVerification(_) | IapError::GoogleApi { .. } | IapError::AlreadyConsumed => {
            CoreError::BadRequest(e.to_string())
        }
        IapError::ServiceAccountAuth(_) | IapError::Transport(_) | IapError::Json(_) => {
            CoreError::InternalServerError(e.to_string())
        }
    }
}

/// pure function so the routing rule is unit-testable without standing up the
/// async `GoogleDeveloperClient` mock fixture. A one-time mapping that
/// configures an enabled fixed grant rule is a consumable points pack and
/// must be consumed so it can be re-purchased; a points-less one-time mapping
/// (buyout / non-consumable) must be acknowledged only so "restore purchases"
/// still sees the entitlement and Google does not auto-refund after 3 days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoogleOneTimeAckAction {
    /// Consumable points pack — `purchases.products.consume`.
    Consume,
    /// Buyout / non-consumable — `purchases.products.acknowledge`.
    Acknowledge,
}

fn google_one_time_ack_action(is_consumable_points_pack: bool) -> GoogleOneTimeAckAction {
    if is_consumable_points_pack {
        GoogleOneTimeAckAction::Consume
    } else {
        GoogleOneTimeAckAction::Acknowledge
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use herald_infra_iap::apple::models::ProductType;

    // Google purchase is consumed (points pack) or merely acknowledged
    // (buyout). Reversing this would either block re-purchase of points packs
    #[test]
    fn google_one_time_ack_action_consumes_when_points_configured() {
        // Points pack (consumable): must consume so it can be re-purchased.
        assert_eq!(
            google_one_time_ack_action(true),
            GoogleOneTimeAckAction::Consume
        );
        assert_eq!(
            google_one_time_ack_action(true),
            GoogleOneTimeAckAction::Consume
        );
    }

    #[test]
    fn google_one_time_ack_action_acknowledges_when_no_points() {
        // Buyout / non-consumable: acknowledge only.
        assert_eq!(
            google_one_time_ack_action(false),
            GoogleOneTimeAckAction::Acknowledge
        );
        assert_eq!(
            google_one_time_ack_action(false),
            GoogleOneTimeAckAction::Acknowledge
        );
    }

    // is a *diagnostic only* (mismatch does not block fulfillment), but getting
    // the alignment table wrong would either suppress a real config-error
    // signal or spam false-positive warns.
    #[test]
    fn apple_txn_type_matches_billing_type_aligned_pairs() {
        assert!(apple_txn_type_matches_billing_type(
            &ProductType::AutoRenewableSubscription,
            &BillingType::Recurring
        ));
        assert!(apple_txn_type_matches_billing_type(
            &ProductType::NonRenewingSubscription,
            &BillingType::NonRenewing
        ));
        assert!(apple_txn_type_matches_billing_type(
            &ProductType::NonConsumable,
            &BillingType::OneTime
        ));
        assert!(apple_txn_type_matches_billing_type(
            &ProductType::Consumable,
            &BillingType::OneTime
        ));
    }

    #[test]
    fn apple_txn_type_matches_billing_type_mismatched_pairs() {
        // Non-consumable against a recurring mapping is a config error
        // (non-consumable products are not auto-renewable).
        assert!(!apple_txn_type_matches_billing_type(
            &ProductType::NonConsumable,
            &BillingType::Recurring
        ));
        // Auto-renewable against a one_time mapping is a config error.
        assert!(!apple_txn_type_matches_billing_type(
            &ProductType::AutoRenewableSubscription,
            &BillingType::OneTime
        ));
        // Non-renewing subscription against recurring mapping is a config error.
        assert!(!apple_txn_type_matches_billing_type(
            &ProductType::NonRenewingSubscription,
            &BillingType::Recurring
        ));
    }
}
