// Handles subscription lifecycle events (paid, update, canceled) and refund events.
// All handlers return 202 Accepted immediately and process events asynchronously.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::webhook_common::{
    create_placeholder_transaction, metadata_value, parse_attempt_id, parse_event_id,
    parse_optional_uuid_field, parse_uuid_field, revoke_payment_roles_for_source,
};
use crate::webhook_subscription_helpers::{
    ResolvedEntitlement, SyncSubscriptionInput, mapping_rule_value, resolve_entitlement_mapping,
    save_subscription_history, sync_subscription,
};
use crate::webhooks::verify_webhook_signature;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::billing::{
    ACTOR_WEBHOOK, BillingRepository, BillingType, ExternalInvoiceData, HistoryEventType,
    InvoiceProvider, InvoiceRepository, InvoiceStatus, PaymentEvent, Subscription,
    SubscriptionHistoryService, SubscriptionStatus, detect_change_type,
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::points::IdempotencyResult;
use herald_core::domain::points::entities::{PointsTransaction, TransactionType};
use herald_core::domain::points::subscription_service::CancelMode;
use herald_core::domain::purchase::metadata_keys;
use herald_core::domain::purchase::{CompletePaymentAttemptInput, PaymentCompletionSource};
use herald_core::domain::realm_config::RealmConfigRepository;

struct CreemCheckoutCompletedPayload {
    event_id: String,
    client_app_id: Uuid,
    entitlement_key: String,
    is_trial: bool,
    creem_product_id: String,
    attempt_id: Option<Uuid>,
}

/// Resolve entitlement_key for a Creem webhook event via the price-aware resolver.
/// Creem is price-less: `external_price_id` is always `None`,
/// which the repository maps to `external_price_id IS NULL`. We never
/// synthesize a product_id placeholder.
///
/// `metadata` is the event metadata object (may carry `herald_entitlement_key`).
async fn resolve_creem_entitlement(
    app_state: &AppState,
    realm_id: &str,
    external_product_id: &str,
    metadata: Option<&Value>,
) -> Result<ResolvedEntitlement, CoreError> {
    let metadata_key = metadata.and_then(|m| {
        m["herald_entitlement_key"]
            .as_str()
            .or_else(|| m["entitlementKey"].as_str())
    });
    Ok(resolve_entitlement_mapping(
        app_state,
        realm_id,
        "creem",
        external_product_id,
        None,
        metadata_key,
    )
    .await?)
}

async fn resolve_creem_entitlement_key(
    app_state: &AppState,
    realm_id: &str,
    external_product_id: &str,
    metadata: Option<&Value>,
) -> Result<String, CoreError> {
    Ok(
        resolve_creem_entitlement(app_state, realm_id, external_product_id, metadata)
            .await?
            .entitlement_key,
    )
}

struct CreemSubscriptionPaidPayload {
    event_id: String,
    user_id: Option<Uuid>,
    entitlement_key: String,
    client_app_id: Option<Uuid>,
    external_subscription_id: String,
    external_product_id: String,
    is_renewal: bool,
    cancel_at_period_end: bool,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
    status: SubscriptionStatus,
    // Renewal charge attribution. `last_transaction_id` is the
    // idempotency anchor for the renewal charge; `amount`/`currency` are in
    // smallest currency units. All optional because Creem may omit them on
    // edge events; callers skip the renewal attempt + invoice write when
    // `amount` is missing or zero (zero-yuan cycle).
    last_transaction_id: Option<String>,
    amount: Option<i64>,
    currency: Option<String>,
}

struct CreemSubscriptionUpdatedPayload {
    event_id: String,
    user_id: Option<Uuid>,
    previous_entitlement_key: String,
    current_entitlement_key: String,
    client_app_id: Option<Uuid>,
    external_subscription_id: String,
    external_product_id: String,
    cancel_at_period_end: bool,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
    status: SubscriptionStatus,
}

struct CreemSubscriptionCanceledPayload {
    event_id: String,
    user_id: Option<Uuid>,
    entitlement_key: Option<String>,
    client_app_id: Option<Uuid>,
    external_subscription_id: String,
    external_product_id: String,
    cancel_at_period_end: bool,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
    status: SubscriptionStatus,
}

struct CreemRefundCreatedPayload {
    event_id: String,
    refund_id: String,
    payment_id: String,
    amount: i64,
    original_amount: i64,
    user_id: Uuid,
    refund_type: String,
    external_subscription_id: Option<String>,
}

struct CreemSubscriptionLifecyclePayload {
    event_id: String,
    user_id: Option<Uuid>,
    entitlement_key: Option<String>,
    client_app_id: Option<Uuid>,
    external_subscription_id: String,
    external_product_id: String,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
}

struct CreemDisputeCreatedPayload {
    event_id: String,
    external_subscription_id: String,
    external_product_id: String,
    amount: i64,
    currency: String,
    dispute_id: String,
}

fn creem_event_object(event: &Value) -> &Value {
    if !event["data"]["object"].is_null() {
        &event["data"]["object"]
    } else {
        &event["object"]
    }
}

fn creem_metadata(object: &Value) -> &Value {
    &object["metadata"]
}

fn parse_creem_user_id(object: &Value) -> Option<Uuid> {
    object
        .get("herald_user_id")
        .or_else(|| object.get("metadata").and_then(|m| m.get("herald_user_id")))
        .or_else(|| object.get("userId"))
        .or_else(|| object.get("metadata").and_then(|m| m.get("userId")))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
}

fn parse_creem_client_app_id(object: &Value) -> Option<Uuid> {
    parse_optional_uuid_field(metadata_value(
        object,
        "herald_client_app_id",
        "clientAppId",
    ))
    .or_else(|| {
        parse_optional_uuid_field(metadata_value(
            creem_metadata(object),
            "herald_client_app_id",
            "clientAppId",
        ))
    })
}

fn parse_creem_entitlement_key(object: &Value) -> Option<String> {
    object["herald_entitlement_key"]
        .as_str()
        .or_else(|| object["metadata"]["herald_entitlement_key"].as_str())
        .or_else(|| object["entitlementKey"].as_str())
        .or_else(|| object["metadata"]["entitlementKey"].as_str())
        .filter(|key| !key.is_empty())
        .map(str::to_string)
}

fn creem_event_data<'a>(event: &'a Value, field: &str) -> &'a Value {
    if !event["data"][field].is_null() {
        &event["data"][field]
    } else {
        &Value::Null
    }
}

fn parse_creem_datetime(value: &Value, field_name: &str) -> Result<DateTime<Utc>, CoreError> {
    if let Some(raw) = value.as_str() {
        return DateTime::parse_from_rfc3339(raw)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| {
                CoreError::BadRequest(format!("Invalid RFC3339 timestamp for {}", field_name))
            });
    }

    if let Some(timestamp) = value.as_i64() {
        return DateTime::<Utc>::from_timestamp(timestamp, 0).ok_or_else(|| {
            CoreError::BadRequest(format!("Invalid unix timestamp for {}", field_name))
        });
    }

    if let Some(timestamp) = value.as_u64() {
        return DateTime::<Utc>::from_timestamp(timestamp as i64, 0).ok_or_else(|| {
            CoreError::BadRequest(format!("Invalid unix timestamp for {}", field_name))
        });
    }

    Err(CoreError::BadRequest(format!(
        "Missing or invalid {}",
        field_name
    )))
}

fn parse_optional_creem_datetime(value: &Value) -> Result<Option<DateTime<Utc>>, CoreError> {
    if value.is_null() {
        return Ok(None);
    }

    parse_creem_datetime(value, "timestamp").map(Some)
}

/// Normalize a Creem subscription object's billing period to a unique
/// `(period_start, period_end)` pair (P0, symmetric to
/// Stripe's `normalize_stripe_period`).
///
/// Creem exposes the period under several field-name variants
/// (`currentPeriodStart` / `current_period_start` / `current_period_start_date`,
/// and the matching `*End` / `*end_date`). This function tries each variant
/// for both endpoints.
///
/// Returns `Some((start, end))` only when both endpoints resolve and form a
/// valid window (`start < end`). Returns `None` on any partial / missing /
/// unparseable / inverted result — per P0 the caller must then skip the
/// grant, emit a structured warning, and await a later webhook / API
/// compensation (never guess the period from event time, never write a
/// ledger with an invented period).
fn normalize_creem_period(object: &Value) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let start = read_creem_period_field(
        object,
        &[
            "currentPeriodStart",
            "current_period_start",
            "current_period_start_date",
        ],
    );
    let end = read_creem_period_field(
        object,
        &[
            "currentPeriodEnd",
            "current_period_end",
            "current_period_end_date",
        ],
    );
    match (start, end) {
        (Some(s), Some(e)) if s < e => Some((s, e)),
        _ => None,
    }
}

fn read_creem_period_field(object: &Value, fields: &[&str]) -> Option<DateTime<Utc>> {
    for field in fields {
        let Some(v) = object.get(*field) else {
            continue;
        };
        if v.is_null() {
            continue;
        }
        // `parse_creem_datetime` returns Result; on error treat as absent so
        // the P0 "skip + warn" path applies uniformly.
        if let Ok(dt) = parse_creem_datetime(v, field) {
            return Some(dt);
        }
    }
    None
}

/// Extract tax details from a Creem checkout.completed event object.
///
/// Creem (Merchant of Record) includes tax fields in the checkout object.
/// Common field names: tax_amount, tax_rate, tax_country, tax_region.
/// Returns None if no tax fields are present.
fn extract_creem_tax_details(object: &Value) -> Option<serde_json::Value> {
    let tax_amount = object.get("tax_amount").and_then(|v| v.as_i64());
    let tax_rate = object.get("tax_rate").and_then(|v| v.as_f64());
    let tax_country = object
        .get("tax_country")
        .and_then(|v| v.as_str())
        .map(String::from);
    let tax_region = object
        .get("tax_region")
        .and_then(|v| v.as_str())
        .map(String::from);

    if tax_amount.is_none() && tax_rate.is_none() && tax_country.is_none() && tax_region.is_none() {
        return None;
    }

    let mut map = serde_json::Map::new();
    if let Some(amount) = tax_amount {
        map.insert("tax_amount".to_string(), serde_json::Value::from(amount));
    }
    if let Some(rate) = tax_rate {
        map.insert("tax_rate".to_string(), serde_json::json!(rate));
    }
    if let Some(country) = tax_country {
        map.insert(
            "tax_country".to_string(),
            serde_json::Value::String(country),
        );
    }
    if let Some(region) = tax_region {
        map.insert("tax_region".to_string(), serde_json::Value::String(region));
    }

    Some(serde_json::Value::Object(map))
}

