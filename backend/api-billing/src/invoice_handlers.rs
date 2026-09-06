use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::Response,
};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use herald_api_base::application::http::common::auth_utils::{
    require_authenticated_user_in_realm_with_token, require_token_scope,
};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialScope, Identity, TokenCredentialContext};
use herald_core::domain::billing::credit_note::{
    CreditNoteRepository, CreditNoteStatus, NewCreditNote,
};
use herald_core::domain::billing::entities::SubscriptionStatus;
use herald_core::domain::billing::invoice::{
    ActorType, AttributionFilter, InvoiceDetail, InvoiceListFilters, InvoicePdfGenerator,
    InvoiceProvider, InvoiceRepository, InvoiceSource, InvoiceStatus, InvoiceStatusTransition,
    NewInvoice, NewLineItem, UpdateInvoiceDraft,
};
use herald_core::domain::billing::invoice_service;
use herald_core::domain::billing::invoice_service::{
    InvoicePolicyConfig, external_invoice_capability_enabled, parse_invoice_policy_config,
    validate_external_invoice_readonly, validate_invoice_policy_allows_creation,
    validate_not_mor_provider, validate_pdf_allowed_by_policy, validate_status_transition,
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::payment_attempt::PaymentAttemptStatus;
use herald_core::domain::realm_config::RealmConfigRepository;
use herald_core::infrastructure::billing::IronPressInvoicePdfGenerator;

use crate::handlers::require_billing_permission;
use crate::invoice_eligibility::determine_invoice_apply_route;
use crate::invoice_types::*;

/// Extract the user ID from an authenticated user identity.
fn actor_user_id_from_identity(identity: &Identity) -> Option<Uuid> {
    if identity.is_user() {
        Uuid::parse_str(&identity.user_id()).ok()
    } else {
        None
    }
}

/// Helper: load invoice detail and return 404 if not found.
async fn load_detail(
    state: &AppState,
    realm_id: &str,
    invoice_id: Uuid,
) -> Result<herald_core::domain::billing::invoice::InvoiceDetail, ApiError> {
    state
        .invoice_repository
        .find_with_items(realm_id, invoice_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Invoice not found"))
}

async fn validate_account_in_realm(
    pool: &PgPool,
    account_id: Uuid,
    realm_id: &str,
) -> Result<(), ApiError> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM account WHERE id = $1 AND realm_id = $2)")
            .bind(account_id)
            .bind(realm_id)
            .fetch_one(pool)
            .await
            .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    if !exists {
        return Err(ApiError::bad_request(format!(
            "Account {} does not exist in this realm",
            account_id
        )));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum OwnedResource {
    PaymentAttempt,
    Subscription,
}

impl OwnedResource {
    fn table_name(&self) -> &'static str {
        match self {
            Self::PaymentAttempt => "payment_attempts",
            Self::Subscription => "subscription",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::PaymentAttempt => "payment attempt",
            Self::Subscription => "subscription",
        }
    }
}

async fn validate_resource_ownership(
    pool: &PgPool,
    resource: OwnedResource,
    resource_id: Uuid,
    user_id: Uuid,
    realm_id: &str,
) -> Result<(), ApiError> {
    let query = format!(
        "SELECT user_id FROM {} WHERE id = $1 AND realm_id = $2",
        resource.table_name()
    );
    let owner: Option<Option<Uuid>> = sqlx::query_scalar(&query)
        .bind(resource_id)
        .bind(realm_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    match owner {
        Some(Some(uid)) if uid == user_id => Ok(()),
        Some(_) => Err(ApiError::forbidden(format!(
            "You can only apply for invoices for your own {}s",
            resource.label()
        ))),
        None => Err(ApiError::bad_request(format!(
            "{} {} not found",
            resource.label(),
            resource_id
        ))),
    }
}

/// Whether the referenced purchase has reached a paid state.
///
/// A payment attempt is paid only after the provider-confirmed terminal
/// success state (`Succeeded`). A subscription can be invoiced once it has
/// left the unpaid setup/trial states; canceled/expired subscriptions remain
/// historical paid purchases and therefore stay invoiceable.
async fn resource_is_paid(
    pool: &PgPool,
    resource: OwnedResource,
    resource_id: Uuid,
    realm_id: &str,
) -> Result<bool, ApiError> {
    let status: Option<String> = match resource {
        OwnedResource::PaymentAttempt => sqlx::query_scalar(
            "SELECT status FROM payment_attempts WHERE id = $1 AND realm_id = $2",
        )
        .bind(resource_id)
        .bind(realm_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?,
        OwnedResource::Subscription => {
            sqlx::query_scalar("SELECT status FROM subscription WHERE id = $1 AND realm_id = $2")
                .bind(resource_id)
                .bind(realm_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
        }
    };

    Ok(match resource {
        OwnedResource::PaymentAttempt => matches!(
            status
                .as_deref()
                .and_then(|s| s.parse::<PaymentAttemptStatus>().ok()),
            Some(PaymentAttemptStatus::Succeeded)
        ),
        OwnedResource::Subscription => matches!(
            status
                .as_deref()
                .and_then(|s| s.parse::<SubscriptionStatus>().ok()),
            Some(
                SubscriptionStatus::Active
                    | SubscriptionStatus::ScheduledCancel
                    | SubscriptionStatus::Canceled
                    | SubscriptionStatus::Expired
                    | SubscriptionStatus::Paused
                    | SubscriptionStatus::PastDue
                    | SubscriptionStatus::Dispute
            )
        ),
    })
}

async fn validate_resource_paid(
    pool: &PgPool,
    resource: OwnedResource,
    resource_id: Uuid,
    realm_id: &str,
) -> Result<(), ApiError> {
    if !resource_is_paid(pool, resource, resource_id, realm_id).await? {
        return Err(ApiError::bad_request(format!(
            "Only paid {}s can be invoiced",
            resource.label()
        )));
    }
    Ok(())
}

/// Whether an `external_sync` invoice already covers the given resource.
/// Shared by the creation policy (write path) and the apply-eligibility read
/// path so the two judgments cannot drift. No repository method exists for
/// this lookup; this is the minimal SQL. Columns confirmed against migration
/// 20260508_invoice.sql (`source` is TEXT CHECK in
/// {'admin_manual','user_application','external_sync'}).
async fn external_sync_invoice_exists(
    state: &AppState,
    realm_id: &str,
    payment_attempt_id: Option<Uuid>,
    subscription_id: Option<Uuid>,
) -> Result<bool, ApiError> {
    if let Some(pa_id) = payment_attempt_id {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM invoice
                 WHERE realm_id = $1 AND source = 'external_sync' AND payment_attempt_id = $2)",
        )
        .bind(realm_id)
        .bind(pa_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))
    } else if let Some(sub_id) = subscription_id {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM invoice
                 WHERE realm_id = $1 AND source = 'external_sync' AND subscription_id = $2)",
        )
        .bind(realm_id)
        .bind(sub_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))
    } else {
        Ok(false)
    }
}