fn parse_creem_status(
    status: Option<&str>,
    cancel_at_period_end: bool,
) -> Result<SubscriptionStatus, CoreError> {
    let parsed = match status.unwrap_or("active").to_lowercase().as_str() {
        "active" => SubscriptionStatus::Active,
        "trialing" => SubscriptionStatus::Trialing,
        "canceled" => SubscriptionStatus::Canceled,
        "expired" => SubscriptionStatus::Expired,
        "incomplete" => SubscriptionStatus::Incomplete,
        "paused" => SubscriptionStatus::Paused,
        "past_due" => SubscriptionStatus::PastDue,
        "scheduled_cancel" => SubscriptionStatus::ScheduledCancel,
        "dispute" => SubscriptionStatus::Dispute,
        other => {
            return Err(CoreError::BadRequest(format!(
                "Invalid subscription status: {}",
                other
            )));
        }
    };

    if cancel_at_period_end && parsed == SubscriptionStatus::Active {
        Ok(SubscriptionStatus::ScheduledCancel)
    } else {
        Ok(parsed)
    }
}

fn parse_checkout_completed_payload(
    event: &Value,
) -> Result<CreemCheckoutCompletedPayload, CoreError> {
    let metadata = &event["object"]["metadata"];

    let entitlement_key = metadata["herald_entitlement_key"]
        .as_str()
        .or_else(|| metadata["entitlementKey"].as_str())
        .unwrap_or("")
        .to_string();

    Ok(CreemCheckoutCompletedPayload {
        event_id: parse_event_id(event)?,
        client_app_id: parse_uuid_field(
            metadata_value(metadata, "herald_client_app_id", "clientAppId"),
            "clientAppId",
        )?,
        entitlement_key,
        is_trial: metadata["herald_trial_days"]
            .as_u64()
            .or_else(|| metadata["trialDays"].as_u64())
            .is_some_and(|days| days > 0),
        creem_product_id: event["object"]["product"]["id"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_default(),
        attempt_id: parse_attempt_id(&metadata[metadata_keys::ATTEMPT_ID]),
    })
}

/// Normalize Creem billing type strings to domain BillingType.
///
/// Creem uses "onetime" for one-time products. The domain uses "one_time".
/// Also handles "subscription" as an alias for "recurring".
fn normalize_creem_billing_type(raw: &str) -> BillingType {
    match raw.to_ascii_lowercase().as_str() {
        "onetime" | "one_time" => BillingType::OneTime,
        "recurring" | "subscription" => BillingType::Recurring,
        _ => BillingType::Recurring,
    }
}

fn parse_subscription_paid_payload(
    event: &Value,
) -> Result<CreemSubscriptionPaidPayload, CoreError> {
    let object = creem_event_object(event);
    let cancel_at_period_end = object["cancelAtPeriodEnd"].as_bool().unwrap_or(false);

    let entitlement_key = object["herald_entitlement_key"]
        .as_str()
        .or_else(|| object["metadata"]["herald_entitlement_key"].as_str())
        .or_else(|| object["entitlementKey"].as_str())
        .or_else(|| object["metadata"]["entitlementKey"].as_str())
        .unwrap_or("")
        .to_string();

    let user_id = object
        .get("herald_user_id")
        .or_else(|| object.get("metadata").and_then(|m| m.get("herald_user_id")))
        .or_else(|| object.get("userId"))
        .or_else(|| object.get("metadata").and_then(|m| m.get("userId")))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    Ok(CreemSubscriptionPaidPayload {
        event_id: parse_event_id(event)?,
        user_id,
        entitlement_key,
        client_app_id: parse_optional_uuid_field(metadata_value(
            object,
            "herald_client_app_id",
            "clientAppId",
        ))
        .or_else(|| {
            parse_optional_uuid_field(metadata_value(
                &object["metadata"],
                "herald_client_app_id",
                "clientAppId",
            ))
        }),
        external_subscription_id: object["subscriptionId"]
            .as_str()
            .or_else(|| object["id"].as_str())
            .map(str::to_string)
            .ok_or_else(|| CoreError::BadRequest("Missing subscriptionId".to_string()))?,
        external_product_id: object["productId"]
            .as_str()
            .or_else(|| object["product"]["id"].as_str())
            .map(str::to_string)
            .ok_or_else(|| CoreError::BadRequest("Missing productId".to_string()))?,
        is_renewal: object["isRenewal"].as_bool().unwrap_or(false),
        cancel_at_period_end,
        current_period_start: parse_optional_creem_datetime(&object["currentPeriodStart"])
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_start"]))
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_start_date"]))?,
        current_period_end: parse_optional_creem_datetime(&object["currentPeriodEnd"])
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_end"]))
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_end_date"]))?,
        status: parse_creem_status(object["status"].as_str(), cancel_at_period_end)?,
        // Renewal attribution. `last_transaction_id` falls back to
        // camelCase; amount reuses the checkout resolver priority
        // (object.amount -> object.product.price) and is in smallest currency
        // units; currency falls back to `currency_code`.
        last_transaction_id: object["last_transaction_id"]
            .as_str()
            .or_else(|| object["lastTransactionId"].as_str())
            .map(str::to_string),
        amount: object["amount"]
            .as_i64()
            .or_else(|| object["product"]["price"].as_i64()),
        currency: object["currency"]
            .as_str()
            .or_else(|| object["currency_code"].as_str())
            .map(str::to_string),
    })
}

fn parse_subscription_updated_payload(
    event: &Value,
) -> Result<CreemSubscriptionUpdatedPayload, CoreError> {
    let object = creem_event_object(event);
    let previous_attributes = creem_event_data(event, "previousAttributes");
    let cancel_at_period_end = object["cancelAtPeriodEnd"].as_bool().unwrap_or(false);

    let current_entitlement_key = object["herald_entitlement_key"]
        .as_str()
        .or_else(|| object["metadata"]["herald_entitlement_key"].as_str())
        .or_else(|| object["entitlementKey"].as_str())
        .or_else(|| object["metadata"]["entitlementKey"].as_str())
        .unwrap_or("")
        .to_string();

    let previous_entitlement_key = previous_attributes["herald_entitlement_key"]
        .as_str()
        .or_else(|| previous_attributes["entitlementKey"].as_str())
        .unwrap_or("")
        .to_string();

    let user_id = object
        .get("herald_user_id")
        .or_else(|| object.get("metadata").and_then(|m| m.get("herald_user_id")))
        .or_else(|| object.get("userId"))
        .or_else(|| object.get("metadata").and_then(|m| m.get("userId")))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    Ok(CreemSubscriptionUpdatedPayload {
        event_id: parse_event_id(event)?,
        user_id,
        previous_entitlement_key,
        current_entitlement_key,
        client_app_id: parse_optional_uuid_field(metadata_value(
            object,
            "herald_client_app_id",
            "clientAppId",
        ))
        .or_else(|| {
            parse_optional_uuid_field(metadata_value(
                &object["metadata"],
                "herald_client_app_id",
                "clientAppId",
            ))
        }),
        external_subscription_id: object["subscriptionId"]
            .as_str()
            .or_else(|| object["id"].as_str())
            .map(str::to_string)
            .ok_or_else(|| CoreError::BadRequest("Missing subscriptionId".to_string()))?,
        external_product_id: object["productId"]
            .as_str()
            .or_else(|| object["product"]["id"].as_str())
            .map(str::to_string)
            .ok_or_else(|| CoreError::BadRequest("Missing productId".to_string()))?,
        cancel_at_period_end,
        current_period_start: parse_optional_creem_datetime(&object["currentPeriodStart"])
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_start"]))
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_start_date"]))?,
        current_period_end: parse_optional_creem_datetime(&object["currentPeriodEnd"])
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_end"]))
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_end_date"]))?,
        status: parse_creem_status(object["status"].as_str(), cancel_at_period_end)?,
    })
}

fn parse_subscription_canceled_payload(
    event: &Value,
) -> Result<CreemSubscriptionCanceledPayload, CoreError> {
    let object = creem_event_object(event);
    let cancel_at_period_end = object["cancelAtPeriodEnd"].as_bool().unwrap_or(false);
    let status = if cancel_at_period_end {
        SubscriptionStatus::ScheduledCancel
    } else {
        SubscriptionStatus::Canceled
    };

    let entitlement_key = object["herald_entitlement_key"]
        .as_str()
        .or_else(|| object["metadata"]["herald_entitlement_key"].as_str())
        .or_else(|| object["entitlementKey"].as_str())
        .or_else(|| object["metadata"]["entitlementKey"].as_str())
        .map(str::to_string);

    let user_id = object
        .get("herald_user_id")
        .or_else(|| object.get("metadata").and_then(|m| m.get("herald_user_id")))
        .or_else(|| object.get("userId"))
        .or_else(|| object.get("metadata").and_then(|m| m.get("userId")))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    Ok(CreemSubscriptionCanceledPayload {
        event_id: parse_event_id(event)?,
        user_id,
        entitlement_key,
        client_app_id: parse_optional_uuid_field(metadata_value(
            object,
            "herald_client_app_id",
            "clientAppId",
        ))
        .or_else(|| {
            parse_optional_uuid_field(metadata_value(
                &object["metadata"],
                "herald_client_app_id",
                "clientAppId",
            ))
        }),
        external_subscription_id: object["subscriptionId"]
            .as_str()
            .or_else(|| object["id"].as_str())
            .map(str::to_string)
            .ok_or_else(|| CoreError::BadRequest("Missing subscriptionId".to_string()))?,
        external_product_id: object["productId"]
            .as_str()
            .or_else(|| object["product"]["id"].as_str())
            .map(str::to_string)
            .ok_or_else(|| CoreError::BadRequest("Missing productId".to_string()))?,
        cancel_at_period_end,
        current_period_start: parse_optional_creem_datetime(&object["currentPeriodStart"])
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_start"]))?,
        current_period_end: parse_optional_creem_datetime(&object["currentPeriodEnd"])
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_end"]))?,
        status,
    })
}

fn parse_refund_created_payload(event: &Value) -> Result<CreemRefundCreatedPayload, CoreError> {
    let object = creem_event_object(event);

    Ok(CreemRefundCreatedPayload {
        event_id: parse_event_id(event)?,
        refund_id: object["id"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing refund id".to_string()))?
            .to_string(),
        payment_id: object["paymentId"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing paymentId".to_string()))?
            .to_string(),
        amount: object["amount"]
            .as_i64()
            .ok_or_else(|| CoreError::BadRequest("Missing or invalid amount".to_string()))?,
        original_amount: object["originalAmount"].as_i64().ok_or_else(|| {
            CoreError::BadRequest("Missing or invalid originalAmount".to_string())
        })?,
        user_id: parse_uuid_field(
            metadata_value(&object["metadata"], "herald_user_id", "userId"),
            "userId",
        )?,
        refund_type: object["metadata"]["refundType"]
            .as_str()
            .unwrap_or("subscription")
            .to_string(),
        external_subscription_id: object["subscriptionId"].as_str().map(str::to_string),
    })
}

fn parse_subscription_lifecycle_payload(
    event: &Value,
) -> Result<CreemSubscriptionLifecyclePayload, CoreError> {
    let object = creem_event_object(event);

    Ok(CreemSubscriptionLifecyclePayload {
        event_id: parse_event_id(event)?,
        user_id: parse_creem_user_id(object),
        entitlement_key: parse_creem_entitlement_key(object),
        client_app_id: parse_creem_client_app_id(object),
        external_subscription_id: object["subscriptionId"]
            .as_str()
            .or_else(|| object["id"].as_str())
            .map(str::to_string)
            .ok_or_else(|| CoreError::BadRequest("Missing subscription id".to_string()))?,
        external_product_id: object["productId"]
            .as_str()
            .or_else(|| object["product"]["id"].as_str())
            .or_else(|| object["product"].as_str())
            .map(str::to_string)
            .ok_or_else(|| CoreError::BadRequest("Missing product id".to_string()))?,
        current_period_start: parse_optional_creem_datetime(&object["currentPeriodStart"])
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_start"]))
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_start_date"]))?,
        current_period_end: parse_optional_creem_datetime(&object["currentPeriodEnd"])
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_end"]))
            .or_else(|_| parse_optional_creem_datetime(&object["current_period_end_date"]))?,
    })
}

fn parse_dispute_created_payload(event: &Value) -> Result<CreemDisputeCreatedPayload, CoreError> {
    let object = creem_event_object(event);
    let subscription = &object["subscription"];

    Ok(CreemDisputeCreatedPayload {
        event_id: parse_event_id(event)?,
        external_subscription_id: subscription["id"]
            .as_str()
            .or_else(|| object["transaction"]["subscription"].as_str())
            .map(str::to_string)
            .ok_or_else(|| CoreError::BadRequest("Missing dispute subscription id".to_string()))?,
        external_product_id: subscription["product"]
            .as_str()
            .or_else(|| subscription["product"]["id"].as_str())
            .map(str::to_string)
            .unwrap_or_default(),
        amount: object["amount"].as_i64().unwrap_or(0),
        currency: object["currency"].as_str().unwrap_or("").to_string(),
        dispute_id: object["id"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing dispute id".to_string()))?
            .to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
async fn sync_creem_subscription(
    app_state: &AppState,
    realm_id: &str,
    user_id: Uuid,
    creem_subscription_id: &str,
    client_app_id: Option<Uuid>,
    entitlement_key: String,
    creem_product_id: String,
    status: SubscriptionStatus,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
    cancel_at_period_end: bool,
    cancel_at: Option<DateTime<Utc>>,
    existing_subscription: Option<Subscription>,
) -> Result<Option<(Subscription, Option<Subscription>)>, CoreError> {
    sync_subscription(
        app_state,
        SyncSubscriptionInput {
            provider: "creem",
            realm_id: realm_id.to_string(),
            user_id: Some(user_id),
            external_subscription_id: creem_subscription_id.to_string(),
            external_product_id: creem_product_id,
            client_app_id,
            entitlement_key,
            external_price_id: None,
            provider_metadata: None,
            status,
            current_period_start,
            current_period_end,
            cancel_at_period_end,
            cancel_at,
            existing_subscription,
        },
    )
    .await
}

/// Handle checkout.completed events
///
/// For one-time products, completes payment attempt and grants topup_credit.
/// For recurring products, records audit state and defers to subscription.paid.
async fn handle_checkout_completed(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_checkout_completed_payload(&event)?;
    let event_id = payload.event_id.as_str();

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing checkout.completed event"
    );

    // Resolve entitlement_key via price-aware fallback chain (price-less)
    let entitlement_key = if payload.entitlement_key.is_empty() {
        resolve_creem_entitlement_key(&app_state, realm_id, &payload.creem_product_id, None).await?
    } else {
        payload.entitlement_key.clone()
    };

    let metadata = &event["object"]["metadata"];
    let event_object = &event["object"];

    let billing_type = {
        // Priority 1: metadata herald_billing_kind
        let from_metadata = metadata["herald_billing_kind"]
            .as_str()
            .map(normalize_creem_billing_type);

        // Priority 2: Creem product.billing_type
        let from_product = event_object["product"]["billing_type"]
            .as_str()
            .map(normalize_creem_billing_type);

        // Priority 3: mapping lookup by provider product ID
        let from_mapping = if from_metadata.is_none() && from_product.is_none() {
            app_state
                .entitlement_mapping_service
                .find_mapping_by_provider_product(realm_id, "creem", &payload.creem_product_id)
                .await
                .ok()
                .flatten()
                .and_then(|m| m.billing_type)
        } else {
            None
        };

        from_metadata
            .or(from_product)
            .or(from_mapping)
            .unwrap_or(BillingType::Recurring)
    };

    info!(
        realm_id = %realm_id,
        client_app_id = %payload.client_app_id,
        entitlement_key = %entitlement_key,
        is_trial = payload.is_trial,
        creem_product_id = %payload.creem_product_id,
        billing_type = %billing_type.as_str(),
        event_id = %event_id,
        "Checkout completed -- dispatching by billing_type"
    );

    // Extract amount/currency with fallback: event.object -> event.object.product -> payment_attempts -> skip with warn
    let amount_from_event = event_object["amount"]
        .as_i64()
        .or_else(|| event_object["product"]["price"].as_i64());
    let currency_from_event = event_object["currency"]
        .as_str()
        .or_else(|| event_object["product"]["currency"].as_str())
        .map(String::from);

    let (resolved_amount, resolved_currency, resolved_account_id) = match (
        amount_from_event,
        currency_from_event.as_deref(),
    ) {
        (Some(amt), Some(cur)) => {
            // Priority 1: amount and currency from event.object
            (amt, cur.to_string(), None)
        }
        _ => {
            // Priority 2: query payment_attempts via checkout session ID as provider_reference
            let checkout_id = event_object["id"].as_str().unwrap_or_default();
            match app_state
                .payment_attempt_service
                .get_payment_attempt_by_provider_reference("creem", checkout_id)
                .await
            {
                Ok(Some(attempt)) if attempt.realm_id == realm_id => (
                    attempt.amount,
                    attempt.currency.clone(),
                    Some(attempt.user_id),
                ),
                Ok(Some(attempt)) => {
                    // The provider-reference lookup is realm-free; never
                    // attribute another realm's attempt to this realm's invoice.
                    warn!(
                        realm_id = %realm_id,
                        event_id = %event_id,
                        checkout_id = checkout_id,
                        attempt_realm_id = %attempt.realm_id,
                        "Creem checkout payment_attempt belongs to a different realm -- skipping invoice sync"
                    );
                    (0, String::new(), None)
                }
                Ok(None) => {
                    // Priority 3: not found, skip sync
                    warn!(
                        realm_id = %realm_id,
                        event_id = %event_id,
                        checkout_id = %checkout_id,
                        "No payment_attempt found for Creem checkout -- skipping invoice sync"
                    );
                    (0, String::new(), None)
                }
                Err(e) => {
                    warn!(
                        realm_id = %realm_id,
                        event_id = %event_id,
                        error = %e,
                        "Failed to query payment_attempt for Creem checkout -- skipping invoice sync"
                    );
                    (0, String::new(), None)
                }
            }
        }
    };

    // Extract tax details from Creem event payload (MoR tax fields)
    let tax_details = extract_creem_tax_details(event_object);

    if !resolved_currency.is_empty() {
        let checkout_id = event_object["id"].as_str().map(String::from);
        let external_data = ExternalInvoiceData {
            realm_id: realm_id.to_string(),
            provider: InvoiceProvider::Creem,
            payment_provider: Some("creem".to_string()),
            external_invoice_id: checkout_id,
            external_order_id: Some(payload.event_id.clone()),
            external_status: Some("completed".to_string()),
            external_hosted_url: None,
            external_pdf_url: None,
            external_payload: Some(event.clone()),
            tax_details,
            account_id: resolved_account_id,
            // Creem buyer-snapshot extraction is out of scope; left None and
            // COALESCE-preserved on upsert.
            applicant_user_id: None,
            billing_name: None,
            billing_email: None,
            billing_phone: None,
            billing_address: None,
            currency: resolved_currency,
            total: resolved_amount,
            status: InvoiceStatus::Paid,
            // Attribute the checkout invoice to the originating checkout payment
            // attempt (one-time or first-period recurring). `subscription_id`
            // stays None here: for one-time there is no subscription; for
            // recurring the subscription is created later by `subscription.paid`,
            // whose first-period branch returns early and does NOT re-upsert this
            // checkout invoice (P0 dedup — the checkout invoice is keyed by
            // checkout id, the renewal invoice by transaction id).
            subscription_id: None,
            payment_attempt_id: payload.attempt_id,
        };

        match app_state
            .invoice_repository
            .upsert_external_invoice(external_data)
            .await
        {
            Ok(_) => {
                info!(
                    realm_id = %realm_id,
                    event_id = %event_id,
                    external_order_id = %payload.event_id,
                    "Creem invoice synced successfully"
                );
            }
            Err(e) => {
                warn!(
                    realm_id = %realm_id,
                    event_id = %event_id,
                    error = %e,
                    "Failed to sync Creem invoice -- payment flow continues"
                );
            }
        }
    }

    match billing_type {
        BillingType::OneTime => {
            if let Some(attempt_id) = payload.attempt_id {
                let provider_transaction_id = event_object["id"]
                    .as_str()
                    .ok_or_else(|| CoreError::BadRequest("Missing checkout id".to_string()))?
                    .to_string();
                let completed_at = parse_optional_creem_datetime(&event_object["createdAt"])?
                    .or_else(|| {
                        parse_optional_creem_datetime(&event_object["created_at"])
                            .ok()
                            .flatten()
                    })
                    .unwrap_or_else(Utc::now);

                crate::shared_fulfillment::fulfill_provider_event(
                    &app_state,
                    realm_id,
                    attempt_id,
                    "creem",
                    "succeeded",
                    provider_transaction_id,
                    completed_at,
                    Some(BillingType::OneTime),
                )
                .await?;

                info!(
                    realm_id = %realm_id,
                    event_id = %event_id,
                    attempt_id = %attempt_id,
                    billing_type = "one_time",
                    "One-time checkout completed -- payment attempt fulfilled, topup_credit granted"
                );

                Ok(create_placeholder_transaction(
                    attempt_id,
                    realm_id,
                    TransactionType::Recharge,
                ))
            } else {
                warn!(
                    realm_id = %realm_id,
                    event_id = %event_id,
                    billing_type = "one_time",
                    "One-time checkout completed without attemptId -- auditing but not fulfilling"
                );

                Ok(create_placeholder_transaction(
                    payload.client_app_id,
                    realm_id,
                    TransactionType::Recharge,
                ))
            }
        }
        BillingType::Recurring => {
            info!(
                realm_id = %realm_id,
                event_id = %event_id,
                "Recurring checkout completed -- subscription creation deferred to subscription.paid"
            );

            Ok(create_placeholder_transaction(
                payload.client_app_id,
                realm_id,
                TransactionType::SubscriptionGrant,
            ))
        }
        // Non-renewing fulfillment (create a fixed-duration Subscription) is
        // fulfill_non_renewing_purchase dispatch.
        BillingType::NonRenewing => {
            info!(
                realm_id = %realm_id,
                event_id = %event_id,
                "Non-renewing checkout completed -- fulfillment deferred (pay_model BE-D02)"
            );

            Ok(create_placeholder_transaction(
                payload.client_app_id,
                realm_id,
                TransactionType::SubscriptionGrant,
            ))
        }
    }
}

/// Handle subscription.paid events
///
/// Grants subscription points to user on initial subscription or renewal.
async fn handle_subscription_paid(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let object = creem_event_object(&event);

    if let Some(attempt_id) = parse_attempt_id(&object["metadata"][metadata_keys::ATTEMPT_ID]) {
        let provider_transaction_id = object["subscriptionId"]
            .as_str()
            .or_else(|| object["id"].as_str())
            .ok_or_else(|| CoreError::BadRequest("Missing provider transaction id".to_string()))?
            .to_string();
        let completed_at = parse_optional_creem_datetime(&object["currentPeriodStart"])?
            .or_else(|| {
                parse_optional_creem_datetime(&object["current_period_start"])
                    .ok()
                    .flatten()
            })
            .unwrap_or_else(Utc::now);

        app_state
            .purchase_service
            .complete_succeeded_payment_attempt(CompletePaymentAttemptInput {
                attempt_id,
                provider_status: "succeeded".to_string(),
                provider_transaction_id,
                completed_at,
                source: PaymentCompletionSource::ProviderWebhook {
                    provider: "creem".to_string(),
                },
                billing_type_override: None,
                expected_realm_id: Some(realm_id.to_string()),
            })
            .await?;

        return Ok(create_placeholder_transaction(
            attempt_id,
            realm_id,
            TransactionType::SubscriptionGrant,
        ));
    }

    let payload = parse_subscription_paid_payload(&event)?;
    let event_id = payload.event_id.as_str();

    // Resolve user_id: prefer payload, fall back to existing subscription
    let user_id = match payload.user_id {
        Some(uid) => uid,
        None => {
            let existing = app_state
                .billing_repository
                .find_by_external_subscription_id(&payload.external_subscription_id, "creem")
                .await?
                .ok_or_else(|| {
                    CoreError::BadRequest(format!(
                        "Cannot resolve userId: no userId in event and no existing subscription for {}",
                        payload.external_subscription_id
                    ))
                })?;
            existing.user_id
        }
    };

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing subscription.paid event"
    );

    // Resolve the price-level entitlement (projection key + strategy mapping).
    // Always run the price-aware resolver so the strategy mapping is price-level
    // (US-EM-008). Creem is price-less, so the resolver maps to
    // `external_price_id IS NULL`.
    let resolved =
        resolve_creem_entitlement(&app_state, realm_id, &payload.external_product_id, None).await?;
    let entitlement_key = if payload.entitlement_key.is_empty() {
        resolved.entitlement_key.clone()
    } else {
        payload.entitlement_key.clone()
    };
    let strategy_mapping = resolved.mapping;

    let synced = sync_creem_subscription(
        &app_state,
        realm_id,
        user_id,
        payload.external_subscription_id.as_str(),
        payload.client_app_id,
        entitlement_key.clone(),
        payload.external_product_id,
        payload.status,
        payload.current_period_start,
        payload.current_period_end,
        payload.cancel_at_period_end,
        None,
        None,
    )
    .await?;

    if let Some((subscription, previous)) = synced.as_ref() {
        let history_event_type = if payload.is_renewal {
            HistoryEventType::Renewed
        } else {
            HistoryEventType::Created
        };

        save_subscription_history(
            &app_state,
            previous.as_ref(),
            subscription,
            history_event_type,
        )
        .await?;
    }

    // Normalize the provider billing period (P0, symmetric
    // to Stripe). When the period cannot be uniquely resolved we skip the
    // grant and emit a structured warning — never guess the period from
    // event time, never write a ledger with an invented period (P0).
    let normalized_period = normalize_creem_period(creem_event_object(&event));

    // bucket_id was resolved eagerly above and bound at subscription creation.
    // synced carries the persisted subscription_id (fallback to nil only when
    // sync returned None — an edge case that should not occur for paid events).
    let subscription_id = synced
        .as_ref()
        .map(|(subscription, _)| subscription.id)
        .unwrap_or_else(Uuid::nil);

    let grant_result = if let Some((period_start, period_end)) = normalized_period {
        Some(
            app_state
                .subscription_service
                .handle_subscription_paid(
                    user_id,
                    subscription_id,
                    realm_id,
                    &strategy_mapping,
                    payload.is_renewal,
                    period_start,
                    period_end,
                    payload.event_id.clone(),
                )
                .await,
        )
    } else {
        warn!(
            realm_id = %realm_id,
            user_id = %user_id,
            external_subscription_id = %payload.external_subscription_id,
            event_id = %event_id,
            reason = "period_uniquely_unresolvable",
            source = "creem",
            "Creem period normalization failed; skipping subscription grant and awaiting compensation (P0)"
        );
        // Mirrors the graceful-skip outcome of EntitlementMappingNotFound
        // below: no grant is issued, the event is acknowledged, and a later
        // webhook / API compensation can reprocess.
        None
    };

    if let Some(Err(error)) = grant_result {
        if matches!(error, CoreError::EntitlementMappingNotFound) {
            info!(
                realm_id = %realm_id,
                user_id = %user_id,
                entitlement_key = %entitlement_key,
                event_id = %event_id,
                "Subscription projection synced; skipping points grant because entitlement mapping is disabled or missing"
            );
        } else {
            return Err(error);
        }
    }

    // Only the renewal branch reaches here: the first-period branch
    // (`attempt_id` in metadata) returns early above, so this never duplicates
    // the first-period invoice (P0 dedup).
    // Zero-yuan cycle: when `amount` is missing or == 0 we SKIP both the
    // renewal attempt and the invoice write (DB CHECK `amount > 0`,
    // `20260408_unified_purchase.sql:100`). The subscription cycle / points
    // grant above still ran. Best-effort: failures only warn and never block
    // the already-completed cycle/points.
    if let Some(amount) = payload.amount {
        if amount > 0 {
            let currency = payload.currency.clone().unwrap_or_default();
            // Idempotency key: creem_renewal:{ext_sub_id}:{last_transaction_id},
            // falling back to current_period_start. Without either anchor we
            // cannot dedup safely, so we warn and skip the attempt/invoice.
            let provider_reference = payload
                .last_transaction_id
                .as_deref()
                .map(|tx| format!("creem_renewal:{}:{}", payload.external_subscription_id, tx))
                .or_else(|| {
                    payload.current_period_start.map(|ps| {
                        format!(
                            "creem_renewal:{}:{}",
                            payload.external_subscription_id,
                            ps.to_rfc3339()
                        )
                    })
                });

            let completed_at = payload.current_period_start.unwrap_or_else(Utc::now);

            let renewal_attempt = match provider_reference {
                Some(reference) => {
                    match app_state
                        .payment_attempt_service
                        .record_subscription_renewal_attempt(
                            herald_core::domain::payment_attempt::RecordRenewalAttemptInput {
                                realm_id: realm_id.to_string(),
                                user_id,
                                payment_provider: "creem".to_string(),
                                target_id: strategy_mapping.id,
                                amount,
                                currency: currency.clone(),
                                provider_reference: reference.clone(),
                                completed_at,
                            },
                        )
                        .await
                    {
                        Ok(attempt) => Some(attempt),
                        Err(e) => {
                            warn!(
                                realm_id = %realm_id,
                                user_id = %user_id,
                                external_subscription_id = %payload.external_subscription_id,
                                event_id = %event_id,
                                error = %e,
                                "Failed to record Creem renewal payment attempt -- subscription cycle/points unaffected"
                            );
                            None
                        }
                    }
                }
                None => {
                    warn!(
                        realm_id = %realm_id,
                        user_id = %user_id,
                        external_subscription_id = %payload.external_subscription_id,
                        event_id = %event_id,
                        "Missing last_transaction_id and current_period_start -- cannot build renewal idempotency key, skipping renewal attempt/invoice"
                    );
                    None
                }
            };

            // Upsert renewal invoice with attribution. external_invoice_id is
            // the transaction id (fallback `{ext_sub_id}:{period_start}`); when
            // no anchor at all is available we skip the invoice write.
            let external_invoice_id = payload.last_transaction_id.clone().or_else(|| {
                payload
                    .current_period_start
                    .map(|ps| format!("{}:{}", payload.external_subscription_id, ps.to_rfc3339()))
            });

            if let (Some(attempt), Some(external_invoice_id)) =
                (renewal_attempt.as_ref(), external_invoice_id.as_deref())
            {
                let invoice_data = ExternalInvoiceData {
                    realm_id: realm_id.to_string(),
                    provider: InvoiceProvider::Creem,
                    payment_provider: Some("creem".to_string()),
                    external_invoice_id: Some(external_invoice_id.to_string()),
                    external_order_id: Some(payload.event_id.clone()),
                    external_status: Some("paid".to_string()),
                    external_hosted_url: None,
                    external_pdf_url: None,
                    external_payload: Some(event.clone()),
                    tax_details: extract_creem_tax_details(object),
                    account_id: Some(user_id),
                    // Creem buyer-snapshot extraction is out of scope; left None.
                    applicant_user_id: None,
                    billing_name: None,
                    billing_email: None,
                    billing_phone: None,
                    billing_address: None,
                    currency: currency.clone(),
                    total: amount,
                    status: InvoiceStatus::Paid,
                    subscription_id: Some(subscription_id),
                    payment_attempt_id: Some(attempt.id),
                };

                if let Err(e) = app_state
                    .invoice_repository
                    .upsert_external_invoice(invoice_data)
                    .await
                {
                    warn!(
                        realm_id = %realm_id,
                        user_id = %user_id,
                        external_subscription_id = %payload.external_subscription_id,
                        event_id = %event_id,
                        error = %e,
                        "Failed to sync Creem renewal invoice -- renewal attempt recorded, cycle/points unaffected"
                    );
                }
            }
        } else {
            warn!(
                realm_id = %realm_id,
                user_id = %user_id,
                external_subscription_id = %payload.external_subscription_id,
                event_id = %event_id,
                amount = amount,
                "Creem renewal amount is zero -- skipping renewal attempt and invoice (zero-yuan cycle)"
            );
        }
    } else {
        warn!(
            realm_id = %realm_id,
            user_id = %user_id,
            external_subscription_id = %payload.external_subscription_id,
            event_id = %event_id,
            "Creem renewal payload missing amount -- skipping renewal attempt and invoice"
        );
    }

    info!(
        realm_id = %realm_id,
        user_id = %user_id,
        entitlement_key = %entitlement_key,
        event_id = %event_id,
        is_renewal = payload.is_renewal,
        normalized_period = ?normalized_period,
        "Subscription paid event processed - credit ledger created"
    );

    Ok(create_placeholder_transaction(
        user_id,
        realm_id,
        if payload.is_renewal {
            TransactionType::SubscriptionRenewal
        } else {
            TransactionType::SubscriptionGrant
        },
    ))
}

/// Handle subscription.update events
///
/// Handles subscription upgrades and downgrades.
async fn handle_subscription_updated(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_subscription_updated_payload(&event)?;
    let event_id = payload.event_id.as_str();

    // Resolve user_id: prefer payload, fall back to existing subscription
    let user_id = match payload.user_id {
        Some(uid) => uid,
        None => {
            let existing = app_state
                .billing_repository
                .find_by_external_subscription_id(&payload.external_subscription_id, "creem")
                .await?
                .ok_or_else(|| {
                    CoreError::BadRequest(format!(
                        "Cannot resolve userId: no userId in event and no existing subscription for {}",
                        payload.external_subscription_id
                    ))
                })?;
            existing.user_id
        }
    };

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing subscription.update event"
    );

    // Resolve the CURRENT (new) price-level entitlement (projection key +
    // strategy mapping) via the price-aware chain (US-EM-008).
    // Always run the resolver so the strategy mapping is price-level, killing
    // shared-key ambiguity. Creem is price-less, so the resolver maps to
    // `external_price_id IS NULL`.
    let current_resolved =
        resolve_creem_entitlement(&app_state, realm_id, &payload.external_product_id, None).await?;
    let current_entitlement_key = if payload.current_entitlement_key.is_empty() {
        current_resolved.entitlement_key.clone()
    } else {
        payload.current_entitlement_key.clone()
    };
    let new_mapping = current_resolved.mapping;

    // Fetch existing subscription once — reuse for both entitlement resolution and sync
    let existing_subscription_for_update = if payload.previous_entitlement_key.is_empty() {
        app_state
            .billing_repository
            .find_by_external_subscription_id(&payload.external_subscription_id, "creem")
            .await?
    } else {
        None
    };

    let previous_entitlement_key = if payload.previous_entitlement_key.is_empty() {
        let from_db = existing_subscription_for_update
            .as_ref()
            .map(|s| s.entitlement_key.clone())
            .unwrap_or_default();

        if from_db.is_empty() {
            // Pre-migration subscription with no entitlement_key — resolve via mapping
            resolve_creem_entitlement_key(&app_state, realm_id, &payload.external_product_id, None)
                .await?
        } else {
            from_db
        }
    } else {
        payload.previous_entitlement_key.clone()
    };

    // Resolve the PREVIOUS (old) price-level strategy mapping. The previous
    // entitlement comes from the prior subscription state (no ResolvedEntitlement
    // in scope here), so re-locate the price-level mapping by (entitlement_key,
    // price). Creem is price-less, so this maps to `external_price_id IS NULL`.
    let old_mapping = app_state
        .billing_repository
        .find_entitlement_mapping_by_key_price(realm_id, &previous_entitlement_key, None)
        .await?
        .ok_or_else(|| {
            CoreError::InternalServerError(format!(
                "Entitlement mapping not found for previous key '{}' during subscription update",
                previous_entitlement_key
            ))
        })?;

    let old_points = mapping_rule_value(&app_state, realm_id, old_mapping.id).await?;
    let new_points = mapping_rule_value(&app_state, realm_id, new_mapping.id).await?;
    let is_upgrade = new_points > old_points;

    let period_end_fallback = payload
        .current_period_end
        .unwrap_or_else(|| Utc::now() + chrono::Duration::days(30));

    let synced = sync_creem_subscription(
        &app_state,
        realm_id,
        user_id,
        payload.external_subscription_id.as_str(),
        payload.client_app_id,
        current_entitlement_key.clone(),
        payload.external_product_id,
        payload.status,
        payload.current_period_start,
        payload.current_period_end,
        payload.cancel_at_period_end,
        None,
        existing_subscription_for_update.clone(),
    )
    .await?;

    // If sync returned None we must NOT pass a nil subscription_id into the
    // upgrade/downgrade handlers — they revoke the old entitlement by
    // source_id and a nil would silently match zero rows. Fail loud instead.
    let subscription_id = match synced.as_ref() {
        Some((subscription, _)) => subscription.id,
        None => {
            tracing::warn!(
                realm_id = %realm_id,
                user_id = %user_id,
                "subscription change webhook: sync returned no subscription; skipping \
                 upgrade/downgrade revoke to avoid a nil-source_id silent no-op."
            );
            return Ok(create_placeholder_transaction(
                user_id,
                realm_id,
                TransactionType::SubscriptionDowngrade,
            ));
        }
    };

    let history_event_type = if is_upgrade {
        app_state
            .subscription_service
            .handle_subscription_upgrade(
                user_id,
                realm_id,
                subscription_id,
                &new_mapping,
                period_end_fallback,
                &payload.event_id,
            )
            .await?;
        HistoryEventType::Upgraded
    } else {
        app_state
            .subscription_service
            .handle_subscription_downgrade(
                user_id,
                subscription_id,
                realm_id,
                &old_mapping,
                &new_mapping,
            )
            .await?;
        HistoryEventType::Downgraded
    };

    if let Some((subscription, previous)) = synced {
        save_subscription_history(
            &app_state,
            previous.as_ref(),
            &subscription,
            history_event_type,
        )
        .await?;
    }

    Ok(create_placeholder_transaction(
        user_id,
        realm_id,
        if is_upgrade {
            TransactionType::SubscriptionUpgrade
        } else {
            TransactionType::SubscriptionDowngrade
        },
    ))
}

/// Handle subscription.canceled events
///
/// Handles subscription cancellation (immediate or end-of-period).
async fn handle_subscription_canceled(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_subscription_canceled_payload(&event)?;
    let event_id = payload.event_id.as_str();

    // Resolve user_id: prefer payload, fall back to existing subscription.
    // When falling back, keep the fetched subscription to avoid a redundant DB query
    // in sync_creem_subscription below.
    let (user_id, existing_subscription) = match payload.user_id {
        Some(uid) => (uid, None),
        None => {
            let existing = app_state
                .billing_repository
                .find_by_external_subscription_id(&payload.external_subscription_id, "creem")
                .await?
                .ok_or_else(|| {
                    CoreError::BadRequest(format!(
                        "Cannot resolve userId: no userId in event and no existing subscription for {}",
                        payload.external_subscription_id
                    ))
                })?;
            let uid = existing.user_id;
            (uid, Some(existing))
        }
    };

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing subscription.canceled event"
    );

    let (cancel_mode, period_end, cancel_at) = if payload.cancel_at_period_end {
        let period_end = payload.current_period_end.ok_or_else(|| {
            CoreError::BadRequest("Missing currentPeriodEnd for cancel at period end".to_string())
        })?;

        info!(
            realm_id = %realm_id,
            user_id = %user_id,
            period_end = %period_end,
            event_id = %event_id,
            "Subscription cancel at period end - setting expiration"
        );

        (
            CancelMode::DefaultCancel,
            Some(period_end),
            Some(period_end),
        )
    } else {
        info!(
            realm_id = %realm_id,
            user_id = %user_id,
            event_id = %event_id,
            "Subscription immediate cancel - revoking subscription credits"
        );

        (CancelMode::ImmediateCancel, None, Some(Utc::now()))
    };

    // Resolve entitlement_key early for targeted revocation (price-less)
    let entitlement_key = if let Some(key) = &payload.entitlement_key {
        if !key.is_empty() {
            key.clone()
        } else {
            resolve_creem_entitlement_key(&app_state, realm_id, &payload.external_product_id, None)
                .await?
        }
    } else {
        resolve_creem_entitlement_key(&app_state, realm_id, &payload.external_product_id, None)
            .await?
    };

    let synced = sync_creem_subscription(
        &app_state,
        realm_id,
        user_id,
        payload.external_subscription_id.as_str(),
        payload.client_app_id,
        entitlement_key.clone(),
        payload.external_product_id,
        payload.status,
        payload.current_period_start,
        payload.current_period_end,
        payload.cancel_at_period_end,
        cancel_at,
        existing_subscription,
    )
    .await?;

    // entitlement by `source_id = subscription_id`. No ledger-row reclaim; the
    // quota model. Idempotent: no active entitlement ⟹ no-op.
    //
    // PRD §4.1: a missed role/quota revoke on a subscription cancel is a P0
    // fault ("漏撤视为 P0 故障"). Passing `Uuid::nil()` as the source_id would
    // match zero rows and silently skip the revoke. Instead, when sync returned
    // None we fail loud: log a warning and skip the (no-op) call so the
    // compensation framework / retry sweep can intervene, rather than masking
    // the miss as an idempotent success.
    if let Some(ref synced_pair) = synced {
        let subscription_id = synced_pair.0.id;
        let _output = app_state
            .subscription_service
            .handle_subscription_cancel(
                user_id,
                realm_id,
                subscription_id,
                cancel_mode,
                period_end,
                Some(&entitlement_key),
            )
            .await?;
    } else {
        tracing::warn!(
            realm_id = %realm_id,
            user_id = %user_id,
            entitlement_key = %entitlement_key,
            external_subscription_id = %payload.external_subscription_id,
            "subscription cancel webhook: sync returned no subscription; skipping role/quota \
             revoke to avoid a nil-source_id silent no-op. The compensation/retry sweep must \
             reconcile this entitlement. (PRD §4.1: missed revoke = P0 fault)"
        );
    }

    if let Some((subscription, previous)) = synced {
        save_subscription_history(
            &app_state,
            previous.as_ref(),
            &subscription,
            HistoryEventType::Canceled,
        )
        .await?;
    }

    Ok(create_placeholder_transaction(
        user_id,
        realm_id,
        TransactionType::CancelRevoke,
    ))
}

/// Handle refund.created events
///
/// Revokes unused points based on refund type (topup vs subscription).
async fn handle_refund_created(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_refund_created_payload(&event)?;
    let event_id = payload.event_id.as_str();

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing refund.created event"
    );

    info!(
        realm_id = %realm_id,
        refund_id = %payload.refund_id,
        payment_id = %payload.payment_id,
        amount = payload.amount,
        original_amount = payload.original_amount,
        refund_type = %payload.refund_type,
        user_id = %payload.user_id,
        event_id = %event_id,
        "Processing refund - revoking points"
    );

    // Resolve the originating payment_attempt so its id can anchor the
    // rule-attributed revoke (revocation targets the same source the original
    // grant was attributed to). Look up by provider reference (Creem
    // payment_id). When no attempt snapshot exists, fail loud rather than
    // revoke from an arbitrary implicit pool — over-revoking unrelated credits
    // would be a silent bug.
    let attempt = app_state
        .payment_attempt_service
        .get_payment_attempt_by_provider_reference("creem", &payload.payment_id)
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                payment_id = %payload.payment_id,
                error = %e,
                "Failed to look up payment_attempt for refund bucket resolution"
            );
            CoreError::InternalServerError(format!(
                "Failed to resolve bucket for refund {}: {e}",
                payload.refund_id
            ))
        })?
        .ok_or_else(|| {
            CoreError::BadRequest(format!(
                "Cannot resolve bucket for refund {}: no payment_attempt for payment_id {}",
                payload.refund_id, payload.payment_id
            ))
        })?;
    match payload.refund_type.as_str() {
        "topup" => {
            let _output = app_state
                .points_service
                .revoke_topup_source_proportional(
                    realm_id,
                    payload.user_id,
                    &attempt.id.to_string(),
                    payload.amount,
                    payload.original_amount,
                    &payload.refund_id,
                )
                .await?;

            // Revoke payment-granted permanent roles for this one-time attempt
            // `source_id = attempt.id`, so revoke with the same source id.
            // Idempotent (NotFound is a no-op); manual grants unaffected.
            revoke_payment_roles_for_source(
                &app_state,
                realm_id,
                payload.user_id,
                &attempt.id.to_string(),
            )
            .await;

            info!(
                realm_id = %realm_id,
                user_id = %payload.user_id,
                refund_id = %payload.refund_id,
                amount = payload.amount,
                original_amount = payload.original_amount,
                "Topup refund - proportionally revoked topup credits"
            );
        }
        _ => {
            // subscription's active quota entitlement by `source_id =
            // broad `revoke_subscription_unused` ledger-row paths are retired
            // under the window quota model. A refund targets the originating
            // subscription; resolve it by external_subscription_id from the
            // event payload and verify it belongs to the routing bucket resolved
            // from the original payment attempt. Idempotent: no active
            // entitlement / already-revoked ⟹ no-op.
            let external_subscription_id =
                payload.external_subscription_id.as_deref().ok_or_else(|| {
                    CoreError::BadRequest(
                        "Missing subscriptionId in subscription refund payload".to_string(),
                    )
                })?;
            let subscription =
                resolve_existing_creem_subscription(&app_state, external_subscription_id)
                    .await?
                    .ok_or_else(|| {
                        CoreError::BadRequest(format!(
                            "No subscription found for refund {}: external_subscription_id {}",
                            payload.refund_id, external_subscription_id
                        ))
                    })?;

            let _output = app_state
                .subscription_service
                .handle_subscription_cancel(
                    payload.user_id,
                    realm_id,
                    subscription.id,
                    CancelMode::ImmediateCancel,
                    None,
                    None,
                )
                .await?;

            // Mirror Stripe's charge.refunded handling: record a subscription
            // history event so provider behavior stays symmetric. Topup refunds
            // have no subscription and skip this.
            let history_event = SubscriptionHistoryService::create_subscription_refunded_event(
                &subscription,
                serde_json::json!({
                    "provider": "creem",
                    "refundId": payload.refund_id,
                    "amountRefunded": payload.amount,
                    "originalAmount": payload.original_amount,
                    "refundType": payload.refund_type,
                }),
                Some(ACTOR_WEBHOOK.to_string()),
            );
            app_state
                .billing_repository
                .save_history_event(history_event)
                .await?;

            info!(
                realm_id = %realm_id,
                user_id = %payload.user_id,
                refund_id = %payload.refund_id,
                subscription_id = %subscription.id,
                "Subscription refund - revoked subscription quota entitlement"
            );
        }
    }

    Ok(create_placeholder_transaction(
        payload.user_id,
        realm_id,
        TransactionType::RefundRevoke,
    ))
}

async fn resolve_existing_creem_subscription(
    app_state: &AppState,
    external_subscription_id: &str,
) -> Result<Option<Subscription>, CoreError> {
    app_state
        .billing_repository
        .find_by_external_subscription_id(external_subscription_id, "creem")
        .await
}

async fn handle_subscription_lifecycle_status(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    status: SubscriptionStatus,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_subscription_lifecycle_payload(&event)?;
    let event_id = payload.event_id.as_str();
    let existing =
        resolve_existing_creem_subscription(&app_state, &payload.external_subscription_id).await?;
    let user_id = payload
        .user_id
        .or_else(|| existing.as_ref().map(|subscription| subscription.user_id))
        .ok_or_else(|| {
            CoreError::BadRequest(format!(
                "Cannot resolve userId for subscription {}",
                payload.external_subscription_id
            ))
        })?;
    let entitlement_key = if let Some(key) = payload.entitlement_key {
        key
    } else if let Some(subscription) = &existing {
        subscription.entitlement_key.clone()
    } else {
        resolve_creem_entitlement_key(&app_state, realm_id, &payload.external_product_id, None)
            .await?
    };
    let external_product_id = if payload.external_product_id.is_empty() {
        existing
            .as_ref()
            .map(|subscription| subscription.external_product_id.clone())
            .unwrap_or_default()
    } else {
        payload.external_product_id
    };
    let cancel_at_period_end = status == SubscriptionStatus::ScheduledCancel;
    let cancel_at = if cancel_at_period_end {
        payload.current_period_end
    } else {
        None
    };

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        external_subscription_id = %payload.external_subscription_id,
        status = %status.as_str(),
        "Processing Creem subscription lifecycle event"
    );

    let synced = sync_creem_subscription(
        &app_state,
        realm_id,
        user_id,
        &payload.external_subscription_id,
        payload.client_app_id,
        entitlement_key.clone(),
        external_product_id,
        status.clone(),
        payload.current_period_start,
        payload.current_period_end,
        cancel_at_period_end,
        cancel_at,
        existing,
    )
    .await?;

    if let Some((subscription, previous)) = synced.as_ref() {
        let history_event = match previous {
            Some(prev) => detect_change_type(prev, subscription),
            None => HistoryEventType::Created,
        };

        save_subscription_history(&app_state, previous.as_ref(), subscription, history_event)
            .await?;
    }

    // terminal lifecycle state; revoke the active quota entitlement
    // immediately so the user's window availability drops to zero. This is
    // idempotent because `revoke_quota_entitlement` is keyed by
    // (realm_id, user_id, bucket_id, credit_type, source_id).
    //
    // PRD §4.1: a missed revoke is a P0 fault. If sync returned no
    // subscription we must NOT fall back to `Uuid::nil()` (that would match
    // zero rows and silently skip the revoke). Instead fail loud so the
    // compensation/retry sweep reconciles.
    if status == SubscriptionStatus::Expired {
        match synced.as_ref() {
            Some((subscription, _)) => {
                let _output = app_state
                    .subscription_service
                    .handle_subscription_cancel(
                        user_id,
                        realm_id,
                        subscription.id,
                        CancelMode::ImmediateCancel,
                        None,
                        Some(&entitlement_key),
                    )
                    .await?;
                info!(
                    realm_id = %realm_id,
                    user_id = %user_id,
                    entitlement_key = %entitlement_key,
                    subscription_id = %subscription.id,
                    "Subscription expired - revoked active quota entitlement (window quota model)"
                );
            }
            None => {
                tracing::warn!(
                    realm_id = %realm_id,
                    user_id = %user_id,
                    entitlement_key = %entitlement_key,
                    external_subscription_id = %payload.external_subscription_id,
                    "subscription.expired webhook: sync returned no subscription; skipping \
                     role/quota revoke to avoid a nil-source_id silent no-op. The \
                     compensation/retry sweep must reconcile. (PRD §4.1: missed revoke = P0 fault)"
                );
            }
        }
    }

    Ok(create_placeholder_transaction(
        user_id,
        realm_id,
        TransactionType::SubscriptionGrant,
    ))
}