/// Check Creem MoR guard and invoice policy for manual invoice creation.
///
/// Queries the payment_provider from the payment_attempt (if present) to reject
/// Creem-managed transactions, then checks the realm's invoice policy.
async fn validate_invoice_creation_policy(
    state: &AppState,
    realm_id: &str,
    payment_attempt_id: Option<Uuid>,
    subscription_id: Option<Uuid>,
) -> Result<(), ApiError> {
    let mut payment_provider: Option<String> = if let Some(pa_id) = payment_attempt_id {
        sqlx::query_scalar(
            "SELECT payment_provider FROM payment_attempts WHERE id = $1 AND realm_id = $2",
        )
        .bind(pa_id)
        .bind(realm_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
        .flatten()
    } else {
        None
    };

    if payment_provider.is_none()
        && let Some(sub_id) = subscription_id
    {
        payment_provider = sqlx::query_scalar(
            "SELECT payment_provider FROM subscription WHERE id = $1 AND realm_id = $2",
        )
        .bind(sub_id)
        .bind(realm_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
        .flatten();
    }

    validate_not_mor_provider(payment_provider.as_deref()).map_err(|error| {
        ApiError::with_error_code(
            StatusCode::BAD_REQUEST,
            "mor_provider_invoice_blocked",
            error.to_string(),
        )
    })?;

    let policy_config = get_invoice_policy(state, realm_id).await?;
    validate_invoice_policy_allows_creation(&policy_config)?;

    // Keep the write path consistent with the eligibility read path
    // (invoice.md §4.1 behavior matrix / `determine_invoice_apply_route`):
    // under provider_first a Stripe resource's invoices arrive via webhook
    // (external_provider verdict), and a resource that already carries an
    // externally-synced invoice must not get a duplicate manual one. A
    // provider whose external-invoice capability is switched OFF degrades to
    // manual fallback and stays writable (§4.3).
    if policy_config.policy == "provider_first"
        && payment_provider.as_deref() == Some("stripe")
        && external_invoice_capability_enabled(&policy_config, "stripe")
    {
        return Err(ApiError::conflict(
            "Invoices for Stripe resources are issued by Stripe under the provider_first policy",
        ));
    }

    if external_sync_invoice_exists(state, realm_id, payment_attempt_id, subscription_id).await? {
        return Err(ApiError::conflict(
            "An externally-synced invoice already exists for this resource",
        ));
    }

    Ok(())
}

/// Load the invoice policy config for a realm.
///
/// Queries `realm_config` for the `invoice_policy` / `policy` row via RealmConfigRepository.
/// Returns a default "provider_first" config when no row exists.
///
/// Shared at `pub(crate)` visibility so realm-level eligibility evaluation
/// (`crate::invoice_eligibility`) reuses the same policy-reading logic instead
/// of duplicating the SQL/realm_config read.
pub(crate) async fn get_invoice_policy(
    state: &AppState,
    realm_id: &str,
) -> Result<InvoicePolicyConfig, ApiError> {
    let config = state
        .realm_config_repository
        .get(
            realm_id.to_string(),
            "invoice_policy".to_string(),
            "policy".to_string(),
        )
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    match config {
        Some(rc) if rc.enabled => {
            parse_invoice_policy_config(&rc.config_value).map_err(ApiError::from)
        }
        _ => Ok(InvoicePolicyConfig {
            policy: "provider_first".to_string(),
            provider_capabilities: serde_json::Value::Object(serde_json::Map::new()),
        }),
    }
}

/// Policy gate for the manual invoice write endpoints (update/issue/void/
/// mark_paid): they share the policy=none "creation disabled" gate with
/// create, so the check lives in one place if the lifecycles ever diverge.
async fn require_invoice_policy_allows_writes(
    state: &AppState,
    realm_id: &str,
) -> Result<(), ApiError> {
    validate_invoice_policy_allows_creation(&get_invoice_policy(state, realm_id).await?)?;
    Ok(())
}

/// Read-path policy filter for the list endpoints: fetches the realm's
/// invoice policy and delegates the visible-set mapping to
/// `invoice_service::apply_invoice_policy_list_filter`.
async fn apply_invoice_policy_list_filter(
    state: &AppState,
    realm_id: &str,
    filters: &mut InvoiceListFilters,
) -> Result<(), ApiError> {
    let policy = get_invoice_policy(state, realm_id).await?;
    invoice_service::apply_invoice_policy_list_filter(&policy, filters);
    Ok(())
}

fn validate_optional_non_blank(value: Option<&str>, field_name: &str) -> Result<(), ApiError> {
    if value.is_some_and(|s| s.trim().is_empty()) {
        return Err(ApiError::bad_request(format!(
            "{} must not be blank",
            field_name
        )));
    }
    Ok(())
}

fn parse_optional_paid_at(
    value: Option<&str>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, ApiError> {
    value
        .map(|paid_at| {
            chrono::DateTime::parse_from_rfc3339(paid_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|_| ApiError::bad_request("paidAt must be a valid RFC3339 timestamp"))
        })
        .transpose()
}

#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/invoice-seller-config",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Seller config found", body = SellerConfigResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Seller config not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_seller_config(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Result<Json<SellerConfigResponse>, ApiError> {
    tracing::info!("Getting seller config for realm: {}", realm_id);
    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let config = state
        .invoice_repository
        .find_seller_config(&realm_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Seller config not found for this realm"))?;

    Ok(Json(SellerConfigResponse::from(config)))
}

#[utoipa::path(
    put,
    path = "/api/bill/{realmId}/invoice-seller-config",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = SellerConfigRequest,
    responses(
        (status = 200, description = "Seller config saved", body = SellerConfigResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn upsert_seller_config(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Json(request): Json<SellerConfigRequest>,
) -> Result<Json<SellerConfigResponse>, ApiError> {
    tracing::info!("Upserting seller config for realm: {}", realm_id);
    require_billing_permission(&state, &identity, &realm_id, "manage").await?;
    request
        .validate()
        .map_err(|e: validator::ValidationErrors| {
            CoreError::BadRequest(format!("Validation failed: {}", e))
        })?;

    // Trim and validate seller_address is non-empty
    if request.seller_address.trim().is_empty() {
        return Err(ApiError::bad_request("seller_address must not be blank"));
    }

    let now = chrono::Utc::now();
    let config = herald_core::domain::billing::invoice::InvoiceSellerConfig {
        realm_id: realm_id.clone(),
        seller_name: request.seller_name,
        seller_address: request.seller_address,
        seller_email: request.seller_email,
        seller_phone: request.seller_phone,
        seller_tax_id: request.seller_tax_id,
        default_payment_terms: request.default_payment_terms,
        created_at: now,
        updated_at: now,
    };

    let saved = state
        .invoice_repository
        .upsert_seller_config(config)
        .await?;
    Ok(Json(SellerConfigResponse::from(saved)))
}

#[utoipa::path(
    post,
    path = "/api/bill/{realmId}/invoices",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = CreateInvoiceRequest,
    responses(
        (status = 201, description = "Invoice created", body = InvoiceDetailResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_invoice(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Json(request): Json<CreateInvoiceRequest>,
) -> Result<(StatusCode, Json<InvoiceDetailResponse>), ApiError> {
    tracing::info!("Creating invoice for realm: {}", realm_id);
    require_billing_permission(&state, &identity, &realm_id, "manage").await?;
    request
        .validate()
        .map_err(|e: validator::ValidationErrors| {
            CoreError::BadRequest(format!("Validation failed: {}", e))
        })?;

    // Trim and validate address fields are non-empty
    if request.billing_address.trim().is_empty() {
        return Err(ApiError::bad_request("billing_address must not be blank"));
    }
    if request.seller_address.trim().is_empty() {
        return Err(ApiError::bad_request("seller_address must not be blank"));
    }

    validate_account_in_realm(&state.pool, request.account_id, &realm_id).await?;
    if let Some(applicant_id) = request.applicant_user_id {
        validate_account_in_realm(&state.pool, applicant_id, &realm_id).await?;
    }

    validate_invoice_creation_policy(
        &state,
        &realm_id,
        request.payment_attempt_id,
        request.subscription_id,
    )
    .await?;

    let line_items: Vec<NewLineItem> = request
        .line_items
        .into_iter()
        .map(|li| NewLineItem {
            name: li.name,
            description: li.description,
            quantity: li.quantity,
            unit_price: li.unit_price,
        })
        .collect();

    let new_invoice = NewInvoice {
        realm_id: realm_id.clone(),
        source: InvoiceSource::AdminManual,
        account_id: request.account_id,
        applicant_user_id: request.applicant_user_id,
        subscription_id: request.subscription_id,
        payment_attempt_id: request.payment_attempt_id,
        currency: request.currency,
        line_items,
        actor_user_id: actor_user_id_from_identity(&identity),
        billing_name: request.billing_name,
        billing_address: request.billing_address,
        billing_email: request.billing_email,
        billing_phone: request.billing_phone,
        billing_tax_id: request.billing_tax_id,
        seller_name: request.seller_name,
        seller_address: request.seller_address,
        seller_email: request.seller_email,
        seller_phone: request.seller_phone,
        seller_tax_id: request.seller_tax_id,
        discount_mode: parse_adjustment_mode(request.discount_mode.as_deref()),
        discount_value: request.discount_value,
        tax_mode: parse_adjustment_mode(request.tax_mode.as_deref()),
        tax_value: request.tax_value,
        shipping_mode: parse_adjustment_mode(request.shipping_mode.as_deref()),
        shipping_value: request.shipping_value,
        due_date: request.due_date,
        payment_terms: request.payment_terms,
        notes: request.notes,
    };

    let invoice = state.invoice_repository.create_invoice(new_invoice).await?;

    let detail = load_detail(&state, &realm_id, invoice.id).await?;

    Ok((
        StatusCode::CREATED,
        Json(invoice_to_detail_response(detail)),
    ))
}

#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/invoices",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        InvoiceListQuery
    ),
    responses(
        (status = 200, description = "Invoices listed", body = InvoiceListResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_invoices(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Query(query): Query<InvoiceListQuery>,
) -> Result<Json<InvoiceListResponse>, ApiError> {
    tracing::info!("Listing invoices for realm: {}", realm_id);
    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let mut filters = query.to_filters();
    apply_invoice_policy_list_filter(&state, &realm_id, &mut filters).await?;

    let result = state
        .invoice_repository
        .list_admin(&realm_id, filters)
        .await?;

    Ok(Json(InvoiceListResponse {
        total: result.total,
        page: result.page,
        page_size: result.page_size,
        data: result.data.into_iter().map(summary_to_response).collect(),
    }))
}

/// Lookback window for "payment without invoice": succeeded attempts older than
/// this are considered out of scope for active anomaly triage. 90 days matches
/// typical billing-investigation cadences without scanning the full history.
const ANOMALY_LOOKBACK_DAYS: i64 = 90;

/// Row shape for the payments-without-invoice SQL (keeps sqlx boilerplate local
/// to this handler; not part of the domain layer). Uses runtime `query_as`
/// (matches the rest of this handler file; the sqlx compile-time macro would
/// require a DATABASE_URL / offline cache that this crate does not set up).
#[derive(sqlx::FromRow)]
struct PaymentWithoutInvoiceRow {
    payment_attempt_id: Uuid,
    payment_provider: String,
    target_type: String,
    amount: i64,
    currency: String,
    completed_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/invoice-attribution/anomalies",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Attribution anomalies", body = AttributionAnomaliesResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - billing.view required", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
/// Admin read-only attribution anomaly discovery.
///
/// Returns two anomaly classes:
/// - `unattributed_invoices`: externally-synced invoices that lack both
///   `subscription_id` and `payment_attempt_id` (reuses `list_admin` with
///   `attribution=Missing`, capped at the first 100 rows).
/// - `payments_without_invoice`: succeeded renewal / one-time payment attempts
///   inside a 90-day window with no invoice linked via
///   `invoice.payment_attempt_id` (anti-join read; no writes).
///
/// Permission reuses `require_billing_permission(..., "view")` — same model as
/// `GET /invoices`. Realm isolation is enforced both by the repository
/// (`realm_id` filter) and the shared middleware.
pub async fn list_attribution_anomalies(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Result<Json<AttributionAnomaliesResponse>, ApiError> {
    tracing::info!("Listing attribution anomalies for realm: {}", realm_id);
    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let unattributed_filters = InvoiceListFilters {
        attribution: Some(AttributionFilter::Missing),
        page: Some(1),
        page_size: Some(100),
        ..Default::default()
    };
    let unattributed_result = state
        .invoice_repository
        .list_admin(&realm_id, unattributed_filters)
        .await?;
    let unattributed_invoices: Vec<InvoiceResponse> = unattributed_result
        .data
        .into_iter()
        .map(summary_to_response)
        .collect();

    // Shapes covered:
    //   - renewal AND one-time: target_type = 'entitlement_mapping', status = 'Succeeded'
    // (The renewal writer uses target_type = 'entitlement_mapping' too — migration
    // 20260609 restricts payment_attempts.target_type to that single value and
    // deletes any legacy 'subscription_entitlement' rows. Checkout attempts
    // start as Pending; only Succeeded rows can be "missing an invoice". The
    // amount>0 CHECK on payment_attempts makes the renewal 0-yuan-cycle skip
    // explicit at the write side, so no extra filter here.)
    //
    // `subscription_id` is intentionally NULL here: `payment_attempts` has no
    // such column (target_id is the entitlement-mapping id, not a subscription).
    // The admin investigates via `payment_attempt_id` + `provider`; a real
    // subscription link, when recoverable, lives on the invoice side. Surfacing
    // NULL is more honest than a fragile provider_reference parse.
    //
    // Anti-join via NOT EXISTS keeps the query index-friendly and avoids
    // duplicating attempt rows when multiple invoices exist.
    let rows: Vec<PaymentWithoutInvoiceRow> = sqlx::query_as::<_, PaymentWithoutInvoiceRow>(
        r#"
        SELECT
            pa.id AS payment_attempt_id,
            pa.payment_provider,
            pa.target_type,
            pa.amount,
            pa.currency,
            pa.completed_at
        FROM payment_attempts pa
        WHERE pa.realm_id = $1
          AND pa.status = 'Succeeded'
          AND pa.target_type = 'entitlement_mapping'
          AND pa.completed_at >= NOW() - ($2 || ' days')::interval
          AND NOT EXISTS (
              SELECT 1 FROM invoice inv
              WHERE inv.realm_id = $1
                AND inv.payment_attempt_id = pa.id
          )
        ORDER BY pa.completed_at DESC
        LIMIT 200
        "#,
    )
    .bind(realm_id)
    .bind(ANOMALY_LOOKBACK_DAYS)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    let payments_without_invoice = rows
        .into_iter()
        .map(|r| PaymentWithoutInvoiceResponse {
            payment_attempt_id: r.payment_attempt_id,
            // payment_attempts has no subscription_id column; see note above.
            subscription_id: None,
            provider: r.payment_provider,
            target_type: r.target_type,
            amount: r.amount,
            currency: r.currency,
            completed_at: r.completed_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(AttributionAnomaliesResponse {
        unattributed_invoices,
        payments_without_invoice,
    }))
}

#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/invoices/{invoiceId}",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("invoiceId" = Uuid, Path, description = "Invoice ID")
    ),
    responses(
        (status = 200, description = "Invoice detail", body = InvoiceDetailResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Invoice not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_invoice(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, invoice_id)): Path<(String, Uuid)>,
) -> Result<Json<InvoiceDetailResponse>, ApiError> {
    tracing::info!("Getting invoice {} for realm: {}", invoice_id, realm_id);
    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;
    let credit_notes = state
        .credit_note_repository
        .find_by_invoice_id(&realm_id, invoice_id)
        .await?;

    Ok(Json(invoice_to_detail_response_with_credits(
        detail,
        credit_notes,
    )))
}

#[utoipa::path(
    patch,
    path = "/api/bill/{realmId}/invoices/{invoiceId}",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("invoiceId" = Uuid, Path, description = "Invoice ID")
    ),
    request_body = UpdateInvoiceRequest,
    responses(
        (status = 200, description = "Invoice updated", body = InvoiceDetailResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Invoice not found", body = ErrorResponse),
        (status = 409, description = "Conflict - invoice not in draft status", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_invoice(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, invoice_id)): Path<(String, Uuid)>,
    Json(request): Json<UpdateInvoiceRequest>,
) -> Result<Json<InvoiceDetailResponse>, ApiError> {
    tracing::info!("Updating invoice {} for realm: {}", invoice_id, realm_id);
    require_billing_permission(&state, &identity, &realm_id, "manage").await?;
    require_invoice_policy_allows_writes(&state, &realm_id).await?;
    request
        .validate()
        .map_err(|e: validator::ValidationErrors| {
            CoreError::BadRequest(format!("Validation failed: {}", e))
        })?;
    validate_optional_non_blank(request.billing_tax_id.as_deref(), "billing_tax_id")?;
    validate_optional_non_blank(request.seller_tax_id.as_deref(), "seller_tax_id")?;

    // Provider readonly guard: only manual invoices can be updated
    {
        let detail = load_detail(&state, &realm_id, invoice_id).await?;
        validate_external_invoice_readonly(detail.invoice.provider)?;
    }

    let line_items = request.line_items.map(|items| {
        items
            .into_iter()
            .map(|li| NewLineItem {
                name: li.name,
                description: li.description,
                quantity: li.quantity,
                unit_price: li.unit_price,
            })
            .collect()
    });

    let update = UpdateInvoiceDraft {
        realm_id: realm_id.clone(),
        invoice_id,
        actor_user_id: actor_user_id_from_identity(&identity),
        billing_name: request.billing_name,
        billing_address: request.billing_address,
        billing_email: request.billing_email,
        billing_phone: request.billing_phone,
        billing_tax_id: request.billing_tax_id,
        seller_name: request.seller_name,
        seller_address: request.seller_address,
        seller_email: request.seller_email,
        seller_phone: request.seller_phone,
        seller_tax_id: request.seller_tax_id,
        line_items,
        discount_mode: parse_adjustment_mode(request.discount_mode.as_deref()),
        discount_value: request.discount_value,
        tax_mode: parse_adjustment_mode(request.tax_mode.as_deref()),
        tax_value: request.tax_value,
        shipping_mode: parse_adjustment_mode(request.shipping_mode.as_deref()),
        shipping_value: request.shipping_value,
        due_date: request.due_date,
        payment_terms: request.payment_terms,
        notes: request.notes,
    };

    state.invoice_repository.update_draft(update).await?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;

    Ok(Json(invoice_to_detail_response(detail)))
}

#[utoipa::path(
    post,
    path = "/api/bill/{realmId}/invoices/{invoiceId}/issue",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("invoiceId" = Uuid, Path, description = "Invoice ID")
    ),
    request_body = IssueInvoiceRequest,
    responses(
        (status = 200, description = "Invoice issued", body = InvoiceDetailResponse),
        (status = 400, description = "Bad request - no line items or zero total", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Invoice not found", body = ErrorResponse),
        (status = 409, description = "Conflict - invalid status transition", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn issue_invoice(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, invoice_id)): Path<(String, Uuid)>,
    Json(request): Json<IssueInvoiceRequest>,
) -> Result<Json<InvoiceDetailResponse>, ApiError> {
    tracing::info!("Issuing invoice {} for realm: {}", invoice_id, realm_id);
    require_billing_permission(&state, &identity, &realm_id, "manage").await?;
    require_invoice_policy_allows_writes(&state, &realm_id).await?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;

    // Provider readonly guard: only manual invoices can be issued
    validate_external_invoice_readonly(detail.invoice.provider)?;

    validate_status_transition(
        detail.invoice.status,
        InvoiceStatus::Issued,
        detail.line_items.len(),
        detail.invoice.total,
        ActorType::User,
        false,
        None,
    )?;

    // Determine issue_date: use request override or default to today
    let issue_date = request
        .issue_date
        .unwrap_or(chrono::Utc::now().date_naive());

    // Validate due_date >= issue_date
    if let Some(due) = detail.invoice.due_date
        && due < issue_date
    {
        return Err(ApiError::bad_request(
            "due_date must be on or after issue_date",
        ));
    }

    // Validate at least one of billing_email or billing_phone is non-empty
    let has_email = detail
        .invoice
        .billing_email
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let has_phone = detail
        .invoice
        .billing_phone
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    if !has_email && !has_phone {
        return Err(ApiError::bad_request(
            "At least one of billing_email or billing_phone must be provided",
        ));
    }

    let actor_user_id = Uuid::parse_str(&identity.user_id()).ok();
    state
        .invoice_repository
        .transition_status(InvoiceStatusTransition {
            realm_id: realm_id.clone(),
            invoice_id,
            target_status: InvoiceStatus::Issued,
            actor_user_id,
            actor_type: ActorType::User,
            void_reason: None,
            issue_date: Some(issue_date),
            paid_at: None,
        })
        .await?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;

    Ok(Json(invoice_to_detail_response(detail)))
}

#[utoipa::path(
    post,
    path = "/api/bill/{realmId}/invoices/{invoiceId}/void",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("invoiceId" = Uuid, Path, description = "Invoice ID")
    ),
    request_body = VoidInvoiceRequest,
    responses(
        (status = 200, description = "Invoice voided", body = InvoiceDetailResponse),
        (status = 400, description = "Bad request - void reason required for issued invoices", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Invoice not found", body = ErrorResponse),
        (status = 409, description = "Conflict - terminal state or active refund credit note", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn void_invoice(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, invoice_id)): Path<(String, Uuid)>,
    Json(request): Json<VoidInvoiceRequest>,
) -> Result<Json<InvoiceDetailResponse>, ApiError> {
    tracing::info!("Voiding invoice {} for realm: {}", invoice_id, realm_id);
    require_billing_permission(&state, &identity, &realm_id, "manage").await?;
    require_invoice_policy_allows_writes(&state, &realm_id).await?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;

    // Provider readonly guard: only manual invoices can be voided
    validate_external_invoice_readonly(detail.invoice.provider)?;

    // Active Credit Note guard: an invoice cannot be voided while it has any
    // active refund credit note. Voided notes are non-blocking (audit-only).
    // This guard runs before status-transition validation so that the dedicated
    // 409 message is reachable for `paid` invoices (which are otherwise terminal),
    // and so that a `paid` invoice with only voided notes can still be voided.
    let credit_notes = state
        .credit_note_repository
        .find_by_invoice_id(&realm_id, invoice_id)
        .await?;
    let has_active_note = credit_notes
        .iter()
        .any(|note| note.status == CreditNoteStatus::Active);
    if has_active_note {
        return Err(ApiError::conflict(
            "Invoice cannot be voided while it has active refund credit notes",
        ));
    }

    // Status-transition validation. A paid manual invoice normally remains
    // terminal, but a paid invoice that only has historical voided credit notes
    // is allowed to be voided: those notes are audit-only and have already been
    // reversed out of refunded/remaining totals.
    let has_voided_note = credit_notes
        .iter()
        .any(|note| note.status == CreditNoteStatus::Voided);
    if !(detail.invoice.status == InvoiceStatus::Paid && has_voided_note) {
        validate_status_transition(
            detail.invoice.status,
            InvoiceStatus::Void,
            detail.line_items.len(),
            detail.invoice.total,
            ActorType::User,
            false,
            request.void_reason.as_deref(),
        )?;
    }

    let actor_user_id = Uuid::parse_str(&identity.user_id()).ok();
    state
        .invoice_repository
        .transition_status(InvoiceStatusTransition {
            realm_id: realm_id.clone(),
            invoice_id,
            target_status: InvoiceStatus::Void,
            actor_user_id,
            actor_type: ActorType::User,
            void_reason: request.void_reason,
            issue_date: None,
            paid_at: None,
        })
        .await?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;

    Ok(Json(invoice_to_detail_response(detail)))
}

#[utoipa::path(
    post,
    path = "/api/bill/{realmId}/invoices/{invoiceId}/mark-paid",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("invoiceId" = Uuid, Path, description = "Invoice ID")
    ),
    request_body = MarkPaidRequest,
    responses(
        (status = 200, description = "Invoice marked as paid", body = InvoiceDetailResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Invoice not found", body = ErrorResponse),
        (status = 409, description = "Conflict - invalid status transition", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn mark_paid(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, invoice_id)): Path<(String, Uuid)>,
    Json(request): Json<MarkPaidRequest>,
) -> Result<Json<InvoiceDetailResponse>, ApiError> {
    tracing::info!(
        "Marking invoice {} as paid for realm: {}",
        invoice_id,
        realm_id
    );
    require_billing_permission(&state, &identity, &realm_id, "manage").await?;
    require_invoice_policy_allows_writes(&state, &realm_id).await?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;

    // Provider readonly guard: only manual invoices can be marked as paid
    validate_external_invoice_readonly(detail.invoice.provider)?;

    validate_status_transition(
        detail.invoice.status,
        InvoiceStatus::Paid,
        detail.line_items.len(),
        detail.invoice.total,
        ActorType::User,
        false,
        None,
    )?;

    let paid_at = parse_optional_paid_at(request.paid_at.as_deref())?;
    let actor_user_id = Uuid::parse_str(&identity.user_id()).ok();
    state
        .invoice_repository
        .transition_status(InvoiceStatusTransition {
            realm_id: realm_id.clone(),
            invoice_id,
            target_status: InvoiceStatus::Paid,
            actor_user_id,
            actor_type: ActorType::User,
            void_reason: None,
            issue_date: None,
            paid_at,
        })
        .await?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;

    Ok(Json(invoice_to_detail_response(detail)))
}

#[utoipa::path(
    post,
    path = "/api/bill/{realmId}/invoices/{invoiceId}/credit-notes",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("invoiceId" = Uuid, Path, description = "Invoice ID")
    ),
    request_body = CreateCreditNoteRequest,
    responses(
        (status = 201, description = "Manual credit note created", body = CreditNoteResponse),
        (status = 400, description = "Bad request - amount invalid, status not paid, or exceeds remaining", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - refunds for this provider are managed externally", body = ErrorResponse),
        (status = 404, description = "Invoice not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_credit_note(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, invoice_id)): Path<(String, Uuid)>,
    Json(request): Json<CreateCreditNoteRequest>,
) -> Result<(StatusCode, Json<CreditNoteResponse>), ApiError> {
    tracing::info!(
        "Creating manual credit note for invoice {} in realm: {}",
        invoice_id,
        realm_id
    );
    require_billing_permission(&state, &identity, &realm_id, "manage").await?;
    request
        .validate()
        .map_err(|e: validator::ValidationErrors| {
            CoreError::BadRequest(format!("Validation failed: {}", e))
        })?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;

    // Provider guard: refunds for non-manual providers are managed externally.
    if detail.invoice.provider != InvoiceProvider::Manual {
        return Err(ApiError::forbidden(
            "Refunds for this provider are managed externally",
        ));
    }

    // Status guard: only paid invoices support manual refund recording.
    if detail.invoice.status != InvoiceStatus::Paid {
        return Err(ApiError::bad_request(
            "Only paid invoices support refund recording",
        ));
    }

    // Amount guard: refund must not exceed remaining payable.
    if request.amount > detail.invoice.amount_remaining {
        return Err(ApiError::bad_request(
            "Refund amount exceeds remaining payable",
        ));
    }

    let actor_user_id = Uuid::parse_str(&identity.user_id()).map_err(|_| {
        ApiError::internal("Authenticated identity has invalid user_id format".to_string())
    })?;

    let new_credit_note = NewCreditNote {
        invoice_id,
        realm_id: realm_id.clone(),
        amount: request.amount,
        currency: detail.invoice.currency.clone(),
        source: herald_core::domain::billing::credit_note::CreditNoteSource::Manual,
        external_credit_note_id: None,
        memo: Some(request.memo),
        created_by_user_id: Some(actor_user_id),
    };

    let credit_note = state
        .credit_note_repository
        .create_credit_note_and_update_invoice(new_credit_note)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(CreditNoteResponse::from(credit_note)),
    ))
}

#[utoipa::path(
    post,
    path = "/api/user/bill/invoices",
    tag = "billing-invoice",
    request_body = ApplyInvoiceRequest,
    responses(
        (status = 201, description = "Invoice application created", body = InvoiceDetailResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn apply_invoice(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Json(request): Json<ApplyInvoiceRequest>,
) -> Result<(StatusCode, Json<InvoiceDetailResponse>), ApiError> {
    let realm_id = identity.realm_id();
    tracing::info!("User applying for invoice in realm: {}", realm_id);
    require_token_scope(&identity, &context, CredentialScope::InvoiceApply)?;
    let applicant_user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "apply invoices",
    )?;

    request
        .validate()
        .map_err(|e: validator::ValidationErrors| {
            CoreError::BadRequest(format!("Validation failed: {}", e))
        })?;

    // Trim and validate billing_address is non-empty
    if request.billing_address.trim().is_empty() {
        return Err(ApiError::bad_request("billing_address must not be blank"));
    }

    if request.payment_attempt_id.is_none() && request.subscription_id.is_none() {
        return Err(ApiError::bad_request(
            "At least one of paymentAttemptId or subscriptionId is required",
        ));
    }

    if let Some(pa_id) = request.payment_attempt_id {
        validate_resource_ownership(
            &state.pool,
            OwnedResource::PaymentAttempt,
            pa_id,
            applicant_user_id,
            &realm_id,
        )
        .await?;
        validate_resource_paid(&state.pool, OwnedResource::PaymentAttempt, pa_id, &realm_id)
            .await?;
    }
    if let Some(sub_id) = request.subscription_id {
        validate_resource_ownership(
            &state.pool,
            OwnedResource::Subscription,
            sub_id,
            applicant_user_id,
            &realm_id,
        )
        .await?;
        validate_resource_paid(&state.pool, OwnedResource::Subscription, sub_id, &realm_id).await?;
    }

    validate_invoice_creation_policy(
        &state,
        &realm_id,
        request.payment_attempt_id,
        request.subscription_id,
    )
    .await?;

    let seller_config = state
        .invoice_repository
        .find_seller_config(&realm_id)
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(
                "No seller configuration found for this realm. An admin must configure seller info first.",
            )
        })?;

    let new_invoice = NewInvoice {
        realm_id: realm_id.clone(),
        source: InvoiceSource::UserApplication,
        account_id: applicant_user_id,
        applicant_user_id: Some(applicant_user_id),
        subscription_id: request.subscription_id,
        payment_attempt_id: request.payment_attempt_id,
        currency: request.currency,
        line_items: vec![],
        actor_user_id: Some(applicant_user_id),
        billing_name: request.billing_name,
        billing_address: request.billing_address,
        billing_email: request.billing_email,
        billing_phone: request.billing_phone,
        billing_tax_id: request.billing_tax_id,
        seller_name: seller_config.seller_name,
        seller_address: seller_config.seller_address,
        seller_email: seller_config.seller_email,
        seller_phone: seller_config.seller_phone,
        seller_tax_id: seller_config.seller_tax_id,
        discount_mode: None,
        discount_value: None,
        tax_mode: None,
        tax_value: None,
        shipping_mode: None,
        shipping_value: None,
        due_date: request.due_date,
        payment_terms: seller_config.default_payment_terms,
        notes: request.notes,
    };

    let invoice = state.invoice_repository.create_invoice(new_invoice).await?;

    let detail = load_detail(&state, &realm_id, invoice.id).await?;

    Ok((
        StatusCode::CREATED,
        Json(invoice_to_detail_response(detail)),
    ))
}

#[utoipa::path(
    get,
    path = "/api/user/bill/invoices/apply-eligibility",
    tag = "billing-invoice",
    operation_id = "get_invoice_apply_eligibility",
    params(
        InvoiceApplyEligibilityQuery
    ),
    responses(
        (status = 200, description = "Apply eligibility for the referenced resource", body = InvoiceApplyEligibilityResponse),
        (status = 400, description = "Bad request - invalid referenceType", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - resource owned by another user", body = ErrorResponse),
        (status = 404, description = "Not Found - resource does not exist in this realm", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
/// Per-resource invoice apply-eligibility (read-only, context-level).
///
/// Decision order: ownership → provider → policy → seller config →
/// external-invoice-exists, then delegates the verdict to the pure
/// `determine_invoice_apply_route`. The endpoint resolves facts; the pure
/// function owns the rules (single home — avoids divergence from the write
/// path `validate_invoice_creation_policy` / `apply_invoice`).
pub async fn get_invoice_apply_eligibility(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Query(query): Query<InvoiceApplyEligibilityQuery>,
) -> Result<Json<InvoiceApplyEligibilityResponse>, ApiError> {
    let realm_id = identity.realm_id();
    tracing::info!(
        "User checking invoice apply-eligibility for realm: {} reference_type: {} reference_id: {}",
        realm_id,
        query.reference_type,
        query.reference_id
    );
    require_token_scope(&identity, &context, CredentialScope::InvoiceRead)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "apply invoice eligibility",
    )?;

    let resource = match query.reference_type.as_str() {
        "payment_attempt" => OwnedResource::PaymentAttempt,
        "subscription" => OwnedResource::Subscription,
        other => {
            return Err(ApiError::bad_request(format!(
                "referenceType must be 'payment_attempt' or 'subscription', got '{}'",
                other
            )));
        }
    };

    // Note: we do NOT reuse `validate_resource_ownership` here because that
    // helper returns 400 (bad_request) for not-found, but the spec requires 404
    // for the eligibility endpoint. We mirror its SQL instead.
    let owner_sql = format!(
        "SELECT user_id FROM {} WHERE id = $1 AND realm_id = $2",
        resource.table_name()
    );
    let owner: Option<Option<Uuid>> = sqlx::query_scalar(&owner_sql)
        .bind(query.reference_id)
        .bind(&realm_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    match owner {
        None => {
            return Err(ApiError::not_found(format!(
                "{} {} not found",
                resource.label(),
                query.reference_id
            )));
        }
        Some(Some(uid)) if uid == user_id => {}
        Some(_) => {
            return Err(ApiError::forbidden(format!(
                "You can only check invoice eligibility for your own {}s",
                resource.label()
            )));
        }
    }

    if !resource_is_paid(&state.pool, resource, query.reference_id, &realm_id).await? {
        return Ok(Json(InvoiceApplyEligibilityResponse {
            reference_type: query.reference_type,
            reference_id: query.reference_id,
            can_apply: false,
            route: "disabled".to_string(),
            provider: None,
            reason: Some(format!("Only paid {}s can be invoiced", resource.label())),
        }));
    }

    // Mirrors the write-path SQL in `validate_invoice_creation_policy` exactly.
    let provider: Option<String> = match resource {
        OwnedResource::PaymentAttempt => sqlx::query_scalar(
            "SELECT payment_provider FROM payment_attempts WHERE id = $1 AND realm_id = $2",
        )
        .bind(query.reference_id)
        .bind(&realm_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
        .flatten(),
        OwnedResource::Subscription => sqlx::query_scalar(
            "SELECT payment_provider FROM subscription WHERE id = $1 AND realm_id = $2",
        )
        .bind(query.reference_id)
        .bind(&realm_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?,
    };

    let policy_config = get_invoice_policy(&state, &realm_id).await?;
    let policy = policy_config.policy.clone();

    let has_seller_config = state
        .invoice_repository
        .find_seller_config(&realm_id)
        .await?
        .is_some();

    let (payment_attempt_id, subscription_id) = match resource {
        OwnedResource::PaymentAttempt => (Some(query.reference_id), None),
        OwnedResource::Subscription => (None, Some(query.reference_id)),
    };
    let external_invoice_exists =
        external_sync_invoice_exists(&state, &realm_id, payment_attempt_id, subscription_id)
            .await?;

    let verdict = determine_invoice_apply_route(
        provider.as_deref(),
        &policy,
        has_seller_config,
        external_invoice_exists,
        external_invoice_capability_enabled(&policy_config, provider.as_deref().unwrap_or("")),
    );

    Ok(Json(InvoiceApplyEligibilityResponse {
        reference_type: query.reference_type,
        reference_id: query.reference_id,
        can_apply: verdict.can_apply,
        route: verdict.route,
        provider,
        reason: verdict.reason,
    }))
}

#[utoipa::path(
    get,
    path = "/api/user/bill/invoices",
    tag = "billing-invoice",
    params(
        InvoiceListQuery
    ),
    responses(
        (status = 200, description = "My invoices listed", body = InvoiceListResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_my_invoices(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Query(query): Query<InvoiceListQuery>,
) -> Result<Json<InvoiceListResponse>, ApiError> {
    let realm_id = identity.realm_id();
    tracing::info!("Listing my invoices for realm: {}", realm_id);
    require_token_scope(&identity, &context, CredentialScope::InvoiceRead)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "view invoices",
    )?;

    let mut filters = query.to_filters();
    apply_invoice_policy_list_filter(&state, &realm_id, &mut filters).await?;

    let result = state
        .invoice_repository
        .list_user(&realm_id, user_id, filters)
        .await?;

    Ok(Json(InvoiceListResponse {
        total: result.total,
        page: result.page,
        page_size: result.page_size,
        // Regular users must not see the internal payment-attempt identifier
        // (invoice.md §4.2) — the same trimming the detail endpoint applies.
        data: result
            .data
            .into_iter()
            .map(|summary| {
                let mut response = summary_to_response(summary);
                response.payment_attempt_id = None;
                response
            })
            .collect(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/user/bill/invoices/{invoiceId}",
    tag = "billing-invoice",
    params(
        ("invoiceId" = Uuid, Path, description = "Invoice ID")
    ),
    responses(
        (status = 200, description = "Invoice detail", body = InvoiceDetailResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Invoice not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_my_invoice_scoped(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Path(invoice_id): Path<Uuid>,
) -> Result<Json<InvoiceDetailResponse>, ApiError> {
    let realm_id = identity.realm_id();
    tracing::info!("Getting my invoice {} for realm: {}", invoice_id, realm_id);
    require_token_scope(&identity, &context, CredentialScope::InvoiceRead)?;
    let current_user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "view invoices",
    )?;

    get_my_invoice_for_user(&state, &realm_id, current_user_id, invoice_id).await
}

/// Direct business-handler entry retained for internal callers and tests.
/// Enforces the same browser token scope as `get_my_invoice_scoped`.
pub async fn get_my_invoice(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Path(invoice_id): Path<Uuid>,
) -> Result<Json<InvoiceDetailResponse>, ApiError> {
    let realm_id = identity.realm_id();
    require_token_scope(&identity, &context, CredentialScope::InvoiceRead)?;
    let current_user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "view invoices",
    )?;

    get_my_invoice_for_user(&state, &realm_id, current_user_id, invoice_id).await
}

async fn get_my_invoice_for_user(
    state: &AppState,
    realm_id: &str,
    current_user_id: Uuid,
    invoice_id: Uuid,
) -> Result<Json<InvoiceDetailResponse>, ApiError> {
    let detail = load_detail(state, realm_id, invoice_id).await?;

    validate_invoice_ownership(
        &detail,
        current_user_id,
        "You can only view your own invoices",
    )?;

    let credit_notes = state
        .credit_note_repository
        .find_by_invoice_id(realm_id, invoice_id)
        .await?;

    // Strip Stripe-internal identifiers and admin user IDs so regular users
    // see only refund amount/currency/source, not external IDs or internal
    // operator UUIDs.
    let credit_notes = credit_notes
        .into_iter()
        .map(|mut cn| {
            cn.external_credit_note_id = None;
            cn.created_by_user_id = None;
            cn
        })
        .collect();

    let mut response = invoice_to_detail_response_with_credits(detail, credit_notes);
    // Regular users must NOT receive the internal `payment_attempt_id`; only
    // admin responses carry it. The summary `InvoiceResponse` used by
    // list_my_invoices strips the same field at its own call site.
    response.payment_attempt_id = None;

    Ok(Json(response))
}

/// Validate that an invoice status allows PDF download (not draft).
fn validate_pdf_status(status: InvoiceStatus) -> Result<(), ApiError> {
    if status == InvoiceStatus::Draft {
        return Err(ApiError::conflict(
            "PDF is not available for draft invoices. Issue the invoice first.",
        ));
    }
    Ok(())
}

/// Build a PDF response with Content-Type and Content-Disposition headers.
fn build_pdf_response(pdf_bytes: Vec<u8>, invoice_number: &str) -> Response {
    use axum::http::header;

    let disposition = format!("attachment; filename=\"{}.pdf\"", invoice_number);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(axum::body::Body::from(pdf_bytes))
        .unwrap()
}

/// Check that the current user owns the invoice (via applicant_user_id or account_id).
fn validate_invoice_ownership(
    detail: &InvoiceDetail,
    user_id: Uuid,
    message: &str,
) -> Result<(), ApiError> {
    if detail.invoice.applicant_user_id != Some(user_id)
        && detail.invoice.account_id != Some(user_id)
    {
        return Err(ApiError::forbidden(message));
    }
    Ok(())
}

/// Resolve the PDF response using dual-track logic:
/// - Manual provider: generate PDF via IronPress (caller handles generation)
/// - External provider with url: 302 redirect
/// - External provider without url: 404
fn resolve_external_pdf_response(detail: &InvoiceDetail) -> Option<Result<Response, ApiError>> {
    match detail.invoice.provider {
        InvoiceProvider::Manual => None, // caller handles IronPress generation
        _ => {
            // External provider: redirect or 404
            match &detail.invoice.external_pdf_url {
                Some(url) if !url.is_empty() => {
                    tracing::info!(
                        "Redirecting to external PDF URL for invoice {} (provider: {})",
                        detail.invoice.id,
                        detail.invoice.provider.as_str()
                    );
                    Some(Ok(Response::builder()
                        .status(StatusCode::FOUND)
                        .header(axum::http::header::LOCATION, url.as_str())
                        .body(axum::body::Body::empty())
                        .unwrap()))
                }
                _ => {
                    tracing::info!(
                        "No external PDF URL for invoice {} (provider: {})",
                        detail.invoice.id,
                        detail.invoice.provider.as_str()
                    );
                    Some(Err(ApiError::not_found(format!(
                        "Invoice PDF is managed by {}",
                        detail.invoice.provider.as_str()
                    ))))
                }
            }
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/invoices/{invoiceId}/pdf",
    tag = "billing-invoice",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("invoiceId" = Uuid, Path, description = "Invoice ID")
    ),
    responses(
        (status = 200, description = "Invoice PDF binary data"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Invoice not found", body = ErrorResponse),
        (status = 409, description = "Conflict - invoice is draft", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn download_invoice_pdf(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, invoice_id)): Path<(String, Uuid)>,
) -> Result<Response, ApiError> {
    tracing::info!(
        "Downloading invoice PDF {} for realm: {}",
        invoice_id,
        realm_id
    );
    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;
    let policy = get_invoice_policy(&state, &realm_id).await?;
    validate_pdf_allowed_by_policy(&policy, detail.invoice.provider)?;
    validate_pdf_status(detail.invoice.status)?;

    // External provider dual-track: redirect or 404
    if let Some(response) = resolve_external_pdf_response(&detail) {
        return response;
    }

    // Manual provider: generate PDF via IronPress
    let generator = IronPressInvoicePdfGenerator;
    let pdf_bytes = generator.generate(&detail).await?;

    Ok(build_pdf_response(
        pdf_bytes,
        &detail.invoice.invoice_number,
    ))
}

#[utoipa::path(
    get,
    path = "/api/user/bill/invoices/{invoiceId}/pdf",
    tag = "billing-invoice",
    params(
        ("invoiceId" = Uuid, Path, description = "Invoice ID")
    ),
    responses(
        (status = 200, description = "Invoice PDF binary data"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - not your invoice", body = ErrorResponse),
        (status = 404, description = "Invoice not found", body = ErrorResponse),
        (status = 409, description = "Conflict - invoice is draft", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn download_my_invoice_pdf(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Path(invoice_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let realm_id = identity.realm_id();
    tracing::info!(
        "Downloading my invoice PDF {} for realm: {}",
        invoice_id,
        realm_id
    );
    require_token_scope(&identity, &context, CredentialScope::InvoiceRead)?;
    let current_user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "view invoices",
    )?;

    let detail = load_detail(&state, &realm_id, invoice_id).await?;

    validate_invoice_ownership(
        &detail,
        current_user_id,
        "You can only download your own invoices",
    )?;

    let policy = get_invoice_policy(&state, &realm_id).await?;
    validate_pdf_allowed_by_policy(&policy, detail.invoice.provider)?;
    validate_pdf_status(detail.invoice.status)?;

    // External provider dual-track: redirect or 404
    if let Some(response) = resolve_external_pdf_response(&detail) {
        return response;
    }

    // Manual provider: generate PDF via IronPress
    let generator = IronPressInvoicePdfGenerator;
    let pdf_bytes = generator.generate(&detail).await?;

    Ok(build_pdf_response(
        pdf_bytes,
        &detail.invoice.invoice_number,
    ))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use herald_core::domain::billing::invoice::{
        Invoice, InvoiceDetail, InvoiceProvider, InvoiceSource, InvoiceStatus,
    };

    use super::*;

    /// Build a minimal InvoiceDetail for testing, with only the fields that
    /// matter for PDF dual-track resolution (provider and external_pdf_url).
    fn make_detail(provider: InvoiceProvider, external_pdf_url: Option<&str>) -> InvoiceDetail {
        InvoiceDetail {
            invoice: Invoice {
                id: Uuid::now_v7(),
                realm_id: "test-realm".to_string(),
                invoice_number: "INV-001".to_string(),
                source: InvoiceSource::AdminManual,
                account_id: None,
                applicant_user_id: None,
                subscription_id: None,
                payment_attempt_id: None,
                status: InvoiceStatus::Issued,
                currency: "usd".to_string(),
                provider,
                payment_provider: None,
                external_invoice_id: None,
                external_order_id: None,
                external_status: None,
                external_hosted_url: None,
                external_pdf_url: external_pdf_url.map(|s| s.to_string()),
                external_payload: None,
                tax_details: None,
                issue_date: None,
                due_date: None,
                issued_at: None,
                paid_at: None,
                voided_at: None,
                subtotal: 0,
                discount_amount: 0,
                tax_amount: 0,
                shipping_amount: 0,
                total: 0,
                amount_refunded: 0,
                amount_remaining: 0,
                discount_mode: None,
                discount_value: None,
                tax_mode: None,
                tax_value: None,
                shipping_mode: None,
                shipping_value: None,
                billing_name: None,
                billing_address: None,
                billing_email: None,
                billing_phone: None,
                billing_tax_id: None,
                seller_name: None,
                seller_address: None,
                seller_email: None,
                seller_phone: None,
                seller_tax_id: None,
                notes: None,
                payment_terms: None,
                void_reason: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            line_items: vec![],
            history: vec![],
        }
    }

    #[test]
    fn browser_scope_invoice_rejects_cross_user_detail() {
        let owner = Uuid::now_v7();
        let mut detail = make_detail(InvoiceProvider::Manual, None);
        detail.invoice.account_id = Some(owner);
        detail.invoice.applicant_user_id = Some(owner);

        assert!(
            validate_invoice_ownership(&detail, Uuid::now_v7(), "not owned by caller").is_err()
        );
    }

    #[test]
    fn manual_provider_returns_none_so_caller_generates_pdf() {
        let detail = make_detail(InvoiceProvider::Manual, None);
        assert!(resolve_external_pdf_response(&detail).is_none());
    }

    #[test]
    fn external_provider_with_url_returns_redirect() {
        let detail = make_detail(InvoiceProvider::Stripe, Some("https://stripe.com/pdf/123"));
        let result = resolve_external_pdf_response(&detail);
        assert!(result.is_some());
        let response = result.unwrap().expect("should be Ok");
        assert!(response.status().is_redirection());
    }

    #[test]
    fn external_provider_with_empty_url_returns_404() {
        let detail = make_detail(InvoiceProvider::Stripe, Some(""));
        let result = resolve_external_pdf_response(&detail);
        assert!(result.is_some());
        let err = result.unwrap().expect_err("should be Err for empty URL");
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