async fn handle_dispute_created(
    app_state: AppState,
    event: Value,
    realm_id: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_dispute_created_payload(&event)?;
    let event_id = payload.event_id.as_str();
    let existing =
        resolve_existing_creem_subscription(&app_state, &payload.external_subscription_id).await?;
    let existing = existing.ok_or_else(|| {
        CoreError::BadRequest(format!(
            "Cannot resolve disputed subscription {}",
            payload.external_subscription_id
        ))
    })?;
    let user_id = existing.user_id;
    let provider_metadata = serde_json::json!({
        "disputeId": payload.dispute_id,
        "amount": payload.amount,
        "currency": payload.currency,
    });

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        external_subscription_id = %payload.external_subscription_id,
        dispute_id = %payload.dispute_id,
        "Processing Creem dispute.created event"
    );

    if let Some((subscription, previous)) = sync_subscription(
        &app_state,
        SyncSubscriptionInput {
            provider: "creem",
            realm_id: realm_id.to_string(),
            user_id: Some(user_id),
            external_subscription_id: payload.external_subscription_id,
            external_product_id: if payload.external_product_id.is_empty() {
                existing.external_product_id.clone()
            } else {
                payload.external_product_id
            },
            client_app_id: existing.client_app_id,
            entitlement_key: existing.entitlement_key.clone(),
            external_price_id: existing.external_price_id.clone(),
            provider_metadata: Some(provider_metadata),
            status: SubscriptionStatus::Dispute,
            current_period_start: existing.current_period_start,
            current_period_end: existing.current_period_end,
            cancel_at_period_end: existing.cancel_at_period_end,
            cancel_at: existing.cancel_at,
            existing_subscription: Some(existing),
        },
    )
    .await?
    {
        save_subscription_history(
            &app_state,
            previous.as_ref(),
            &subscription,
            HistoryEventType::Disputed,
        )
        .await?;
    }

    Ok(create_placeholder_transaction(
        user_id,
        realm_id,
        TransactionType::SubscriptionGrant,
    ))
}

/// Canonical Creem event-type routing function.
///
/// Called from both the normal webhook handler and `reprocess_creem_event`
/// so that adding a new event type only requires updating one place.
async fn process_creem_event_once(
    app_state: AppState,
    event: &Value,
    realm_id: &str,
    idempotency_key: &str,
    event_id: &str,
    event_type: &str,
) -> Result<PointsTransaction, CoreError> {
    match event_type {
        "checkout.completed" => {
            handle_checkout_completed(app_state.clone(), event.clone(), realm_id, idempotency_key)
                .await
        }
        "subscription.paid" => {
            handle_subscription_paid(app_state.clone(), event.clone(), realm_id, idempotency_key)
                .await
        }
        "subscription.update" => {
            handle_subscription_updated(app_state.clone(), event.clone(), realm_id, idempotency_key)
                .await
        }
        "subscription.canceled" => {
            handle_subscription_canceled(
                app_state.clone(),
                event.clone(),
                realm_id,
                idempotency_key,
            )
            .await
        }
        "subscription.active" => {
            handle_subscription_lifecycle_status(
                app_state.clone(),
                event.clone(),
                realm_id,
                SubscriptionStatus::Active,
            )
            .await
        }
        "subscription.trialing" => {
            handle_subscription_lifecycle_status(
                app_state.clone(),
                event.clone(),
                realm_id,
                SubscriptionStatus::Trialing,
            )
            .await
        }
        "subscription.paused" => {
            handle_subscription_lifecycle_status(
                app_state.clone(),
                event.clone(),
                realm_id,
                SubscriptionStatus::Paused,
            )
            .await
        }
        "subscription.past_due" => {
            handle_subscription_lifecycle_status(
                app_state.clone(),
                event.clone(),
                realm_id,
                SubscriptionStatus::PastDue,
            )
            .await
        }
        "subscription.scheduled_cancel" => {
            handle_subscription_lifecycle_status(
                app_state.clone(),
                event.clone(),
                realm_id,
                SubscriptionStatus::ScheduledCancel,
            )
            .await
        }
        "subscription.expired" => {
            handle_subscription_lifecycle_status(
                app_state.clone(),
                event.clone(),
                realm_id,
                SubscriptionStatus::Expired,
            )
            .await
        }
        "refund.created" => {
            handle_refund_created(app_state.clone(), event.clone(), realm_id, idempotency_key).await
        }
        "dispute.created" => {
            handle_dispute_created(app_state.clone(), event.clone(), realm_id).await
        }
        _ => {
            warn!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                "Unknown Creem event type - ignoring"
            );
            Ok(create_placeholder_transaction(
                Uuid::now_v7(),
                realm_id,
                TransactionType::SubscriptionGrant,
            ))
        }
    }
}

/// Handle Creem webhook events
///
/// Verifies signature, checks idempotency, routes to appropriate handler,
/// and returns 202 Accepted immediately. Processing happens asynchronously.
#[tracing::instrument(
    // Governance: `body` is the raw provider payload
    // (Creem event bodies may carry PII / customer data); `headers` carries
    // the `creem-signature` header; `realm_id` is conservatively skipped.
    // Only the low-cardinality route template is recorded.
    skip(app_state, realm_id, headers, body),
    fields(http.route = "/api/billing/webhook")
)]
pub async fn handle_creem_webhook(
    State(app_state): State<AppState>,
    Path(realm_id): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Result<StatusCode, CoreError> {
    let event: Value = serde_json::from_str(&body).map_err(|e| {
        error!("Failed to parse webhook JSON: {}", e);
        CoreError::BadRequest(format!("Invalid JSON: {}", e))
    })?;

    let signature = headers
        .get("creem-signature")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            error!("Missing creem-signature header");
            CoreError::BadRequest("Missing signature".to_string())
        })?;

    let webhook_secret = app_state
        .realm_config_repository
        .get(
            realm_id.to_string(),
            "creem".to_string(),
            "webhook_secret".to_string(),
        )
        .await
        .map_err(|e| {
            error!("Failed to load webhook secret from database: {}", e);
            CoreError::InternalServerError(format!("Database error: {}", e))
        })?
        .filter(|c| c.enabled && !c.config_value.trim().is_empty())
        .map(|c| c.config_value)
        .ok_or_else(|| {
            error!(
                realm_id = %realm_id,
                "Webhook secret not found in database"
            );
            CoreError::InternalServerError(format!(
                "Webhook secret not configured for realm: {}",
                realm_id
            ))
        })?;

    verify_webhook_signature(body.as_bytes(), signature, &webhook_secret)?;

    let event_id = parse_event_id(&event)?;
    let event_type = event["eventType"]
        .as_str()
        .ok_or_else(|| CoreError::BadRequest("Missing eventType".to_string()))?
        .to_string();

    let new_payment_event = PaymentEvent {
        id: Uuid::now_v7(),
        realm_id: realm_id.clone(),
        external_event_id: event_id.clone(),
        payment_provider: "creem".to_string(),
        event_type: event_type.clone(),
        subscription_id: None,
        payload: event.clone(),
        processed: false,
        processing_started_at: None,
        created_at: Utc::now(),
    };

    if app_state
        .billing_repository
        .find_payment_event_by_external_id(&realm_id, &event_id, "creem")
        .await?
        .is_some()
    {
        info!(
            realm_id = %realm_id,
            event_id = %event_id,
            event_type = %event_type,
            "Duplicate webhook event - returning OK"
        );
        return Ok(StatusCode::OK);
    }

    let saved_event = match app_state
        .billing_repository
        .create_payment_event(new_payment_event)
        .await
    {
        Ok(saved_event) => saved_event,
        Err(CoreError::DatabaseError(msg))
            if msg.contains("unique constraint") || msg.contains("duplicate key") =>
        {
            info!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                "Concurrent webhook event already inserted - returning OK"
            );
            return Ok(StatusCode::OK);
        }
        Err(e) => return Err(e),
    };

    let idempotency_key = format!("creem_{}", event_id);
    let idempotency_service = &app_state.idempotency_service;

    let idempotency_result = idempotency_service
        .check_or_create(&realm_id, &idempotency_key, &body)
        .await?;

    if let IdempotencyResult::Cached {
        transaction: PointsTransaction {
            id: transaction_id, ..
        },
    } = idempotency_result
    {
        let _ = app_state
            .billing_repository
            .mark_payment_event_processed(saved_event.id)
            .await;

        info!(
            realm_id = %realm_id,
            event_id = %event_id,
            event_type = %event_type,
            transaction_id = %transaction_id,
            "Returning cached result for duplicate webhook event"
        );
        return Ok(StatusCode::OK);
    }

    let result = process_creem_event_once(
        app_state.clone(),
        &event,
        &realm_id,
        &idempotency_key,
        &event_id,
        &event_type,
    )
    .await;

    match result {
        Ok(transaction) => {
            if let Err(e) = idempotency_service
                .save_result(&realm_id, &idempotency_key, &transaction)
                .await
            {
                error!(
                    realm_id = %realm_id,
                    idempotency_key = %idempotency_key,
                    error = %e,
                    "Failed to save idempotency result"
                );
            }

            let _ = app_state
                .billing_repository
                .mark_payment_event_processed(saved_event.id)
                .await;
        }
        Err(e) => {
            error!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                error = %e,
                "Webhook handler failed"
            );

            let _ = idempotency_service
                .mark_failed(&realm_id, &idempotency_key)
                .await;

            return Err(e);
        }
    }

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        event_type = %event_type,
        "Webhook processed successfully"
    );

    Ok(StatusCode::OK)
}

/// Reprocess a single Creem event that Herald missed (compensation path).
///
/// Unlike the normal webhook flow, this:
/// - Skips Redis idempotency checks entirely
/// - Skips signature verification (event comes from Creem API, not webhook)
/// - Uses DB `payment_event` for idempotency only
/// - Reuses the same match routing via `process_creem_event_once`
pub(crate) async fn reprocess_creem_event(
    app_state: AppState,
    realm_id: &str,
    event: &Value,
    event_type: &str,
) -> Result<(), CoreError> {
    let event_id = parse_event_id(event)?;

    if let Some(existing) = app_state
        .billing_repository
        .find_payment_event_by_external_id(realm_id, &event_id, "creem")
        .await?
    {
        if existing.processed {
            info!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                "Creem compensation: event already processed, skipping"
            );
            return Ok(());
        }
        // Exists but not processed -- inconsistent state, do not reprocess
        error!(
            realm_id = %realm_id,
            event_id = %event_id,
            event_type = %event_type,
            "Creem compensation: event exists but processed=false, skipping inconsistent event"
        );
        return Ok(());
    }

    let new_payment_event = PaymentEvent {
        id: Uuid::now_v7(),
        realm_id: realm_id.to_string(),
        external_event_id: event_id.clone(),
        payment_provider: "creem".to_string(),
        event_type: event_type.to_string(),
        subscription_id: None,
        payload: event.clone(),
        processed: false,
        processing_started_at: None,
        created_at: Utc::now(),
    };

    let saved_event = match app_state
        .billing_repository
        .create_payment_event(new_payment_event)
        .await
    {
        Ok(event) => event,
        Err(CoreError::DatabaseError(ref msg))
            if msg.contains("unique constraint") || msg.contains("duplicate key") =>
        {
            info!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                "Creem compensation: concurrent insert detected, event already handled"
            );
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    // Synthetic idempotency key (not used for Redis, only passed to handlers)
    let idempotency_key = format!("compensation_creem_{}", event_id);

    // Route to the same handler match branches via shared routing function
    let result = process_creem_event_once(
        app_state.clone(),
        event,
        realm_id,
        &idempotency_key,
        &event_id,
        event_type,
    )
    .await;

    // Compensation payloads built from REST API lack webhook metadata.
    // When the handler fails with a BadRequest indicating a missing field (e.g.
    // clientAppId, entitlementKey), treat it as a best-effort skip rather than
    // a hard failure that would inflate the failed-event counter.
    let result = match result {
        Err(CoreError::BadRequest(ref msg)) if msg.contains("Missing or invalid") => {
            warn!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                error = %msg,
                "Creem compensation: skipping event due to missing metadata (expected for REST API compensation)"
            );
            // Mark the saved event as processed so we don't retry it indefinitely.
            let _ = app_state
                .billing_repository
                .mark_payment_event_processed(saved_event.id)
                .await;
            return Ok(());
        }
        other => other,
    };

    match result {
        Ok(_transaction) => {
            if let Err(e) = app_state
                .billing_repository
                .mark_payment_event_processed(saved_event.id)
                .await
            {
                tracing::error!(
                    realm_id = %realm_id,
                    event_id = %event_id,
                    event_type = %event_type,
                    error = %e,
                    "Creem compensation: handler succeeded but failed to mark payment_event as processed — event may be reprocessed on next run"
                );
            } else {
                tracing::info!(
                    realm_id = %realm_id,
                    event_id = %event_id,
                    event_type = %event_type,
                    "Creem compensation: event reprocessed successfully"
                );
            }
            Ok(())
        }
        Err(e) => {
            error!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                error = %e,
                "Creem compensation: failed to reprocess event"
            );
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_creem_billing_type_maps_known_variants() {
        assert_eq!(
            normalize_creem_billing_type("onetime"),
            BillingType::OneTime
        );
        assert_eq!(
            normalize_creem_billing_type("one_time"),
            BillingType::OneTime
        );
        assert_eq!(
            normalize_creem_billing_type("recurring"),
            BillingType::Recurring
        );
        assert_eq!(
            normalize_creem_billing_type("subscription"),
            BillingType::Recurring
        );
    }

    #[test]
    fn normalize_creem_billing_type_case_insensitive() {
        assert_eq!(
            normalize_creem_billing_type("OneTime"),
            BillingType::OneTime
        );
        assert_eq!(
            normalize_creem_billing_type("ONETIME"),
            BillingType::OneTime
        );
    }

    #[test]
    fn normalize_creem_billing_type_unknown_defaults_recurring() {
        assert_eq!(
            normalize_creem_billing_type("unknown"),
            BillingType::Recurring
        );
        assert_eq!(normalize_creem_billing_type(""), BillingType::Recurring);
    }

    #[test]
    fn parse_checkout_completed_extracts_attempt_id() {
        let event: Value = serde_json::json!({
            "id": "evt_test",
            "object": {
                "metadata": {
                    "herald_client_app_id": "00000000-0000-0000-0000-000000000001",
                    "herald_entitlement_key": "test-key",
                    "attemptId": "11111111-1111-1111-1111-111111111111"
                },
                "product": { "id": "prod_123" }
            }
        });

        let payload = parse_checkout_completed_payload(&event).unwrap();
        assert_eq!(
            payload.attempt_id,
            Some(Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap())
        );
    }

    #[test]
    fn parse_checkout_completed_no_attempt_id_returns_none() {
        let event: Value = serde_json::json!({
            "id": "evt_test",
            "object": {
                "metadata": {
                    "herald_client_app_id": "00000000-0000-0000-0000-000000000001",
                    "herald_entitlement_key": "test-key"
                },
                "product": { "id": "prod_123" }
            }
        });

        let payload = parse_checkout_completed_payload(&event).unwrap();
        assert!(payload.attempt_id.is_none());
    }

    #[test]
    fn parse_checkout_completed_nil_attempt_id_treated_as_absent() {
        let event: Value = serde_json::json!({
            "id": "evt_test",
            "object": {
                "metadata": {
                    "herald_client_app_id": "00000000-0000-0000-0000-000000000001",
                    "herald_entitlement_key": "test-key",
                    "attemptId": "00000000-0000-0000-0000-000000000000"
                },
                "product": { "id": "prod_123" }
            }
        });

        let payload = parse_checkout_completed_payload(&event).unwrap();
        assert!(payload.attempt_id.is_none());
    }

    // Creem exposes the billing period under several field-name variants.
    // The normalizer must resolve all of them or return None (P0: skip +
    // warn, never guess).

    fn creem_ts(value: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(value, 0).unwrap()
    }

    #[test]
    fn normalize_creem_period_camel_case_resolved() {
        let obj = serde_json::json!({
            "currentPeriodStart": "2023-11-14T22:13:20Z",
            "currentPeriodEnd": "2023-12-14T22:13:20Z",
        });
        let got = normalize_creem_period(&obj).expect("camelCase period must resolve");
        assert_eq!(got.0, creem_ts(1_700_000_000));
        assert_eq!(got.1, creem_ts(1_700_000_000 + 2_592_000));
    }

    #[test]
    fn normalize_creem_period_snake_case_resolved() {
        let obj = serde_json::json!({
            "current_period_start": "2023-11-14T22:13:20Z",
            "current_period_end": "2023-12-14T22:13:20Z",
        });
        let got = normalize_creem_period(&obj).expect("snake_case period must resolve");
        assert_eq!(got.0, creem_ts(1_700_000_000));
    }

    #[test]
    fn normalize_creem_period_date_variants_resolved() {
        let obj = serde_json::json!({
            "current_period_start_date": "2023-11-14T22:13:20Z",
            "current_period_end_date": "2023-12-14T22:13:20Z",
        });
        let got = normalize_creem_period(&obj).expect("date-variant period must resolve");
        assert_eq!(got.0, creem_ts(1_700_000_000));
    }

    #[test]
    fn normalize_creem_period_missing_is_none() {
        let obj = serde_json::json!({ "subscriptionId": "sub_1" });
        assert!(
            normalize_creem_period(&obj).is_none(),
            "absent Creem period must NOT be resolved (P0)"
        );
    }

    #[test]
    fn normalize_creem_period_partial_is_none() {
        // Only start present — cannot form a valid window.
        let obj = serde_json::json!({
            "currentPeriodStart": "2023-11-14T22:13:20Z",
        });
        assert!(
            normalize_creem_period(&obj).is_none(),
            "partial Creem period must NOT be resolved (P0)"
        );
    }

    #[test]
    fn normalize_creem_period_inverted_is_none() {
        let obj = serde_json::json!({
            "currentPeriodStart": "2023-12-14T22:13:20Z",
            "currentPeriodEnd": "2023-11-14T22:13:20Z",
        });
        assert!(normalize_creem_period(&obj).is_none());
    }
}

// Governance tests.
// Covers: billing `handle_creem_webhook` (webhook_handlers.rs) and
// `handle_stripe_webhook` (stripe_webhook_handlers.rs) instrument skip
// correctness.
// WHY: webhook `body` is the raw provider payload (may carry PII / customer
// data) and `headers` carry the provider signature header. If the
// `#[instrument]` macro ever stops skipping those, raw PII / the signature
// leaks into a span field. Source-scan baseline, anchored per
// function to the immediately-preceding `#[tracing::instrument(...)]`.
#[cfg(test)]
mod instrument_skip_tests {
    const CREEM_SRC: &str = include_str!("webhook_handlers.rs");
    const STRIPE_SRC: &str = include_str!("stripe_webhook_handlers.rs");

    fn instrument_body_preceding(src: &str, fn_name: &str) -> String {
        let needle = format!("fn {fn_name}");
        let fn_pos = src
            .find(&needle)
            .unwrap_or_else(|| panic!("fn {fn_name} not found in source"));
        let attr_start = src[..fn_pos]
            .rfind("#[tracing::instrument(")
            .unwrap_or_else(|| panic!("no #[tracing::instrument( preceding fn {fn_name}"));
        let body_start = attr_start + "#[tracing::instrument(".len();
        // Find the attribute close: the first line at/after body_start whose
        // trimmed content is exactly `)]`. This handles indented closes (e.g.
        // inside an `impl` block) and ignores inline `))]` sequences such as
        // `#[validate(length(...))]` that appear on struct fields.
        let tail = &src[body_start..];
        let mut consumed = 0usize;
        for line in tail.lines() {
            let prev = consumed;
            consumed += line.len() + 1; // +1 for the line separator
            if line.trim() == ")]" {
                return tail[..prev].to_string();
            }
        }
        panic!("unterminated #[tracing::instrument( for fn {fn_name}")
    }

    #[test]
    fn instrument_skip_billing_creem_webhook_excludes_body_headers_secret() {
        let body = instrument_body_preceding(CREEM_SRC, "handle_creem_webhook");
        for required in ["body", "headers", "realm_id"] {
            assert!(
                body.contains(required),
                "handle_creem_webhook must skip `{required}`; body was:\n{body}"
            );
        }
        for banned in [
            "token", "password", "email", "secret", "payload", "raw_body",
        ] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "handle_creem_webhook span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_billing_stripe_webhook_excludes_body_headers_secret() {
        let body = instrument_body_preceding(STRIPE_SRC, "handle_stripe_webhook");
        for required in ["body", "headers", "realm_id"] {
            assert!(
                body.contains(required),
                "handle_stripe_webhook must skip `{required}`; body was:\n{body}"
            );
        }
        for banned in [
            "token", "password", "email", "secret", "payload", "raw_body",
        ] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "handle_stripe_webhook span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }
}
