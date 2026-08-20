// Handles subscription lifecycle events (checkout.session.completed, customer.subscription.*)
// and payment events (charge.refunded). All handlers follow the Creem webhook pattern.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::Value;
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::webhook_common::{
    create_placeholder_transaction, metadata_user_id, metadata_value, parse_attempt_id,
    parse_event_id, parse_optional_uuid_field, parse_uuid_field,
    revoke_payment_roles_for_source as revoke_payment_roles_for_attempt,
};
use crate::webhook_subscription_helpers::{
    ResolvedEntitlement, SyncSubscriptionInput, mapping_rule_value, resolve_entitlement_mapping,
    save_subscription_history_in_txn, sync_subscription_in_txn,
};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::billing::credit_note::{
    CreditNoteRepository, CreditNoteSource, CreditNoteStatus, NewCreditNote,
};
use herald_core::domain::billing::invoice_service::map_stripe_invoice_status;
use herald_core::domain::billing::{
    ACTOR_WEBHOOK, BillingRepository, BillingType, ExternalInvoiceData, HistoryEventType,
    InvoiceProvider, InvoiceRepository, InvoiceStatus, PaymentEvent, Subscription,
    SubscriptionHistoryService, SubscriptionStatus, detect_change_type,
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::payment_attempt::{PaymentAttemptStatus, RecordRenewalAttemptInput};
use herald_core::domain::points::IdempotencyResult;
use herald_core::domain::points::entities::{
    PointsRevocationRecord, PointsTransaction, RevocationType, TransactionType,
};
use herald_core::domain::points::ports::PointsRepository;
use herald_core::domain::points::subscription_service::CancelMode;
use herald_core::domain::purchase::metadata_keys;
use herald_core::domain::realm_config::RealmConfigRepository;

struct StripeCheckoutCompletedPayload {
    event_id: String,
    user_id: Uuid,
    client_app_id: Uuid,
    entitlement_key: String,
    is_trial: bool,
    stripe_subscription_id: Option<String>,
    stripe_product_id: String,
}

struct StripeSubscriptionCreatedPayload {
    event_id: String,
    stripe_subscription_id: String,
    user_id: Uuid,
    entitlement_key: String,
    client_app_id: Option<Uuid>,
    external_product_id: String,
    cancel_at_period_end: bool,
    current_period_start: Option<DateTime<Utc>>,
    // Optional: Stripe omits current_period_end on the first
    // customer.subscription.created event for checkout-initiated subscriptions
    // while the subscription is still `incomplete` (Stripe sets it only after the
    // first invoice is paid). Treating it as required aborts the handler before
    // credits are granted.
    current_period_end: Option<DateTime<Utc>>,
    status: SubscriptionStatus,
}

struct StripeSubscriptionUpdatedPayload {
    event_id: String,
    stripe_subscription_id: String,
    user_id: Uuid,
    previous_entitlement_key: String,
    current_entitlement_key: String,
    external_product_id: String,
    cancel_at_period_end: bool,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
    status: SubscriptionStatus,
}

struct StripeSubscriptionDeletedPayload {
    event_id: String,
    stripe_subscription_id: String,
    user_id: Uuid,
    entitlement_key: Option<String>,
    cancel_at_period_end: bool,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
}

struct StripeChargeRefundedPayload {
    event_id: String,
    charge_id: String,
    amount: i64,
    amount_refunded: i64,
    user_id: Uuid,
    subscription_id: Option<Uuid>,
    refund_type: String,
}

struct StripeInvoicePaidPayload {
    event_id: String,
    stripe_subscription_id: String,
    user_id: Uuid,
    entitlement_key: String,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
}

struct StripePaymentIntentSucceededPayload {
    attempt_id: Uuid,
    payment_intent_id: String,
    completed_at: DateTime<Utc>,
}

struct StripePaymentFailedPayload {
    attempt_id: Uuid,
    provider_reference: String,
    provider_status: String,
    completed_at: DateTime<Utc>,
}

fn parse_stripe_datetime(value: &Value, field_name: &str) -> Result<DateTime<Utc>, CoreError> {
    if let Some(timestamp) = value.as_i64() {
        return DateTime::<Utc>::from_timestamp(timestamp, 0).ok_or_else(|| {
            CoreError::BadRequest(format!("Invalid unix timestamp for {}", field_name))
        });
    }

    if let Some(value) = value.as_str() {
        return DateTime::parse_from_rfc3339(value)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| {
                CoreError::BadRequest(format!("Invalid RFC3339 timestamp for {}", field_name))
            });
    }

    Err(CoreError::BadRequest(format!(
        "Missing or invalid {}",
        field_name
    )))
}

fn parse_optional_stripe_datetime(value: &Value) -> Result<Option<DateTime<Utc>>, CoreError> {
    if value.is_null() {
        return Ok(None);
    }

    parse_stripe_datetime(value, "timestamp").map(Some)
}

/// Normalize a Stripe subscription's billing period to a unique
/// `(period_start, period_end)` pair (P0).
///
/// Stripe 2025-03-31.basil removed the top-level `current_period_start` /
/// `current_period_end` fields from the subscription object; the period now
/// lives on each subscription item (`items.data[].current_period_*`). Older
/// API versions still expose the top-level fields. This function reconciles
/// both shapes.
///
/// Resolution order:
/// 1. **Item-level** (`items.data[].current_period_start/end`):
///    - Single item with period fields → use it.
///    - Multiple items all sharing the same period → use the shared period.
///    - Multiple items with disagreeing periods → cannot uniquely map the
///      points entitlement's item → `None` (P0: do not guess).
/// 2. **Top-level fallback** (`current_period_start/end`) — old API versions.
/// 3. Missing / unparseable / `start >= end` → `None`.
///
/// Returns `None` whenever the period cannot be uniquely resolved; the caller
/// must then skip pre-grant, emit a structured warning, and await a later
/// webhook / API compensation (P0 — never guess, never write a
/// ledger with an invented period).
///
/// Errors (e.g. malformed timestamp that would otherwise be silently
/// ignored) are mapped to `None` so that the strict P0 "skip + warn"
/// behavior applies uniformly; parse failures are surfaced via the returned
/// resolution reason in `warn!`.
fn normalize_stripe_period(subscription: &Value) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    // 1. Item-level resolution (2025-03-31.basil+).
    let items = subscription
        .get("items")
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array());
    if let Some(item_periods) = items.and_then(|arr| collect_item_periods(arr)) {
        if item_periods.len() == 1 {
            return validate_optional_period(item_periods[0]);
        }
        // Multiple items: require unanimous period. Any divergence means we
        // cannot uniquely identify the points entitlement's item → P0 None.
        let first = &item_periods[0];
        if item_periods
            .iter()
            .all(|p| p.0 == first.0 && p.1 == first.1)
        {
            return validate_optional_period(*first);
        }
        // Disagreement — cannot uniquely resolve.
        return None;
    }

    // 2. Top-level fallback (pre-basil API versions).
    let top_start = read_stripe_period_field(subscription, "current_period_start");
    let top_end = read_stripe_period_field(subscription, "current_period_end");
    match (top_start, top_end) {
        (Some(s), Some(e)) => validate_period((s, e)),
        _ => None,
    }
}

/// A subscription item's optional period endpoints (each side independently
/// nullable because Stripe may populate only one on some events).
type ItemPeriod = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

/// Read one item's `(current_period_start, current_period_end)` pair, or
/// `None` if the item lacks both period fields (some items legitimately
/// carry no period — e.g. one-time add-ons bundled into a subscription).
fn read_item_period(item: &Value) -> Option<ItemPeriod> {
    let has_start = item
        .get("current_period_start")
        .map(|v| !v.is_null())
        .unwrap_or(false);
    let has_end = item
        .get("current_period_end")
        .map(|v| !v.is_null())
        .unwrap_or(false);
    if !has_start && !has_end {
        return None;
    }
    let start = read_stripe_period_field(item, "current_period_start");
    let end = read_stripe_period_field(item, "current_period_end");
    Some((start, end))
}

/// Collect `(start, end)` pairs from every subscription item that carries
/// period fields. Returns `None` when no item exposes period fields at all
/// (so the caller can fall back to the top-level fields).
fn collect_item_periods(items: &[Value]) -> Option<Vec<ItemPeriod>> {
    let mut collected: Vec<ItemPeriod> = Vec::new();
    for item in items {
        if let Some(period) = read_item_period(item) {
            collected.push(period);
        }
    }
    if collected.is_empty() {
        None
    } else {
        Some(collected)
    }
}

fn read_stripe_period_field(obj: &Value, field: &str) -> Option<DateTime<Utc>> {
    let v = obj.get(field)?;
    if v.is_null() {
        return None;
    }
    // `parse_stripe_datetime` returns Result; on error we treat as absent so
    // the P0 "skip + warn" path applies (do not abort the whole handler
    // with a BadRequest on a period we cannot parse).
    parse_stripe_datetime(v, field).ok()
}

/// Final guard for a fully-resolved pair: a valid period must have
/// `start < end`. Inverted / zero-length windows are rejected as
/// unresolvable (P0).
fn validate_period(pair: (DateTime<Utc>, DateTime<Utc>)) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let (start, end) = pair;
    if start < end {
        Some((start, end))
    } else {
        None
    }
}

/// Guard for an item-level partial pair (each side independently optional):
/// both endpoints must be present and form a valid window (`start < end`).
/// Partial pairs (one side missing) or inverted windows are rejected as
/// unresolvable (P0).
fn validate_optional_period(pair: ItemPeriod) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    match pair {
        (Some(start), Some(end)) => validate_period((start, end)),
        _ => None,
    }
}

/// Normalize a Stripe **Invoice** object's billing period to a unique
/// `(period_start, period_end)` pair (P0).
///
/// This is the invoice counterpart to `normalize_stripe_period`. A Stripe
/// `invoice.payment_succeeded` event carries a Stripe **Invoice** object as
/// `data.object`, which has NO top-level `current_period_*` fields (those live
/// on Subscription/SubscriptionItem objects) and exposes its line items under
/// `lines.data` (NOT `items.data`). For a subscription renewal invoice each
/// invoice line's `period.{start,end}` IS the subscription billing period
/// being paid (Stripe docs: "For subscription line items, this is the
/// subscription period."). That makes the period resolvable directly from the
/// invoice without an extra Stripe API call.
///
/// Resolution order:
/// 1. **Line-level** (`lines.data[].period.{start,end}`):
///    - Single line carrying a period → use it.
///    - Multiple lines all sharing the same period → use the shared period.
///    - Multiple lines with disagreeing periods → cannot uniquely map the
///      points entitlement's line → `None` (P0: do not guess).
///    - Lines legitimately without `period` (e.g. one-time add-on lines) are
///      skipped; resolution requires at least one line with a period.
///    - A line that carries a `period` object but with a null/unparseable
///      `start`/`end` or an inverted (`start >= end`) window is skipped (treated
///      as unresolvable for that line), NOT short-circuited. This lets a valid
///      subscription line still resolve when a sibling proration/credit line
///      carries a malformed period. If every carrying line is malformed → `None`.
/// 2. No resolvable line at all (all lines lack/malform a period) → `None`.
///
/// Returns `None` whenever the period cannot be uniquely resolved; the caller
/// must then skip the renewal grant, emit a structured warning, and await a
/// later webhook / API compensation (P0 — never guess, never write a
/// ledger with an invented period).
///
/// Parse errors are mapped to `None` so that the strict P0 "skip + warn"
/// behavior applies uniformly.
fn normalize_stripe_invoice_period(invoice: &Value) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let lines = invoice
        .get("lines")
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())?;

    // Collect `(start, end)` pairs from every line that carries a `period`.
    let mut line_periods: Vec<(DateTime<Utc>, DateTime<Utc>)> = Vec::new();
    for line in lines {
        let Some(period) = line.get("period") else {
            // Line legitimately carries no period (e.g. one-time add-on) — skip.
            continue;
        };
        // A line that carries a `period` object but with a null/unparseable
        // start or end is treated as unresolvable FOR THIS LINE and skipped
        // (mirroring the no-period branch above), NOT short-circuited to a
        // whole-function None. Real Stripe invoices can carry such lines
        // (proration/credit/tax adjustment lines whose period is absent or
        // malformed) alongside the subscription line whose period IS valid.
        // Short-circuiting would doom the whole resolution whenever ANY single
        // line is malformed, skipping a renewal grant the user paid for. If
        // every carrying line is malformed, `line_periods` stays empty and we
        // return None below (P0 still holds when nothing resolves).
        let Some(start) = read_stripe_period_field(period, "start") else {
            continue;
        };
        let Some(end) = read_stripe_period_field(period, "end") else {
            continue;
        };
        match validate_period((start, end)) {
            Some(pair) => line_periods.push(pair),
            // Inverted / zero-length window on a line that DOES carry a period
            // is a malformed signal — skip this line (consistent with the
            // null/unparseable branches above); resolution proceeds via the
            // remaining valid lines, or returns None if none remain.
            None => continue,
        }
    }

    if line_periods.is_empty() {
        return None;
    }
    if line_periods.len() == 1 {
        return Some(line_periods[0]);
    }
    // Multiple lines: require unanimous period. Any divergence means we cannot
    // uniquely identify the points entitlement's line → P0 None.
    let first = line_periods[0];
    if line_periods
        .iter()
        .all(|p| p.0 == first.0 && p.1 == first.1)
    {
        Some(first)
    } else {
        None
    }
}

fn parse_stripe_subscription_status(
    status: Option<&str>,
    cancel_at_period_end: bool,
) -> Result<SubscriptionStatus, CoreError> {
    let parsed = match status.unwrap_or("active") {
        "active" => SubscriptionStatus::Active,
        "trialing" => SubscriptionStatus::Trialing,
        "canceled" => SubscriptionStatus::Canceled,
        "past_due" | "unpaid" => SubscriptionStatus::PastDue,
        "paused" => SubscriptionStatus::Paused,
        "incomplete" | "incomplete_expired" => SubscriptionStatus::Incomplete,
        other => {
            return Err(CoreError::BadRequest(format!(
                "Unsupported Stripe subscription status: {}",
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

/// Resolve entitlement for a Stripe webhook event, returning the projection
/// `entitlement_key`. Delegates to the price-aware resolver;
/// callers that need the price-level mapping for points issuance should call
/// `resolve_entitlement_mapping` directly and consume `ResolvedEntitlement.mapping`.
async fn resolve_stripe_entitlement(
    app_state: &AppState,
    realm_id: &str,
    metadata: &Value,
    external_product_id: &str,
    external_price_id: Option<&str>,
) -> Result<ResolvedEntitlement, CoreError> {
    let metadata_key = metadata["herald_entitlement_key"]
        .as_str()
        .or_else(|| metadata["entitlementKey"].as_str());
    Ok(resolve_entitlement_mapping(
        app_state,
        realm_id,
        "stripe",
        external_product_id,
        external_price_id,
        metadata_key,
    )
    .await?)
}

async fn resolve_stripe_entitlement_key(
    app_state: &AppState,
    realm_id: &str,
    metadata: &Value,
    external_product_id: &str,
    external_price_id: Option<&str>,
) -> Result<String, CoreError> {
    Ok(resolve_stripe_entitlement(
        app_state,
        realm_id,
        metadata,
        external_product_id,
        external_price_id,
    )
    .await?
    .entitlement_key)
}

fn parse_checkout_completed_payload(
    event: &Value,
) -> Result<StripeCheckoutCompletedPayload, CoreError> {
    let metadata = &event["data"]["object"]["metadata"];
    let client_app_id = parse_uuid_field(
        metadata_value(metadata, "herald_client_app_id", "clientAppId"),
        "clientAppId",
    )?;
    let user_id = parse_uuid_field(
        metadata_value(metadata, "herald_user_id", "userId"),
        "userId",
    )?;

    let entitlement_key = metadata["herald_entitlement_key"]
        .as_str()
        .or_else(|| metadata["entitlementKey"].as_str())
        .unwrap_or("")
        .to_string();

    Ok(StripeCheckoutCompletedPayload {
        event_id: parse_event_id(event)?,
        user_id,
        client_app_id,
        entitlement_key,
        is_trial: metadata["herald_trial_days"]
            .as_u64()
            .or_else(|| metadata["trialDays"].as_u64())
            .is_some_and(|days| days > 0),
        stripe_subscription_id: event["data"]["object"]["subscription"]
            .as_str()
            .map(str::to_string),
        stripe_product_id: event["data"]["object"]["display_items"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item["price"]["product"].as_str())
            .map(str::to_string)
            .unwrap_or_default(),
    })
}

fn parse_subscription_created_payload(
    event: &Value,
) -> Result<StripeSubscriptionCreatedPayload, CoreError> {
    let metadata = &event["data"]["object"]["metadata"];
    let stripe_subscription_id = event["data"]["object"]["id"]
        .as_str()
        .ok_or_else(|| CoreError::BadRequest("Missing subscription id".to_string()))?
        .to_string();
    let user_id = parse_uuid_field(
        metadata_value(metadata, "herald_user_id", "userId"),
        "userId",
    )?;
    let cancel_at_period_end = event["data"]["object"]["cancel_at_period_end"]
        .as_bool()
        .unwrap_or(false);
    let status = parse_stripe_subscription_status(
        event["data"]["object"]["status"].as_str(),
        cancel_at_period_end,
    )?;

    let external_product_id = event["data"]["object"]["items"]["data"][0]["price"]["product"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_default();

    let entitlement_key = metadata["herald_entitlement_key"]
        .as_str()
        .or_else(|| metadata["entitlementKey"].as_str())
        .unwrap_or("")
        .to_string();

    Ok(StripeSubscriptionCreatedPayload {
        event_id: parse_event_id(event)?,
        stripe_subscription_id,
        user_id,
        entitlement_key,
        client_app_id: parse_optional_uuid_field(metadata_value(
            metadata,
            "herald_client_app_id",
            "clientAppId",
        )),
        external_product_id,
        cancel_at_period_end,
        current_period_start: parse_optional_stripe_datetime(
            &event["data"]["object"]["current_period_start"],
        )?,
        current_period_end: parse_optional_stripe_datetime(
            &event["data"]["object"]["current_period_end"],
        )?,
        status,
    })
}

fn parse_subscription_updated_payload(
    event: &Value,
) -> Result<StripeSubscriptionUpdatedPayload, CoreError> {
    let metadata = &event["data"]["object"]["metadata"];
    let previous_entitlement_key = event["data"]["previous_attributes"]["items"]["data"][0]["price"]["metadata"]["herald_entitlement_key"]
        .as_str()
        .or_else(|| event["data"]["previous_attributes"]["items"]["data"][0]["price"]["metadata"]["entitlementKey"].as_str())
        .unwrap_or("")
        .to_string();
    let current_entitlement_key = event["data"]["object"]["items"]["data"][0]["price"]["metadata"]
        ["herald_entitlement_key"]
        .as_str()
        .or_else(|| {
            event["data"]["object"]["items"]["data"][0]["price"]["metadata"]["entitlementKey"]
                .as_str()
        })
        .unwrap_or("")
        .to_string();
    let cancel_at_period_end = event["data"]["object"]["cancel_at_period_end"]
        .as_bool()
        .unwrap_or(false);
    let status = parse_stripe_subscription_status(
        event["data"]["object"]["status"].as_str(),
        cancel_at_period_end,
    )?;

    Ok(StripeSubscriptionUpdatedPayload {
        event_id: parse_event_id(event)?,
        stripe_subscription_id: event["data"]["object"]["id"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing subscription id".to_string()))?
            .to_string(),
        user_id: parse_uuid_field(
            metadata_value(metadata, "herald_user_id", "userId"),
            "userId",
        )?,
        previous_entitlement_key,
        current_entitlement_key,
        external_product_id: event["data"]["object"]["items"]["data"][0]["price"]["product"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_default(),
        cancel_at_period_end,
        current_period_start: parse_optional_stripe_datetime(
            &event["data"]["object"]["current_period_start"],
        )?,
        current_period_end: parse_optional_stripe_datetime(
            &event["data"]["object"]["current_period_end"],
        )?,
        status,
    })
}

fn parse_subscription_deleted_payload(
    event: &Value,
) -> Result<StripeSubscriptionDeletedPayload, CoreError> {
    let metadata = &event["data"]["object"]["metadata"];
    Ok(StripeSubscriptionDeletedPayload {
        event_id: parse_event_id(event)?,
        stripe_subscription_id: event["data"]["object"]["id"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing subscription id".to_string()))?
            .to_string(),
        user_id: parse_uuid_field(
            metadata_value(metadata, "herald_user_id", "userId"),
            "userId",
        )?,
        entitlement_key: metadata["herald_entitlement_key"]
            .as_str()
            .or_else(|| metadata["entitlementKey"].as_str())
            .map(str::to_string),
        cancel_at_period_end: event["data"]["object"]["cancel_at_period_end"]
            .as_bool()
            .unwrap_or(false),
        current_period_start: parse_optional_stripe_datetime(
            &event["data"]["object"]["current_period_start"],
        )?,
        current_period_end: parse_optional_stripe_datetime(
            &event["data"]["object"]["current_period_end"],
        )?,
    })
}

fn parse_charge_refunded_payload(event: &Value) -> Result<StripeChargeRefundedPayload, CoreError> {
    Ok(StripeChargeRefundedPayload {
        event_id: parse_event_id(event)?,
        charge_id: event["data"]["object"]["id"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing charge id".to_string()))?
            .to_string(),
        amount: event["data"]["object"]["amount"]
            .as_i64()
            .ok_or_else(|| CoreError::BadRequest("Missing or invalid amount".to_string()))?,
        amount_refunded: event["data"]["object"]["amount_refunded"]
            .as_i64()
            .ok_or_else(|| {
                CoreError::BadRequest("Missing or invalid amount_refunded".to_string())
            })?,
        user_id: parse_uuid_field(
            metadata_value(
                &event["data"]["object"]["metadata"],
                "herald_user_id",
                "userId",
            ),
            "userId",
        )?,
        subscription_id: parse_optional_uuid_field(metadata_value(
            &event["data"]["object"]["metadata"],
            "herald_subscription_id",
            "subscriptionId",
        )),
        refund_type: event["data"]["object"]["metadata"]["refundType"]
            .as_str()
            .unwrap_or("subscription")
            .to_string(),
    })
}

fn parse_invoice_paid_payload(event: &Value) -> Result<StripeInvoicePaidPayload, CoreError> {
    let metadata = &event["data"]["object"]["metadata"];
    let entitlement_key = metadata["herald_entitlement_key"]
        .as_str()
        .or_else(|| metadata["entitlementKey"].as_str())
        .unwrap_or("")
        .to_string();

    Ok(StripeInvoicePaidPayload {
        event_id: parse_event_id(event)?,
        stripe_subscription_id: event["data"]["object"]["subscription"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing subscription id".to_string()))?
            .to_string(),
        user_id: parse_uuid_field(
            metadata_value(metadata, "herald_user_id", "userId"),
            "userId",
        )?,
        entitlement_key,
        current_period_start: parse_optional_stripe_datetime(
            &event["data"]["object"]["current_period_start"],
        )?,
        current_period_end: parse_optional_stripe_datetime(
            &event["data"]["object"]["current_period_end"],
        )?,
    })
}

fn parse_payment_intent_succeeded_payload(
    event: &Value,
) -> Result<StripePaymentIntentSucceededPayload, CoreError> {
    let object = &event["data"]["object"];
    let attempt_id =
        parse_attempt_id(&object["metadata"][metadata_keys::ATTEMPT_ID]).ok_or_else(|| {
            CoreError::BadRequest("Missing attemptId in payment intent metadata".to_string())
        })?;

    Ok(StripePaymentIntentSucceededPayload {
        attempt_id,
        payment_intent_id: object["id"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing payment intent id".to_string()))?
            .to_string(),
        completed_at: parse_optional_stripe_datetime(&object["created"])?.unwrap_or_else(Utc::now),
    })
}

fn parse_payment_failed_payload(
    event: &Value,
) -> Result<Option<StripePaymentFailedPayload>, CoreError> {
    let object = &event["data"]["object"];
    let Some(attempt_id) = parse_attempt_id(&object["metadata"][metadata_keys::ATTEMPT_ID]) else {
        return Ok(None);
    };

    Ok(Some(StripePaymentFailedPayload {
        attempt_id,
        provider_reference: object["id"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing failed payment object id".to_string()))?
            .to_string(),
        provider_status: object["status"].as_str().unwrap_or("failed").to_string(),
        completed_at: parse_optional_stripe_datetime(&object["created"])?.unwrap_or_else(Utc::now),
    }))
}

struct StripeCreditNoteCreatedPayload {
    event_id: String,
    stripe_credit_note_id: String,
    stripe_invoice_id: String,
    /// Credit note total in the smallest currency unit (Stripe Credit Note `total`).
    amount: i64,
    currency: String,
}

fn parse_credit_note_created_payload(
    event: &Value,
) -> Result<StripeCreditNoteCreatedPayload, CoreError> {
    let object = &event["data"]["object"];
    Ok(StripeCreditNoteCreatedPayload {
        event_id: parse_event_id(event)?,
        stripe_credit_note_id: object["id"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing credit note id".to_string()))?
            .to_string(),
        stripe_invoice_id: object["invoice"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing invoice id on credit note".to_string()))?
            .to_string(),
        amount: object["total"].as_i64().ok_or_else(|| {
            CoreError::BadRequest("Missing or invalid credit note total".to_string())
        })?,
        currency: object["currency"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing credit note currency".to_string()))?
            .to_string(),
    })
}

struct StripeCreditNoteVoidedPayload {
    event_id: String,
    stripe_credit_note_id: String,
    stripe_invoice_id: String,
    amount: i64,
}

fn parse_credit_note_voided_payload(
    event: &Value,
) -> Result<StripeCreditNoteVoidedPayload, CoreError> {
    let object = &event["data"]["object"];
    Ok(StripeCreditNoteVoidedPayload {
        event_id: parse_event_id(event)?,
        stripe_credit_note_id: object["id"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing credit note id".to_string()))?
            .to_string(),
        stripe_invoice_id: object["invoice"]
            .as_str()
            .ok_or_else(|| CoreError::BadRequest("Missing invoice id on credit note".to_string()))?
            .to_string(),
        amount: object["total"].as_i64().ok_or_else(|| {
            CoreError::BadRequest("Missing or invalid credit note total".to_string())
        })?,
    })
}

async fn fulfill_payment_attempt(
    app_state: &AppState,
    realm_id: &str,
    attempt_id: Uuid,
    provider_status: &str,
    provider_transaction_id: String,
    completed_at: DateTime<Utc>,
    billing_type_override: Option<BillingType>,
) -> Result<(), CoreError> {
    crate::shared_fulfillment::fulfill_provider_event(
        app_state,
        realm_id,
        attempt_id,
        "stripe",
        provider_status,
        provider_transaction_id,
        completed_at,
        billing_type_override,
    )
    .await
}

async fn fail_payment_attempt(
    app_state: &AppState,
    realm_id: &str,
    payload: StripePaymentFailedPayload,
) -> Result<(), CoreError> {
    app_state
        .payment_attempt_service
        .mark_payment_failed(
            realm_id,
            payload.attempt_id,
            payload.provider_status,
            payload.completed_at,
        )
        .await?;

    info!(
        realm_id = %realm_id,
        attempt_id = %payload.attempt_id,
        provider_reference = %payload.provider_reference,
        "Stripe payment attempt marked failed"
    );

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn sync_stripe_subscription_with_history_in_txn(
    app_state: &AppState,
    realm_id: &str,
    user_id: Uuid,
    external_subscription_id: &str,
    client_app_id: Option<Uuid>,
    entitlement_key: String,
    external_product_id: String,
    external_price_id: Option<String>,
    status: SubscriptionStatus,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
    cancel_at_period_end: bool,
    cancel_at: Option<DateTime<Utc>>,
    existing_subscription: Option<Subscription>,
    history_event_type: HistoryEventType,
) -> Result<(Subscription, Option<Subscription>), CoreError> {
    let txn = app_state.billing_repository.begin_transaction().await?;

    let (subscription, previous_subscription) = sync_subscription_in_txn(
        &txn,
        SyncSubscriptionInput {
            provider: "stripe",
            realm_id: realm_id.to_string(),
            user_id: Some(user_id),
            external_subscription_id: external_subscription_id.to_string(),
            external_product_id,
            client_app_id,
            entitlement_key,
            external_price_id,
            provider_metadata: None,
            status: status.clone(),
            current_period_start,
            current_period_end,
            cancel_at_period_end,
            cancel_at,
            existing_subscription,
        },
    )
    .await?
    .ok_or_else(|| {
        CoreError::InternalServerError(
            "stripe subscription sync failed to create or update subscription".to_string(),
        )
    })?;

    save_subscription_history_in_txn(
        &app_state.billing_repository,
        &txn,
        previous_subscription.as_ref(),
        &subscription,
        history_event_type,
    )
    .await?;

    txn.commit()
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

    Ok((subscription, previous_subscription))
}

#[allow(clippy::too_many_arguments)]
async fn sync_stripe_subscription_with_detected_history_in_txn(
    app_state: &AppState,
    realm_id: &str,
    user_id: Uuid,
    external_subscription_id: &str,
    client_app_id: Option<Uuid>,
    entitlement_key: String,
    external_product_id: String,
    external_price_id: Option<String>,
    status: SubscriptionStatus,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
    cancel_at_period_end: bool,
    cancel_at: Option<DateTime<Utc>>,
    existing_subscription: Option<Subscription>,
) -> Result<(Subscription, Option<Subscription>), CoreError> {
    let txn = app_state.billing_repository.begin_transaction().await?;

    let (subscription, previous_subscription) = sync_subscription_in_txn(
        &txn,
        SyncSubscriptionInput {
            provider: "stripe",
            realm_id: realm_id.to_string(),
            user_id: Some(user_id),
            external_subscription_id: external_subscription_id.to_string(),
            external_product_id,
            client_app_id,
            entitlement_key,
            external_price_id,
            provider_metadata: None,
            status,
            current_period_start,
            current_period_end,
            cancel_at_period_end,
            cancel_at,
            existing_subscription,
        },
    )
    .await?
    .ok_or_else(|| {
        CoreError::InternalServerError(
            "stripe subscription sync failed to create or update subscription".to_string(),
        )
    })?;

    let history_event_type = match previous_subscription {
        Some(ref previous) => detect_change_type(previous, &subscription),
        None => HistoryEventType::Created,
    };

    save_subscription_history_in_txn(
        &app_state.billing_repository,
        &txn,
        previous_subscription.as_ref(),
        &subscription,
        history_event_type,
    )
    .await?;

    txn.commit()
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

    Ok((subscription, previous_subscription))
}

async fn sync_subscription_input_with_history_in_txn(
    app_state: &AppState,
    input: SyncSubscriptionInput,
    history_event_type: HistoryEventType,
) -> Result<Option<(Subscription, Option<Subscription>)>, CoreError> {
    let txn = app_state.billing_repository.begin_transaction().await?;
    let synced = sync_subscription_in_txn(&txn, input).await?;

    if let Some((subscription, previous_subscription)) = synced.as_ref() {
        save_subscription_history_in_txn(
            &app_state.billing_repository,
            &txn,
            previous_subscription.as_ref(),
            subscription,
            history_event_type,
        )
        .await?;
    }

    txn.commit()
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

    Ok(synced)
}

async fn sync_subscription_input_with_detected_history_in_txn(
    app_state: &AppState,
    input: SyncSubscriptionInput,
) -> Result<Option<(Subscription, Option<Subscription>)>, CoreError> {
    let txn = app_state.billing_repository.begin_transaction().await?;
    let synced = sync_subscription_in_txn(&txn, input).await?;

    if let Some((subscription, previous_subscription)) = synced.as_ref() {
        let history_event_type = match previous_subscription {
            Some(previous) => detect_change_type(previous, subscription),
            None => HistoryEventType::Created,
        };
        save_subscription_history_in_txn(
            &app_state.billing_repository,
            &txn,
            previous_subscription.as_ref(),
            subscription,
            history_event_type,
        )
        .await?;
    }

    txn.commit()
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

    Ok(synced)
}

/// For one-time Checkout Sessions (mode=payment), Stripe emits the PaymentIntent
/// ID on the Checkout Session object — but NOT on the resulting `invoice.*` event
/// payloads. Herald's external invoice sync (`handle_stripe_invoice_event`) reads
/// `payment_intent` off the invoice object, so without this linkage
/// `invoice.external_order_id` stays null forever for one-time purchases.
///
/// This helper is invoked from `handle_checkout_session_completed` whenever the
/// session is in `payment` mode and carries both a `payment_intent` and an
/// `invoice` ID. It performs a Branch-A upsert keyed on the Stripe invoice id,
/// setting `external_order_id = payment_intent`. Subsequent `invoice.*` events
/// reuse Branch A's `COALESCE(EXCLUDED.external_order_id, invoice.external_order_id)`
/// so the PaymentIntent already stored here is preserved.
///
/// Failures are logged and swallowed: this linkage is best-effort enrichment and
/// must never block fulfillment or the rest of the webhook handler.
#[allow(clippy::too_many_arguments)]
async fn link_one_time_payment_intent_to_invoice(
    app_state: &AppState,
    realm_id: &str,
    payment_intent: &str,
    stripe_invoice_id: &str,
    account_id: Option<Uuid>,
    attempt_id: Option<Uuid>,
    total: i64,
    currency: &str,
) {
    let external_data = ExternalInvoiceData {
        realm_id: realm_id.to_string(),
        provider: InvoiceProvider::Stripe,
        payment_provider: Some("stripe".to_string()),
        external_invoice_id: Some(stripe_invoice_id.to_string()),
        external_order_id: Some(payment_intent.to_string()),
        external_status: None,
        // Leave URLs empty; the authoritative values arrive in invoice.finalized/paid
        // events and Branch A upsert will overwrite them without touching external_order_id.
        external_hosted_url: None,
        external_pdf_url: None,
        external_payload: None,
        tax_details: None,
        account_id,
        // The Checkout Session object is not passed into this helper, so the
        // buyer snapshot cannot be extracted here. The buyer columns are left
        // None and get COALESCE-backfilled by the subsequent `invoice.*` events,
        // which carry `customer_name` / `customer_email` / `customer_address`.
        applicant_user_id: None,
        billing_name: None,
        billing_email: None,
        billing_phone: None,
        billing_address: None,
        currency: currency.to_string(),
        total,
        // Use Paid so a missing subsequent invoice.paid (rare race) still leaves the
        // row in a terminal-ish state. invoice.paid will upsert back to paid if it
        // arrives, and invoice.created will downgrade to draft as expected.
        status: InvoiceStatus::Paid,
        subscription_id: None,
        // One-time checkout: attribute the invoice to the one-time payment attempt.
        payment_attempt_id: attempt_id,
    };

    if let Err(e) = app_state
        .invoice_repository
        .upsert_external_invoice(external_data)
        .await
    {
        warn!(
            realm_id = %realm_id,
            payment_intent = %payment_intent,
            stripe_invoice_id = %stripe_invoice_id,
            error = %e,
            "Failed to link one-time payment_intent to invoice external_order_id - invoice.* events will retry"
        );
    } else {
        info!(
            realm_id = %realm_id,
            payment_intent = %payment_intent,
            stripe_invoice_id = %stripe_invoice_id,
            "Linked one-time checkout PaymentIntent to invoice external_order_id"
        );
    }
}

/// Handle checkout.session.completed events
///
/// Dispatches based on checkout mode:
/// - With attemptId in metadata: completes the payment attempt and fulfills (both one-time and recurring)
/// - Without attemptId + mode=payment: logs warning, no fulfillment (orphan one-time event)
/// - Without attemptId + mode=subscription (or absent): creates subscription (legacy/legacy webhook)
async fn handle_checkout_session_completed(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let mode = event["data"]["object"]["mode"].as_str().map(str::to_string);

    // For one-time Checkout Sessions (mode=payment), the PaymentIntent is only
    // available on the Checkout Session object, never on the resulting invoice.*
    // event payloads. Capture it here so we can populate invoice.external_order_id.
    // Done unconditionally (regardless of attemptId) so both the legacy checkout
    // endpoint and the payment-attempt endpoint produce the linkage.
    if mode.as_deref() == Some("payment") {
        let session = &event["data"]["object"];
        if let (Some(payment_intent), Some(stripe_invoice_id)) = (
            session["payment_intent"].as_str(),
            session["invoice"].as_str(),
        ) {
            // Best-effort: resolve account_id from session metadata so the row
            // is attributable; falls back to None like the invoice.* handler.
            // `metadata_user_id` tries all three key variants (`heraldUserId`,
            // `herald_user_id`, `userId`) because Stripe metadata key naming is
            // inconsistent across write paths.
            let account_id = metadata_user_id(&session["metadata"]);
            // Resolve the one-time payment attempt id from metadata so the invoice
            // row is attributed to the attempt that drove this checkout.
            let attempt_id = parse_attempt_id(&session["metadata"][metadata_keys::ATTEMPT_ID]);
            // Carry the session's amount_total/currency so the placeholder row is
            // correct even when checkout.session.completed arrives before the
            // invoice.* events (Branch A UPDATE does not touch total/currency).
            let total = session["amount_total"].as_i64().unwrap_or(0);
            let currency = session["currency"].as_str().unwrap_or("usd").to_string();
            link_one_time_payment_intent_to_invoice(
                &app_state,
                realm_id,
                payment_intent,
                stripe_invoice_id,
                account_id,
                attempt_id,
                total,
                &currency,
            )
            .await;
        }
    }

    // If attemptId is present in metadata, fulfill via payment attempt flow.
    // This covers both one-time (mode=payment) and recurring (mode=subscription) purchases
    // initiated through PurchaseService.
    if let Some(attempt_id) =
        parse_attempt_id(&event["data"]["object"]["metadata"][metadata_keys::ATTEMPT_ID])
    {
        let payment_status = event["data"]["object"]["payment_status"]
            .as_str()
            .unwrap_or("unpaid");
        if payment_status != "paid" && payment_status != "no_payment_required" {
            let strategy = read_async_points_strategy(&app_state, realm_id).await;
            if strategy != AsyncPointsStrategy::Eager {
                info!(
                    realm_id = %realm_id,
                    attempt_id = %attempt_id,
                    payment_status = %payment_status,
                    mode = ?mode,
                    "Checkout session completed before payment settled - waiting for async result"
                );

                return Ok(create_placeholder_transaction(
                    attempt_id,
                    realm_id,
                    TransactionType::Recharge,
                ));
            }
            // Eager strategy: fall through to fulfillment despite unpaid status
            info!(
                realm_id = %realm_id,
                attempt_id = %attempt_id,
                payment_status = %payment_status,
                "Eager strategy: fulfilling despite unpaid async payment"
            );
        }

        let provider_transaction_id = event["data"]["object"]["subscription"]
            .as_str()
            .or_else(|| event["data"]["object"]["payment_intent"].as_str())
            .or_else(|| event["data"]["object"]["id"].as_str())
            .ok_or_else(|| CoreError::BadRequest("Missing provider transaction id".to_string()))?
            .to_string();
        let completed_at = parse_optional_stripe_datetime(&event["data"]["object"]["created"])?
            .unwrap_or_else(Utc::now);

        info!(
            realm_id = %realm_id,
            attempt_id = %attempt_id,
            mode = ?mode,
            "Processing checkout.session.completed with attemptId - fulfilling payment attempt"
        );

        let billing_type_override = mode.as_deref().and_then(|m| match m {
            "payment" => Some(BillingType::OneTime),
            _ => None,
        });

        fulfill_payment_attempt(
            &app_state,
            realm_id,
            attempt_id,
            "succeeded",
            provider_transaction_id,
            completed_at,
            billing_type_override,
        )
        .await?;

        return Ok(create_placeholder_transaction(
            attempt_id,
            realm_id,
            TransactionType::Recharge,
        ));
    }

    let payload = parse_checkout_completed_payload(&event)?;
    let event_id = payload.event_id.as_str();

    if mode.as_deref() == Some("payment") {
        // mode=payment without attemptId: one-time checkout we cannot fulfill
        warn!(
            realm_id = %realm_id,
            event_id = %event_id,
            user_id = %payload.user_id,
            "checkout.session.completed with mode=payment but no attemptId - cannot fulfill, recording audit event"
        );

        return Ok(create_placeholder_transaction(
            payload.user_id,
            realm_id,
            TransactionType::Recharge,
        ));
    }

    let stripe_subscription_id = payload.stripe_subscription_id.as_deref().ok_or_else(|| {
        CoreError::BadRequest("Missing subscription id for subscription checkout".to_string())
    })?;

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing checkout.session.completed event"
    );

    // Extract price_id (best-effort) from display_items; used both for price-aware
    // resolution and for the subscription projection write.
    let checkout_price_id = event["data"]["object"]["display_items"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["price"]["id"].as_str())
        .map(str::to_string);

    // Resolve entitlement_key via price-aware fallback chain.
    // checkout.session.completed only enters the resolver branch when metadata
    // key is empty; price_id falls through to step 3c single-row fallback when None.
    let entitlement_key = if payload.entitlement_key.is_empty() {
        resolve_stripe_entitlement_key(
            &app_state,
            realm_id,
            &event["data"]["object"]["metadata"],
            &payload.stripe_product_id,
            checkout_price_id.as_deref(),
        )
        .await?
    } else {
        payload.entitlement_key.clone()
    };

    let status = if payload.is_trial {
        SubscriptionStatus::Trialing
    } else {
        SubscriptionStatus::Active
    };

    // Check if subscription already exists (customer.subscription.created webhook
    // may have arrived first and already created it via sync_subscription).
    let existing_subscription = app_state
        .billing_repository
        .find_by_external_subscription_id(stripe_subscription_id, "stripe")
        .await?;

    let now = chrono::Utc::now();
    let (_created_subscription, _) = if let Some(existing) = existing_subscription {
        info!(
            realm_id = %realm_id,
            subscription_id = %existing.id,
            stripe_subscription_id = %stripe_subscription_id,
            event_id = %event_id,
            "Checkout completed - subscription already exists from subscription.created webhook, updating"
        );

        sync_stripe_subscription_with_history_in_txn(
            &app_state,
            realm_id,
            payload.user_id,
            stripe_subscription_id,
            Some(payload.client_app_id),
            entitlement_key.clone(),
            payload.stripe_product_id.clone(),
            existing.external_price_id.clone(),
            status,
            existing.current_period_start.or(Some(now)),
            existing.current_period_end,
            existing.cancel_at_period_end,
            existing.cancel_at,
            Some(existing),
            HistoryEventType::Created,
        )
        .await?
    } else {
        let (created, previous) = sync_stripe_subscription_with_history_in_txn(
            &app_state,
            realm_id,
            payload.user_id,
            stripe_subscription_id,
            Some(payload.client_app_id),
            entitlement_key.clone(),
            payload.stripe_product_id.clone(),
            checkout_price_id.clone(),
            status,
            Some(now),
            None,
            false,
            None,
            None,
            HistoryEventType::Created,
        )
        .await?;

        info!(
            realm_id = %realm_id,
            subscription_id = %created.id,
            client_app_id = %payload.client_app_id,
            entitlement_key = %entitlement_key,
            stripe_subscription_id = %stripe_subscription_id,
            event_id = %event_id,
            "Checkout completed - subscription created"
        );

        (created, previous)
    };

    // Actual subscription points will be granted by customer.subscription.created event
    Ok(create_placeholder_transaction(
        payload.client_app_id,
        realm_id,
        TransactionType::SubscriptionGrant,
    ))
}

async fn handle_payment_intent_succeeded(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_payment_intent_succeeded_payload(&event)?;

    fulfill_payment_attempt(
        &app_state,
        realm_id,
        payload.attempt_id,
        "succeeded",
        payload.payment_intent_id,
        payload.completed_at,
        None,
    )
    .await?;

    Ok(create_placeholder_transaction(
        payload.attempt_id,
        realm_id,
        TransactionType::Recharge,
    ))
}

async fn handle_checkout_session_async_succeeded(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let object = &event["data"]["object"];
    let Some(attempt_id) = parse_attempt_id(&object["metadata"][metadata_keys::ATTEMPT_ID]) else {
        warn!(
            realm_id = %realm_id,
            event_id = %parse_event_id(&event)?,
            "Stripe async checkout success has no attemptId metadata - ignoring"
        );
        return Ok(create_placeholder_transaction(
            Uuid::now_v7(),
            realm_id,
            TransactionType::Recharge,
        ));
    };
    let provider_transaction_id = object["subscription"]
        .as_str()
        .or_else(|| object["payment_intent"].as_str())
        .or_else(|| object["id"].as_str())
        .ok_or_else(|| CoreError::BadRequest("Missing provider transaction id".to_string()))?
        .to_string();
    let completed_at = parse_optional_stripe_datetime(&object["created"])?.unwrap_or_else(Utc::now);
    let billing_type_override = object["mode"].as_str().and_then(|mode| match mode {
        "payment" => Some(BillingType::OneTime),
        _ => None,
    });

    // Idempotency check: if eager strategy already fulfilled during checkout.session.completed,
    // the attempt is already Succeeded — skip fulfillment.
    // Also guards against the race where async_payment_failed arrives first (status = Failed/Cancelled).
    let existing_attempt = match app_state
        .payment_attempt_service
        .find_payment_attempt(realm_id, attempt_id)
        .await
    {
        Ok(Some(attempt)) => Some(attempt),
        Ok(None) => None, // attempt not found in this realm — proceed with fulfillment
        Err(e) => return Err(e), // DB error — propagate
    };
    if let Some(attempt) = existing_attempt {
        if attempt.status == PaymentAttemptStatus::Succeeded {
            info!(
                realm_id = %realm_id,
                attempt_id = %attempt_id,
                "Async payment succeeded but attempt already fulfilled (eager strategy) - skipping"
            );
            return Ok(create_placeholder_transaction(
                attempt_id,
                realm_id,
                TransactionType::Recharge,
            ));
        }
        if attempt.status.is_terminal() {
            warn!(
                realm_id = %realm_id,
                attempt_id = %attempt_id,
                status = %attempt.status,
                "Async payment succeeded but attempt already in terminal state — success event lost (race with failure)"
            );
            return Ok(create_placeholder_transaction(
                attempt_id,
                realm_id,
                TransactionType::Recharge,
            ));
        }
    }

    fulfill_payment_attempt(
        &app_state,
        realm_id,
        attempt_id,
        "succeeded",
        provider_transaction_id,
        completed_at,
        billing_type_override,
    )
    .await?;

    Ok(create_placeholder_transaction(
        attempt_id,
        realm_id,
        TransactionType::Recharge,
    ))
}

async fn handle_checkout_session_async_failed(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let object = &event["data"]["object"];
    let Some(attempt_id) = parse_attempt_id(&object["metadata"][metadata_keys::ATTEMPT_ID]) else {
        warn!(
            realm_id = %realm_id,
            event_id = %parse_event_id(&event)?,
            "Stripe async checkout failure has no attemptId metadata - ignoring"
        );
        return Ok(create_placeholder_transaction(
            Uuid::now_v7(),
            realm_id,
            TransactionType::Recharge,
        ));
    };

    // Distinguish "not found" from DB errors: NotFound is safe (conservative path),
    // but DB errors must propagate so the webhook retry loop can recover.
    let attempt = match app_state
        .payment_attempt_service
        .find_payment_attempt(realm_id, attempt_id)
        .await
    {
        Ok(Some(attempt)) => Some(attempt),
        Ok(None) => None, // attempt not found in this realm — conservative path
        Err(e) => return Err(e), // DB error — propagate
    };

    let needs_revocation = attempt
        .as_ref()
        .is_some_and(|a| a.status == PaymentAttemptStatus::Succeeded);

    if !needs_revocation {
        // Conservative strategy path: attempt is Pending/Failed/not-found
        let payload = StripePaymentFailedPayload {
            attempt_id,
            provider_reference: object["id"].as_str().unwrap_or("").to_string(),
            provider_status: object["payment_status"]
                .as_str()
                .unwrap_or("failed")
                .to_string(),
            completed_at: parse_optional_stripe_datetime(&object["created"])?
                .unwrap_or_else(Utc::now),
        };
        fail_payment_attempt(&app_state, realm_id, payload).await?;
        return Ok(create_placeholder_transaction(
            attempt_id,
            realm_id,
            TransactionType::Recharge,
        ));
    }

    // Eager strategy path: attempt was already Succeeded — must revoke points
    let attempt = attempt.unwrap();
    let mode_str = object["mode"].as_str();
    let billing_type_override = mode_str.and_then(|mode| match mode {
        "payment" => Some(BillingType::OneTime),
        _ => None,
    });

    if mode_str.is_none() {
        warn!(
            realm_id = %realm_id,
            attempt_id = %attempt_id,
            "mode field missing in async_payment_failed event — defaulting to subscription revocation path"
        );
    }

    info!(
        realm_id = %realm_id,
        attempt_id = %attempt_id,
        user_id = %attempt.user_id,
        billing_type = ?billing_type_override,
        "Async payment failed after eager fulfillment — revoking points"
    );

    let revocation_result = if billing_type_override == Some(BillingType::OneTime) {
        // One-time purchase: revoke only the TopupCredit ledger from this specific attempt
        // (source_id = attempt_id), avoiding over-broad revocation of unrelated topup credits.
        let mut one_time_result = herald_core::domain::points::dtos::RevokePointsOutput::empty();
        for bucket_id in crate::webhook_common::captured_bucket_ids(&app_state, &attempt).await? {
            let revoked = app_state
                .points_service
                .revoke_points_by_source_id(
                    realm_id,
                    attempt.user_id,
                    bucket_id,
                    &attempt_id.to_string(),
                    RevocationType::RefundRevoke,
                    format!("Async payment failed revocation for attempt {}", attempt_id),
                )
                .await?;
            one_time_result.ledger_ids.extend(revoked.ledger_ids);
            one_time_result.total_revoked += revoked.total_revoked;
            one_time_result.revoked_at = revoked.revoked_at;
        }

        // Revoke payment-granted permanent roles for this one-time attempt
        // with `source_id = attempt.id`, so revoke with the same source id.
        // Idempotent: NotFound (no payment role / already revoked) is a no-op,
        // not an error. Only `source='payment'` rows are touched; manual grants
        // are unaffected.
        revoke_payment_roles_for_attempt(
            &app_state,
            realm_id,
            attempt.user_id,
            &attempt_id.to_string(),
        )
        .await;

        one_time_result
    } else {
        // Resolve entitlement_key for targeted subscription credit revocation
        let entitlement_key = app_state
            .billing_repository
            .find_entitlement_mapping_by_id(attempt.target_id)
            .await?
            .map(|m| m.entitlement_key);

        // Update subscription record status to "canceled" — scope to the specific subscription
        // to avoid canceling unrelated subscriptions for the same user.
        // Try external_subscription_id from checkout session first, then fall back to
        // the most recent subscription for this entitlement.
        let stripe_subscription_id = object["subscription"].as_str();

        // active quota entitlement by `source_id = subscription_id`, so the
        // cancel must pass the subscription that was eagerly granted.
        //
        // PRD §4.1: a missed role/quota revoke is a P0 fault. Previously a nil
        // subscription_id was passed into handle_subscription_cancel, which
        // silently matched zero rows (idempotent no-op) and masked the miss.
        // Now we keep the id as Option and skip the revoke call when None,
        // logging a warning so the compensation/retry sweep can reconcile.
        let subscription_id: Option<Uuid> = if let Some(ext_sub_id) = stripe_subscription_id {
            app_state
                .billing_repository
                .find_by_external_subscription_id(ext_sub_id, "stripe")
                .await?
                .map(|s| s.id)
        } else if let Some(ref ekey) = entitlement_key {
            app_state
                .billing_repository
                .list_subscriptions(realm_id, Some(ekey), Some("active"), Some("stripe"), 1, 50)
                .await?
                .0
                .into_iter()
                .find(|s| s.user_id == attempt.user_id)
                .map(|s| s.id)
        } else {
            None
        };

        let result = if let Some(sub_id) = subscription_id {
            let _subscription = app_state
                .billing_repository
                .find_subscription_by_id(sub_id)
                .await?
                .ok_or_else(|| {
                    CoreError::not_found(&format!(
                        "Subscription {} for async payment revocation",
                        sub_id
                    ))
                })?;
            // Subscription: cancel subscription + revoke the subscription's active
            // quota entitlement (done internally by handle_subscription_cancel via
            // source_id = subscription_id). Idempotent on no-match.
            app_state
                .subscription_service
                .handle_subscription_cancel(
                    attempt.user_id,
                    realm_id,
                    sub_id,
                    CancelMode::ImmediateCancel,
                    None,
                    entitlement_key.as_deref(),
                )
                .await?
        } else {
            warn!(
                realm_id = %realm_id,
                attempt_id = %attempt_id,
                "async_payment_failed: no subscription resolvable (no external id and no \
                 active subscription match); skipping entitlement revoke to avoid a \
                 nil-source_id silent no-op. The compensation/retry sweep must reconcile. \
                 (PRD §4.1: missed revoke = P0 fault)"
            );
            herald_core::domain::points::dtos::RevokePointsOutput::empty()
        };

        let rows_updated = if let Some(ext_sub_id) = stripe_subscription_id {
            app_state
                .billing_repository
                .cancel_subscriptions_by_external_id(realm_id, attempt.user_id, ext_sub_id)
                .await?
        } else {
            warn!(
                realm_id = %realm_id,
                attempt_id = %attempt_id,
                "No subscription field in async_payment_failed event — querying entitlement_key for scoped cancel"
            );
            let entitlement_key = app_state
                .billing_repository
                .find_entitlement_mapping_by_id(attempt.target_id)
                .await?
                .map(|m| m.entitlement_key);
            if let Some(ekey) = entitlement_key {
                app_state
                    .billing_repository
                    .cancel_subscriptions_by_entitlement_key(realm_id, attempt.user_id, &ekey)
                    .await?
            } else {
                warn!(
                    realm_id = %realm_id,
                    attempt_id = %attempt_id,
                    target_id = %attempt.target_id,
                    "Cannot resolve entitlement_key for scoped subscription cancel — skipping"
                );
                0
            }
        };

        info!(
            realm_id = %realm_id,
            user_id = %attempt.user_id,
            rows_updated,
            "Subscription status set to canceled after async payment failure"
        );

        result
    };

    // Debt recording: check if total_revoked < original granted points
    // One-time purchases use source_id = attempt_id, subscriptions use source_id = entitlement_key.
    // Combined query retrieves both id and granted_amount in a single trip (Finding 7).
    let source_id_for_ledger = if billing_type_override == Some(BillingType::OneTime) {
        attempt_id.to_string()
    } else {
        // Subscription credits are keyed by entitlement_key in grant_points_atomic
        let entitlement_key = app_state
            .billing_repository
            .find_entitlement_mapping_by_id(attempt.target_id)
            .await?
            .map(|m| m.entitlement_key);
        entitlement_key.unwrap_or_else(|| attempt_id.to_string())
    };
    let ledger = app_state
        .points_repository
        .find_ledger_by_source_id(realm_id, &source_id_for_ledger)
        .await?;
    let original_points = ledger.as_ref().map(|l| l.granted_amount).unwrap_or(0);

    if revocation_result.total_revoked < original_points {
        let debt_amount = original_points - revocation_result.total_revoked;
        // Resolve ledger_id: prefer the revocation result, fall back to the ledger query above.
        // This handles the case where all credits were already consumed (no ledger entries to revoke).
        let ledger_id = revocation_result
            .ledger_ids
            .first()
            .copied()
            .or_else(|| ledger.as_ref().map(|l| l.id))
            .unwrap_or_else(Uuid::nil);
        let reason = format!(
            "debt:original={},recovered={},shortfall={},reason=async_payment_failed_insufficient_balance",
            original_points, revocation_result.total_revoked, debt_amount
        );
        app_state
            .points_repository
            .create_revocation_record(PointsRevocationRecord {
                id: Uuid::now_v7(),
                ledger_id,
                user_id: attempt.user_id,
                realm_id: realm_id.to_string(),
                revocation_type: RevocationType::RefundRevoke,
                revoked_amount: debt_amount,
                reason,
                reference_id: Some(attempt_id.to_string()),
                created_at: chrono::Utc::now(),
            })
            .await?;

        info!(
            realm_id = %realm_id,
            attempt_id = %attempt_id,
            user_id = %attempt.user_id,
            original_points = original_points,
            total_revoked = revocation_result.total_revoked,
            debt_amount = debt_amount,
            "Recorded debt for insufficient balance during async failure revocation"
        );
    }

    app_state
        .payment_attempt_service
        .mark_failed_for_async_recovery(
            realm_id,
            attempt_id,
            object["payment_status"]
                .as_str()
                .unwrap_or("failed")
                .to_string(),
            parse_optional_stripe_datetime(&object["created"])?.unwrap_or_else(Utc::now),
        )
        .await?;

    Ok(create_placeholder_transaction(
        attempt_id,
        realm_id,
        TransactionType::Recharge,
    ))
}

async fn handle_checkout_session_expired(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let object = &event["data"]["object"];
    let Some(attempt_id) = parse_attempt_id(&object["metadata"][metadata_keys::ATTEMPT_ID]) else {
        warn!(
            realm_id = %realm_id,
            event_id = %parse_event_id(&event)?,
            "Stripe checkout expired has no attemptId metadata - ignoring"
        );
        return Ok(create_placeholder_transaction(
            Uuid::now_v7(),
            realm_id,
            TransactionType::Recharge,
        ));
    };
    let payload = StripePaymentFailedPayload {
        attempt_id,
        provider_reference: object["id"].as_str().unwrap_or("").to_string(),
        provider_status: "expired".to_string(),
        completed_at: parse_optional_stripe_datetime(&object["expires_at"])?
            .unwrap_or_else(Utc::now),
    };

    fail_payment_attempt(&app_state, realm_id, payload).await?;

    Ok(create_placeholder_transaction(
        attempt_id,
        realm_id,
        TransactionType::Recharge,
    ))
}

async fn handle_payment_failed(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let event_id = parse_event_id(&event)?;
    let Some(payload) = parse_payment_failed_payload(&event)? else {
        // No `attemptId` metadata ⟹ this is not a one-time purchase payment
        // attempt. For `invoice.payment_failed` on a subscription renewal,
        // idempotent and keyed to the subscription period, and the prior
        // period's entitlement expires naturally at its `effective_until`.
        // No reclaim is required on a failed renewal.
        warn!(
            realm_id = %realm_id,
            event_id = %event_id,
            "Stripe payment_failed event has no attemptId metadata - ignoring purchase-attempt path (subscription renewal reclaim retired under quota model)"
        );
        return Ok(create_placeholder_transaction(
            Uuid::now_v7(),
            realm_id,
            TransactionType::Recharge,
        ));
    };

    let attempt_id = payload.attempt_id;
    fail_payment_attempt(&app_state, realm_id, payload).await?;

    Ok(create_placeholder_transaction(
        attempt_id,
        realm_id,
        TransactionType::Recharge,
    ))
}

/// Handle customer.subscription.created events
///
/// Grants subscription points when a new subscription is created.
async fn handle_subscription_created(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_subscription_created_payload(&event)?;
    let event_id = payload.event_id.as_str();

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing customer.subscription.created event"
    );

    // Extract price_id first so the price-aware resolver can use it.
    let external_price_id = event["data"]["object"]["items"]["data"][0]["price"]["id"]
        .as_str()
        .map(str::to_string);

    // Resolve the price-level entitlement (projection key + strategy mapping).
    // Always run through the price-aware resolver so the strategy mapping is
    // price-level (kills shared-key ambiguity; US-EM-008). When
    // metadata carries `herald_entitlement_key`, the resolver re-locates the
    // mapping by (key, price); otherwise it resolves by (provider, product, price).
    let resolved = resolve_stripe_entitlement(
        &app_state,
        realm_id,
        &event["data"]["object"]["metadata"],
        &payload.external_product_id,
        external_price_id.as_deref(),
    )
    .await?;
    let entitlement_key = if payload.entitlement_key.is_empty() {
        resolved.entitlement_key.clone()
    } else {
        payload.entitlement_key.clone()
    };
    let strategy_mapping = resolved.mapping;

    let (subscription, _previous_subscription) = sync_stripe_subscription_with_history_in_txn(
        &app_state,
        realm_id,
        payload.user_id,
        &payload.stripe_subscription_id,
        payload.client_app_id,
        entitlement_key.clone(),
        payload.external_product_id.clone(),
        external_price_id,
        payload.status.clone(),
        payload.current_period_start,
        payload.current_period_end,
        payload.cancel_at_period_end,
        None,
        None,
        HistoryEventType::Created,
    )
    .await?;

    // Normalize the provider billing period (P0). Stripe
    // 2025-03-31.basil moved `current_period_*` from the subscription top
    // level to each subscription item; older API versions keep them at the
    // top level. `normalize_stripe_period` reconciles both shapes and
    // returns `None` when the points entitlement's period cannot be uniquely
    // resolved (e.g. multiple items with disagreeing periods) — per P0 we
    // must NOT guess the period from event time and must NOT write a ledger
    // with an invented period; we skip the grant and await a later
    // webhook / API compensation.
    let normalized_period = normalize_stripe_period(&event["data"]["object"]);
    if let Some((period_start, period_end)) = normalized_period {
        // Route grants through the subscription source. The synced
        // subscription is created non-null, so the persisted bucket_id is the
        // authoritative routing target.
        app_state
            .subscription_service
            .handle_subscription_paid(
                payload.user_id,
                subscription.id,
                realm_id,
                &strategy_mapping,
                false,
                period_start,
                period_end,
                payload.event_id.clone(),
            )
            .await?;
    } else {
        warn!(
            realm_id = %realm_id,
            user_id = %payload.user_id,
            stripe_subscription_id = %payload.stripe_subscription_id,
            event_id = %event_id,
            reason = "period_uniquely_unresolvable",
            source = "stripe",
            "Stripe period normalization failed; skipping subscription grant and awaiting compensation (P0)"
        );
    }

    info!(
        realm_id = %realm_id,
        user_id = %payload.user_id,
        entitlement_key = %entitlement_key,
        stripe_subscription_id = %payload.stripe_subscription_id,
        event_id = %event_id,
        current_period_end = ?payload.current_period_end,
        "Subscription created - ledger and aggregate synced"
    );

    Ok(create_placeholder_transaction(
        payload.user_id,
        realm_id,
        TransactionType::SubscriptionGrant,
    ))
}

/// Handle customer.subscription.updated events
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

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing customer.subscription.updated event"
    );

    // Resolve the CURRENT (new) price-level entitlement (projection key +
    // strategy mapping) via the price-aware chain (US-EM-008).
    // Always run the resolver so the strategy mapping is price-level, killing
    // shared-key ambiguity (e.g. monthly 1000 vs annual 12000 under `pro-plan`).
    let current_price_id = event["data"]["object"]["items"]["data"][0]["price"]["id"].as_str();
    let current_resolved = resolve_stripe_entitlement(
        &app_state,
        realm_id,
        &event["data"]["object"]["items"]["data"][0]["price"]["metadata"],
        &payload.external_product_id,
        current_price_id,
    )
    .await?;
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
            .find_by_external_subscription_id(&payload.stripe_subscription_id, "stripe")
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
            // Pre-migration subscription with no entitlement_key — resolve via mapping.
            // Use the previous_attributes price id (the prior plan's price).
            let previous_price_id =
                event["data"]["previous_attributes"]["items"]["data"][0]["price"]["id"].as_str();
            resolve_stripe_entitlement_key(
                &app_state,
                realm_id,
                &event["data"]["previous_attributes"]["items"]["data"][0]["price"]["metadata"],
                &payload.external_product_id,
                previous_price_id,
            )
            .await?
        } else {
            from_db
        }
    } else {
        payload.previous_entitlement_key.clone()
    };

    if previous_entitlement_key == current_entitlement_key {
        let external_price_id = event["data"]["object"]["items"]["data"][0]["price"]["id"]
            .as_str()
            .map(str::to_string);
        let (_subscription, _previous_subscription) =
            sync_stripe_subscription_with_detected_history_in_txn(
                &app_state,
                realm_id,
                payload.user_id,
                &payload.stripe_subscription_id,
                None,
                current_entitlement_key.clone(),
                payload.external_product_id.clone(),
                external_price_id,
                payload.status.clone(),
                payload.current_period_start,
                payload.current_period_end,
                payload.cancel_at_period_end,
                if payload.cancel_at_period_end {
                    payload.current_period_end
                } else {
                    None
                },
                existing_subscription_for_update.clone(),
            )
            .await?;

        return Ok(create_placeholder_transaction(
            payload.user_id,
            realm_id,
            TransactionType::SubscriptionGrant,
        ));
    }

    // Resolve the PREVIOUS (old) price-level strategy mapping. The previous
    // entitlement comes from the prior subscription state (no ResolvedEntitlement
    // in scope here), so re-locate the price-level mapping by (entitlement_key,
    // price). This is the necessary price-level query — not a compat layer.
    // Price source: the `previous_attributes` price id (prior plan's price) when
    // present, else the existing subscription's bound `external_price_id`.
    let previous_price_id = event["data"]["previous_attributes"]["items"]["data"][0]["price"]["id"]
        .as_str()
        .or_else(|| {
            existing_subscription_for_update
                .as_ref()
                .and_then(|s| s.external_price_id.as_deref())
        });
    let old_mapping = app_state
        .billing_repository
        .find_entitlement_mapping_by_key_price(
            realm_id,
            &previous_entitlement_key,
            previous_price_id,
        )
        .await?
        .ok_or_else(|| {
            CoreError::InternalServerError(format!(
                "Entitlement mapping not found for previous key '{}' during subscription update",
                previous_entitlement_key
            ))
        })?;

    let is_upgrade = mapping_rule_value(&app_state, realm_id, new_mapping.id).await?
        > mapping_rule_value(&app_state, realm_id, old_mapping.id).await?;

    if is_upgrade {
        let external_price_id = event["data"]["object"]["items"]["data"][0]["price"]["id"]
            .as_str()
            .map(str::to_string);
        let (subscription, _previous_subscription) = sync_stripe_subscription_with_history_in_txn(
            &app_state,
            realm_id,
            payload.user_id,
            &payload.stripe_subscription_id,
            None,
            current_entitlement_key.clone(),
            payload.external_product_id.clone(),
            external_price_id,
            payload.status.clone(),
            payload.current_period_start,
            payload.current_period_end,
            payload.cancel_at_period_end,
            if payload.cancel_at_period_end {
                payload.current_period_end
            } else {
                None
            },
            existing_subscription_for_update.clone(),
            HistoryEventType::Upgraded,
        )
        .await?;

        // See handle_subscription_created: Stripe may omit current_period_end on
        // events fired while the subscription is incomplete; fall back to +30 days.
        let period_end = payload
            .current_period_end
            .unwrap_or_else(|| Utc::now() + ChronoDuration::days(30));

        app_state
            .subscription_service
            .handle_subscription_upgrade(
                payload.user_id,
                realm_id,
                subscription.id,
                &new_mapping,
                period_end,
                &payload.event_id,
            )
            .await?;

        info!(
            realm_id = %realm_id,
            user_id = %payload.user_id,
            stripe_subscription_id = %payload.stripe_subscription_id,
            old_entitlement_key = %previous_entitlement_key,
            new_entitlement_key = %current_entitlement_key,
            event_id = %event_id,
            "Processed subscription upgrade"
        );

        Ok(create_placeholder_transaction(
            payload.user_id,
            realm_id,
            TransactionType::SubscriptionUpgrade,
        ))
    } else {
        let external_price_id = event["data"]["object"]["items"]["data"][0]["price"]["id"]
            .as_str()
            .map(str::to_string);
        let (subscription, _previous_subscription) = sync_stripe_subscription_with_history_in_txn(
            &app_state,
            realm_id,
            payload.user_id,
            &payload.stripe_subscription_id,
            None,
            current_entitlement_key.clone(),
            payload.external_product_id.clone(),
            external_price_id,
            payload.status.clone(),
            payload.current_period_start,
            payload.current_period_end,
            payload.cancel_at_period_end,
            if payload.cancel_at_period_end {
                payload.current_period_end
            } else {
                None
            },
            existing_subscription_for_update.clone(),
            HistoryEventType::Downgraded,
        )
        .await?;

        app_state
            .subscription_service
            .handle_subscription_downgrade(
                payload.user_id,
                subscription.id,
                realm_id,
                &old_mapping,
                &new_mapping,
            )
            .await?;

        info!(
            realm_id = %realm_id,
            user_id = %payload.user_id,
            stripe_subscription_id = %payload.stripe_subscription_id,
            old_entitlement_key = %previous_entitlement_key,
            new_entitlement_key = %current_entitlement_key,
            event_id = %event_id,
            "Processed subscription downgrade"
        );

        // Placeholder for idempotency; actual downgrade classification is in
        // subscription_history via HistoryEventType::Downgraded above.
        Ok(create_placeholder_transaction(
            payload.user_id,
            realm_id,
            TransactionType::SubscriptionDowngrade,
        ))
    }
}

/// Handle customer.subscription.paused / customer.subscription.resumed events
///
/// Lightweight handler that syncs status without upgrade/downgrade logic.
/// Paused/resumed events typically lack `previous_attributes.items`, so using
/// `handle_subscription_updated` would produce an empty `previous_entitlement_key`
/// and trigger incorrect upgrade/downgrade logic.
async fn handle_subscription_status_change(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let event_id = parse_event_id(&event)?;
    let object = &event["data"]["object"];
    let stripe_subscription_id = object["id"]
        .as_str()
        .ok_or_else(|| CoreError::BadRequest("Missing subscription id".to_string()))?
        .to_string();
    let metadata = &object["metadata"];
    let user_id = parse_uuid_field(
        metadata_value(metadata, "herald_user_id", "userId"),
        "userId",
    )?;
    let cancel_at_period_end = object["cancel_at_period_end"].as_bool().unwrap_or(false);
    let status = parse_stripe_subscription_status(object["status"].as_str(), cancel_at_period_end)?;
    let external_product_id = object["items"]["data"][0]["price"]["product"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_default();
    let entitlement_key = metadata["herald_entitlement_key"]
        .as_str()
        .or_else(|| metadata["entitlementKey"].as_str())
        .unwrap_or("")
        .to_string();
    let external_price_id = object["items"]["data"][0]["price"]["id"]
        .as_str()
        .map(str::to_string);
    let current_period_start = parse_optional_stripe_datetime(&object["current_period_start"])?;
    let current_period_end = parse_optional_stripe_datetime(&object["current_period_end"])?;

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        stripe_subscription_id = %stripe_subscription_id,
        status = ?status,
        "Processing customer.subscription paused/resumed event"
    );

    // Resolve entitlement_key via price-aware fallback chain
    let entitlement_key = if entitlement_key.is_empty() {
        resolve_stripe_entitlement_key(
            &app_state,
            realm_id,
            metadata,
            &external_product_id,
            external_price_id.as_deref(),
        )
        .await?
    } else {
        entitlement_key
    };

    let existing_sub = app_state
        .billing_repository
        .find_by_external_subscription_id(&stripe_subscription_id, "stripe")
        .await?;

    let _synced = sync_stripe_subscription_with_detected_history_in_txn(
        &app_state,
        realm_id,
        user_id,
        &stripe_subscription_id,
        None,
        entitlement_key.clone(),
        external_product_id,
        external_price_id,
        status.clone(),
        current_period_start,
        current_period_end,
        cancel_at_period_end,
        if cancel_at_period_end {
            current_period_end
        } else {
            None
        },
        existing_sub,
    )
    .await?;

    Ok(create_placeholder_transaction(
        user_id,
        realm_id,
        TransactionType::SubscriptionGrant,
    ))
}

/// Handle customer.subscription.deleted events
///
/// Handles subscription cancellation (immediate or end-of-period).
async fn handle_subscription_deleted(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_subscription_deleted_payload(&event)?;
    let event_id = payload.event_id.as_str();

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing customer.subscription.deleted event"
    );

    let existing_subscription = app_state
        .billing_repository
        .find_by_external_subscription_id(&payload.stripe_subscription_id, "stripe")
        .await?;
    let entitlement_key = existing_subscription
        .as_ref()
        .map(|s| s.entitlement_key.clone())
        .or(payload.entitlement_key)
        .ok_or_else(|| CoreError::BadRequest("Missing or invalid entitlement_key".to_string()))?;
    let external_product_id = existing_subscription
        .as_ref()
        .map(|subscription| subscription.external_product_id.clone())
        .unwrap_or_default();
    let external_price_id = existing_subscription
        .as_ref()
        .and_then(|s| s.external_price_id.clone());
    let cancel_mode = if payload.cancel_at_period_end {
        CancelMode::DefaultCancel
    } else {
        CancelMode::ImmediateCancel
    };

    let status = if payload.cancel_at_period_end {
        SubscriptionStatus::ScheduledCancel
    } else {
        SubscriptionStatus::Canceled
    };

    let cancel_entitlement_key = entitlement_key.clone();

    let (subscription, _previous_subscription) = sync_stripe_subscription_with_history_in_txn(
        &app_state,
        realm_id,
        payload.user_id,
        &payload.stripe_subscription_id,
        existing_subscription
            .as_ref()
            .and_then(|subscription| subscription.client_app_id),
        entitlement_key,
        external_product_id,
        external_price_id,
        status,
        payload.current_period_start,
        payload.current_period_end,
        payload.cancel_at_period_end,
        Some(if payload.cancel_at_period_end {
            payload
                .current_period_end
                .unwrap_or_else(|| Utc::now() + ChronoDuration::days(30))
        } else {
            Utc::now()
        }),
        existing_subscription.clone(),
        HistoryEventType::Canceled,
    )
    .await?;

    // pre-grant ledger-row reclaim path is retired under the window quota
    // model. Idempotent: no active entitlement / already-revoked ⟹ no-op.
    // Route revocation through the subscription source. The synced
    // subscription is non-null.
    app_state
        .subscription_service
        .handle_subscription_cancel(
            payload.user_id,
            realm_id,
            subscription.id,
            cancel_mode,
            if payload.cancel_at_period_end {
                payload.current_period_end
            } else {
                None
            },
            Some(&cancel_entitlement_key),
        )
        .await?;

    info!(
        realm_id = %realm_id,
        user_id = %payload.user_id,
        stripe_subscription_id = %payload.stripe_subscription_id,
        event_id = %event_id,
        cancel_at_period_end = payload.cancel_at_period_end,
        "Subscription deleted - cancel flow completed"
    );

    Ok(create_placeholder_transaction(
        payload.user_id,
        realm_id,
        TransactionType::CancelRevoke,
    ))
}

/// Handle charge.refunded events
///
/// Revokes unused points based on refund type.
async fn handle_charge_refunded(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_charge_refunded_payload(&event)?;
    let event_id = payload.event_id.as_str();

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing charge.refunded event"
    );

    info!(
        realm_id = %realm_id,
        charge_id = %payload.charge_id,
        amount = payload.amount_refunded,
        refund_type = %payload.refund_type,
        user_id = %payload.user_id,
        event_id = %event_id,
        "Processing refund - revoking points"
    );

    // Resolve the subscription record up-front for both routing and the
    // history event below. The bucket source-of-truth for a refund revocation
    // is the bucket the original grant targeted:
    //   - topup (one-time): captured payment rule snapshots
    //   - subscription: source-derived distribution results
    let subscription = if let Some(subscription_id) = payload.subscription_id {
        app_state
            .billing_repository
            .find_subscription_by_id(subscription_id)
            .await?
    } else {
        None
    };

    match payload.refund_type.as_str() {
        "topup" => {
            // Look up the originating payment_attempt snapshot for the routing
            // Bucket (Stripe charge_id is stored as the provider reference).
            // Fail loud when the snapshot is missing or has no bucket.
            let attempt = app_state
                .payment_attempt_service
                .get_payment_attempt_by_provider_reference("stripe", &payload.charge_id)
                .await
                .map_err(|e| {
                    tracing::error!(
                        realm_id = %realm_id,
                        charge_id = %payload.charge_id,
                        error = %e,
                        "Failed to look up payment_attempt for refund bucket resolution"
                    );
                    CoreError::InternalServerError(format!(
                        "Failed to resolve bucket for refund {}: {e}",
                        payload.charge_id
                    ))
                })?
                .ok_or_else(|| {
                    CoreError::BadRequest(format!(
                        "Cannot resolve bucket for refund: no payment_attempt for charge_id {}",
                        payload.charge_id
                    ))
                })?;
            // The provider-reference lookup is realm-free; a refund signed for
            // this realm must not revoke against another realm's attempt.
            if attempt.realm_id != realm_id {
                return Err(CoreError::BadRequest(format!(
                    "Stripe refund realm mismatch for charge_id {}",
                    payload.charge_id
                )));
            }
            let _output = app_state
                .points_service
                .revoke_topup_source_proportional(
                    realm_id,
                    payload.user_id,
                    &attempt.id.to_string(),
                    payload.amount_refunded,
                    payload.amount,
                    event_id,
                )
                .await?;

            // Revoke payment-granted permanent roles for this one-time attempt
            // `source_id = attempt.id`, so revoke with the same source id.
            // Idempotent (NotFound is a no-op); manual grants unaffected.
            revoke_payment_roles_for_attempt(
                &app_state,
                realm_id,
                payload.user_id,
                &attempt.id.to_string(),
            )
            .await;

            info!(
                realm_id = %realm_id,
                user_id = %payload.user_id,
                charge_id = %payload.charge_id,
                amount = payload.amount_refunded,
                "Topup refund - proportionally revoked topup credits"
            );
        }
        _ => {
            // subscription's active quota entitlement by `source_id =
            // `revoke_subscription_unused` ledger-row reclaim is retired under
            // the window quota model. Route through the subscription source. Fail
            // loud when no subscription could be resolved for the refund.
            let subscription = subscription.as_ref().ok_or_else(|| {
                CoreError::BadRequest(format!(
                    "Cannot resolve bucket for subscription refund: no subscription for charge_id {}",
                    payload.charge_id
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

            info!(
                realm_id = %realm_id,
                user_id = %payload.user_id,
                charge_id = %payload.charge_id,
                subscription_id = %subscription.id,
                "Subscription refund - revoked subscription quota entitlement"
            );
        }
    }

    if let Some(subscription) = subscription {
        let history_event = SubscriptionHistoryService::create_subscription_refunded_event(
            &subscription,
            serde_json::json!({
                "provider": "stripe",
                "chargeId": payload.charge_id,
                "amountRefunded": payload.amount_refunded,
                "refundType": payload.refund_type,
            }),
            Some(ACTOR_WEBHOOK.to_string()),
        );
        app_state
            .billing_repository
            .save_history_event(history_event)
            .await?;
    }

    Ok(create_placeholder_transaction(
        payload.user_id,
        realm_id,
        TransactionType::RefundRevoke,
    ))
}

/// Handle invoice.payment_succeeded events
///
/// For subscription invoices: grants points on renewal.
/// For one-time payment invoices (no subscription): delegates to
/// `handle_stripe_invoice_event` for external invoice sync only, since there
/// is no subscription to renew and points were already granted by
/// `checkout.session.completed`.
async fn handle_invoice_payment_succeeded(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    // One-time payment invoices have no subscription field.
    // Delegate to the generic invoice sync handler instead of attempting
    // subscription renewal logic which requires a subscription.
    let has_subscription = event["data"]["object"]["subscription"].as_str().is_some();
    if !has_subscription {
        info!(
            realm_id = %realm_id,
            event_id = %parse_event_id(&event).unwrap_or_default(),
            "invoice.payment_succeeded without subscription — one-time payment invoice, delegating to sync handler"
        );
        return handle_stripe_invoice_event(app_state, event, realm_id, idempotency_key).await;
    }

    let payload = parse_invoice_paid_payload(&event)?;
    let event_id = payload.event_id.as_str();

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        "Processing invoice.payment_succeeded event"
    );

    let existing_subscription = app_state
        .billing_repository
        .find_by_external_subscription_id(&payload.stripe_subscription_id, "stripe")
        .await?;

    let external_product_id = existing_subscription
        .as_ref()
        .map(|subscription| subscription.external_product_id.clone())
        .unwrap_or_default();
    let external_price_id = existing_subscription
        .as_ref()
        .and_then(|s| s.external_price_id.clone());

    // Resolve the price-level entitlement (projection key + strategy mapping).
    // Always run the price-aware resolver so the strategy mapping is price-level
    // (US-EM-008). The webhook price is the subscription's bound
    // price (renewal of the same plan).
    let resolved = resolve_stripe_entitlement(
        &app_state,
        realm_id,
        &event["data"]["object"]["metadata"],
        &external_product_id,
        external_price_id.as_deref(),
    )
    .await?;
    let entitlement_key = if payload.entitlement_key.is_empty() {
        resolved.entitlement_key.clone()
    } else {
        payload.entitlement_key.clone()
    };
    let strategy_mapping = resolved.mapping;

    let (subscription, _previous_subscription) = sync_stripe_subscription_with_history_in_txn(
        &app_state,
        realm_id,
        payload.user_id,
        &payload.stripe_subscription_id,
        existing_subscription
            .as_ref()
            .and_then(|subscription| subscription.client_app_id),
        entitlement_key.clone(),
        external_product_id,
        external_price_id,
        SubscriptionStatus::Active,
        payload.current_period_start,
        payload.current_period_end,
        false,
        None,
        existing_subscription.clone(),
        HistoryEventType::Renewed,
    )
    .await?;

    // Normalize the provider billing period (P0). For the
    // renewal grant the period is sourced from the Invoice object carried by
    // the `invoice.payment_succeeded` event: a Stripe Invoice has NO top-level
    // `current_period_*` (those are Subscription/SubscriptionItem fields) and
    // exposes its line items under `lines.data`; each subscription line's
    // `period.{start,end}` IS the subscription billing period being paid.
    // When the period cannot be uniquely resolved we skip the renewal grant
    // and emit a structured warning — never guess, never write a ledger with
    // an invented period (P0).
    let normalized_period = normalize_stripe_invoice_period(&event["data"]["object"]);
    if let Some((period_start, period_end)) = normalized_period {
        // Route grants through the subscription source. The synced
        // subscription is non-null.
        app_state
            .subscription_service
            .handle_subscription_paid(
                payload.user_id,
                subscription.id,
                realm_id,
                &strategy_mapping,
                true,
                period_start,
                period_end,
                payload.event_id.clone(),
            )
            .await?;
    } else {
        warn!(
            realm_id = %realm_id,
            user_id = %payload.user_id,
            stripe_subscription_id = %payload.stripe_subscription_id,
            event_id = %event_id,
            reason = "period_uniquely_unresolvable",
            source = "stripe",
            "Stripe period normalization failed on renewal; skipping grant and awaiting compensation (P0)"
        );
    }

    info!(
        realm_id = %realm_id,
        user_id = %payload.user_id,
        entitlement_key = %entitlement_key,
        stripe_subscription_id = %payload.stripe_subscription_id,
        event_id = %event_id,
        current_period_end = ?payload.current_period_end,
        "Invoice payment succeeded - renewal ledger granted"
    );

    // Record a renewal payment_attempt and backfill invoice attribution.
    // Best-effort: a failure here must NOT block the credit grant that has already
    // succeeded above — it is logged and the webhook transaction is replayed by the
    // existing `payment_event` compensation framework.
    // amount == 0 → skip (zero-yuan cycle: no actual charge, and `payment_attempts.amount`
    // has CHECK(amount > 0)). The renewal ledger/period update above already happened.
    let invoice_object = &event["data"]["object"];
    let renewal_amount = invoice_object["total"].as_i64().unwrap_or(0);
    let renewal_currency = invoice_object["currency"]
        .as_str()
        .unwrap_or("usd")
        .to_string();
    let stripe_invoice_id = invoice_object["id"].as_str().map(str::to_string);

    if renewal_amount > 0
        && let Some(stripe_invoice_id) = stripe_invoice_id.as_ref()
    {
        let provider_reference = format!(
            "stripe_renewal:{}:{}",
            payload.stripe_subscription_id, stripe_invoice_id
        );
        let completed_at = payload.current_period_end.unwrap_or_else(Utc::now);

        let renewal_outcome = async {
            let renewal_attempt = app_state
                .payment_attempt_service
                .record_subscription_renewal_attempt(RecordRenewalAttemptInput {
                    realm_id: realm_id.to_string(),
                    user_id: payload.user_id,
                    payment_provider: "stripe".to_string(),
                    target_id: strategy_mapping.id,
                    amount: renewal_amount,
                    currency: renewal_currency.clone(),
                    provider_reference: provider_reference.clone(),
                    completed_at,
                })
                .await?;

            // Re-upsert the invoice with attribution. Reuses the FULL field
            // construction (hosted_url/pdf_url/external_payload/external_order_id) via
            // the shared helper so the ON CONFLICT branches do NOT regress fields
            // written by the earlier `invoice.*` sync event.
            let external_data = build_stripe_invoice_external_data(
                invoice_object,
                realm_id,
                stripe_invoice_id,
                Some(payload.user_id),
                Some(subscription.id),
                Some(renewal_attempt.id),
            )?;

            app_state
                .invoice_repository
                .upsert_external_invoice(external_data)
                .await?;

            Ok::<(), CoreError>(())
        }
        .await;

        if let Err(e) = renewal_outcome {
            warn!(
                realm_id = %realm_id,
                user_id = %payload.user_id,
                stripe_subscription_id = %payload.stripe_subscription_id,
                stripe_invoice_id = %stripe_invoice_id,
                event_id = %event_id,
                error = %e,
                "Stripe renewal attempt/invoice attribution failed - credits already granted; compensation will retry"
            );
        }
    } else if renewal_amount == 0 {
        info!(
            realm_id = %realm_id,
            user_id = %payload.user_id,
            stripe_subscription_id = %payload.stripe_subscription_id,
            event_id = %event_id,
            "Stripe renewal invoice total=0 — skipping renewal payment_attempt and invoice attribution (zero-yuan cycle)"
        );
    }

    Ok(create_placeholder_transaction(
        payload.user_id,
        realm_id,
        TransactionType::SubscriptionRenewal,
    ))
}

/// Buyer snapshot extracted from a provider payload.
struct StripeBuyerInfo {
    billing_name: Option<String>,
    billing_email: Option<String>,
    billing_phone: Option<String>,
    billing_address: Option<String>,
}

/// Extract buyer snapshot fields from a Stripe object.
///
/// Stripe surfaces buyer info differently across object types:
/// - **Invoice** objects carry `customer_name` / `customer_email` /
///   `customer_phone` / `customer_address` (the latter as a structured object).
/// - **Checkout Session** objects carry `customer_details.{name,email,phone,address}`
///   and a top-level `customer_email`.
///
/// We prefer the Invoice-level fields (richer, present on `invoice.*` events) and
/// fall back to Checkout-Session `customer_details` so the helper works for both
/// event shapes. The structured `customer_address` is flattened into a single
/// comma-joined line to match the `billing_address TEXT` column.
fn extract_stripe_buyer(object: &Value) -> StripeBuyerInfo {
    let details = &object["customer_details"];
    let billing_name = object["customer_name"]
        .as_str()
        .or_else(|| details["name"].as_str())
        .map(str::to_string);
    let billing_email = object["customer_email"]
        .as_str()
        .or_else(|| details["email"].as_str())
        .map(str::to_string);
    let billing_phone = object["customer_phone"]
        .as_str()
        .or_else(|| details["phone"].as_str())
        .map(str::to_string);
    let billing_address = flatten_stripe_address(&object["customer_address"])
        .or_else(|| flatten_stripe_address(&details["address"]));

    StripeBuyerInfo {
        billing_name,
        billing_email,
        billing_phone,
        billing_address,
    }
}

/// Flatten a Stripe structured address object ({line1, line2, city, state,
/// postal_code, country}) into a single non-empty line string. Returns None for
/// an absent/empty address so COALESCE on upsert preserves any prior value.
fn flatten_stripe_address(addr: &Value) -> Option<String> {
    if !addr.is_object() {
        return None;
    }
    let parts = ["line1", "line2", "city", "state", "postal_code", "country"]
        .iter()
        .filter_map(|k| addr.get(k).and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// Build the `ExternalInvoiceData` for a Stripe invoice upsert.
///
/// Shared by:
/// - `handle_stripe_invoice_event` (invoice.created/finalized/paid/voided sync) — fills
///   `subscription_id` when resolvable, `payment_attempt_id = None` (renewal attempt is
///   created by `invoice.payment_succeeded`, not this path).
/// - `handle_invoice_payment_succeeded` renewal re-upsert — fills both `subscription_id`
///   and `payment_attempt_id` to backfill attribution on the same `external_invoice_id`.
///
/// Carrying the FULL field set (hosted_url / pdf_url / external_payload /
/// external_order_id = payment_intent) on every call is intentional: the upsert's
/// ON CONFLICT branches COALESCE these, so a re-upsert from the renewal path must NOT
/// pass `None` for them, otherwise it would regress fields written by an earlier
/// `invoice.*` sync event.
fn build_stripe_invoice_external_data(
    object: &Value,
    realm_id: &str,
    stripe_invoice_id: &str,
    account_id: Option<Uuid>,
    subscription_id: Option<Uuid>,
    payment_attempt_id: Option<Uuid>,
) -> Result<ExternalInvoiceData, CoreError> {
    let stripe_status = object["status"].as_str().unwrap_or("draft");
    let status = map_stripe_invoice_status(stripe_status)?;
    let total = object["total"].as_i64().unwrap_or(0);
    let currency = object["currency"].as_str().unwrap_or("usd").to_string();
    let buyer = extract_stripe_buyer(object);

    Ok(ExternalInvoiceData {
        realm_id: realm_id.to_string(),
        provider: InvoiceProvider::Stripe,
        payment_provider: Some("stripe".to_string()),
        external_invoice_id: Some(stripe_invoice_id.to_string()),
        external_order_id: object["payment_intent"].as_str().map(str::to_string),
        external_status: Some(stripe_status.to_string()),
        external_hosted_url: object["hosted_invoice_url"].as_str().map(str::to_string),
        external_pdf_url: object["invoice_pdf"].as_str().map(str::to_string),
        external_payload: Some(object.clone()),
        tax_details: None,
        account_id,
        applicant_user_id: None,
        billing_name: buyer.billing_name,
        billing_email: buyer.billing_email,
        billing_phone: buyer.billing_phone,
        billing_address: buyer.billing_address,
        currency,
        total,
        status,
        subscription_id,
        payment_attempt_id,
    })
}

/// Handle invoice.created / invoice.finalized / invoice.paid / invoice.voided events
///
/// Syncs Stripe invoice state to Herald external invoice via upsert.
/// This is distinct from `invoice.payment_succeeded` which handles credit granting.
async fn handle_stripe_invoice_event(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let event_id = parse_event_id(&event)?;
    let object = &event["data"]["object"];

    let stripe_invoice_id = object["id"]
        .as_str()
        .ok_or_else(|| CoreError::BadRequest("Missing Stripe invoice id".to_string()))?
        .to_string();

    // Resolve account_id and local subscription_id: try subscription lookup first,
    // then metadata userId for account_id. subscription_id stays None when the
    // subscription cannot be resolved (e.g. invoice.created arriving before the
    // subscription is synced); the renewal attribution is backfilled by
    // `handle_invoice_payment_succeeded` instead.
    let stripe_subscription_id = object["subscription"].as_str();
    let mut account_id: Option<Uuid> = None;
    let mut subscription_id: Option<Uuid> = None;

    if let Some(stripe_sub_id) = stripe_subscription_id
        && let Ok(Some(subscription)) = app_state
            .billing_repository
            .find_by_external_subscription_id(stripe_sub_id, "stripe")
            .await
    {
        subscription_id = Some(subscription.id);
        account_id = Some(subscription.user_id);
    }

    if account_id.is_none() {
        account_id = metadata_user_id(&object["metadata"]);
    }

    if account_id.is_none() {
        warn!(
            realm_id = %realm_id,
            event_id = %event_id,
            stripe_invoice_id = %stripe_invoice_id,
            "Could not resolve account_id for Stripe invoice event - creating with account_id=None"
        );
    }

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        stripe_invoice_id = %stripe_invoice_id,
        account_id = ?account_id,
        subscription_id = ?subscription_id,
        "Processing Stripe invoice event - upserting external invoice"
    );

    let external_data = build_stripe_invoice_external_data(
        object,
        realm_id,
        &stripe_invoice_id,
        account_id,
        subscription_id,
        // Renewal attempt is created by `invoice.payment_succeeded`, not this sync path.
        None,
    )?;

    app_state
        .invoice_repository
        .upsert_external_invoice(external_data)
        .await?;

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        stripe_invoice_id = %stripe_invoice_id,
        "Stripe invoice event processed - external invoice upserted"
    );

    // SubscriptionGrant used as placeholder type for non-subscription invoices too;
    // the returned transaction has a random account_id and zero amount, so the type
    // is semantically irrelevant — no SubscriptionGrant-specific side effects occur.
    Ok(create_placeholder_transaction(
        Uuid::now_v7(),
        realm_id,
        TransactionType::SubscriptionGrant,
    ))
}

async fn find_stripe_subscription_from_metadata(
    app_state: &AppState,
    realm_id: &str,
    object: &Value,
) -> Result<Option<Subscription>, CoreError> {
    // Primary path: look for herald-specific keys in object metadata.
    if let Some(subscription_id) = parse_optional_uuid_field(metadata_value(
        &object["metadata"],
        "herald_subscription_id",
        "subscriptionId",
    )) {
        return app_state
            .billing_repository
            .find_subscription_by_id(subscription_id)
            .await;
    }

    if let Some(external_subscription_id) = object["metadata"]["herald_external_subscription_id"]
        .as_str()
        .or_else(|| object["metadata"]["externalSubscriptionId"].as_str())
    {
        return app_state
            .billing_repository
            .find_by_external_subscription_id(external_subscription_id, "stripe")
            .await;
    }

    // Fallback for dispute objects: their metadata is dispute-level, not the original
    // charge/subscription metadata. Use payment_intent to trace back to the subscription
    // via previously stored payment events (e.g. checkout.session.completed).
    if let Some(payment_intent) = object["payment_intent"].as_str() {
        let stripe_subscription_id = app_state
            .billing_repository
            .find_external_subscription_id_by_payment_intent(payment_intent, "stripe", realm_id)
            .await?;

        if let Some(stripe_sub_id) = stripe_subscription_id
            && let Some(sub) = app_state
                .billing_repository
                .find_by_external_subscription_id(&stripe_sub_id, "stripe")
                .await?
        {
            return Ok(Some(sub));
        }
    }

    Ok(None)
}

async fn handle_charge_dispute_created(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let event_id = parse_event_id(&event)?;
    let object = &event["data"]["object"];
    let Some(existing) =
        find_stripe_subscription_from_metadata(&app_state, realm_id, object).await?
    else {
        warn!(
            realm_id = %realm_id,
            event_id = %event_id,
            "Stripe dispute created could not be mapped to a local subscription - ignoring"
        );
        return Ok(create_placeholder_transaction(
            Uuid::now_v7(),
            realm_id,
            TransactionType::SubscriptionGrant,
        ));
    };
    let user_id = existing.user_id;
    let mut provider_metadata = existing
        .provider_metadata
        .clone()
        .unwrap_or(serde_json::json!({}));
    if let Some(obj) = provider_metadata.as_object_mut() {
        obj.insert("disputeId".to_string(), object["id"].clone());
        obj.insert("charge".to_string(), object["charge"].clone());
        obj.insert(
            "paymentIntent".to_string(),
            object["payment_intent"].clone(),
        );
        obj.insert("dispute_amount".to_string(), object["amount"].clone());
        obj.insert("dispute_reason".to_string(), object["reason"].clone());
    }

    let _synced = sync_subscription_input_with_history_in_txn(
        &app_state,
        SyncSubscriptionInput {
            provider: "stripe",
            realm_id: realm_id.to_string(),
            user_id: Some(user_id),
            external_subscription_id: existing.external_subscription_id.clone(),
            external_product_id: existing.external_product_id.clone(),
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
        HistoryEventType::Disputed,
    )
    .await?;

    Ok(create_placeholder_transaction(
        user_id,
        realm_id,
        TransactionType::SubscriptionGrant,
    ))
}

async fn handle_charge_dispute_closed(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let event_id = parse_event_id(&event)?;
    let object = &event["data"]["object"];
    let Some(existing) =
        find_stripe_subscription_from_metadata(&app_state, realm_id, object).await?
    else {
        warn!(
            realm_id = %realm_id,
            event_id = %event_id,
            "Stripe dispute closed could not be mapped to a local subscription - ignoring"
        );
        return Ok(create_placeholder_transaction(
            Uuid::now_v7(),
            realm_id,
            TransactionType::SubscriptionGrant,
        ));
    };
    let user_id = existing.user_id;
    let dispute_status = object["status"].as_str().unwrap_or("");
    let needs_cancel = dispute_status == "lost";
    let target_status = match dispute_status {
        "lost" => SubscriptionStatus::Canceled,
        "won" => SubscriptionStatus::Active,
        // warning_closed, charge_refunded, etc. — log and stay in current state
        _ => {
            warn!(
                realm_id = %realm_id,
                event_id = %event_id,
                dispute_status = %dispute_status,
                "Stripe dispute closed with non-terminal status — not reactivating"
            );
            existing.status.clone()
        }
    };

    let dispute_entitlement_key = existing.entitlement_key.clone();
    let synced = sync_subscription_input_with_detected_history_in_txn(
        &app_state,
        SyncSubscriptionInput {
            provider: "stripe",
            realm_id: realm_id.to_string(),
            user_id: Some(user_id),
            external_subscription_id: existing.external_subscription_id.clone(),
            external_product_id: existing.external_product_id.clone(),
            client_app_id: existing.client_app_id,
            entitlement_key: existing.entitlement_key.clone(),
            external_price_id: existing.external_price_id.clone(),
            provider_metadata: existing.provider_metadata.clone(),
            status: target_status.clone(),
            current_period_start: existing.current_period_start,
            current_period_end: existing.current_period_end,
            cancel_at_period_end: existing.cancel_at_period_end,
            cancel_at: if target_status == SubscriptionStatus::Canceled {
                Some(Utc::now())
            } else {
                existing.cancel_at
            },
            existing_subscription: Some(existing),
        },
    )
    .await?;

    if synced.is_some() && needs_cancel {
        let subscription_id = synced
            .as_ref()
            .map(|(subscription, _)| subscription.id)
            .ok_or_else(|| {
                CoreError::InternalServerError(
                    "dispute close sync returned no subscription for cancel".to_string(),
                )
            })?;

        app_state
            .subscription_service
            .handle_subscription_cancel(
                user_id,
                realm_id,
                subscription_id,
                CancelMode::ImmediateCancel,
                None,
                Some(dispute_entitlement_key.as_str()),
            )
            .await?;
    }

    Ok(create_placeholder_transaction(
        user_id,
        realm_id,
        TransactionType::SubscriptionGrant,
    ))
}

async fn handle_invoice_payment_action_required(
    event: Value,
    realm_id: &str,
) -> Result<PointsTransaction, CoreError> {
    warn!(
        realm_id = %realm_id,
        event_id = %parse_event_id(&event)?,
        invoice_id = ?event["data"]["object"]["id"].as_str(),
        "Stripe invoice payment requires customer action"
    );

    Ok(create_placeholder_transaction(
        Uuid::now_v7(),
        realm_id,
        TransactionType::SubscriptionGrant,
    ))
}

/// Handle `credit_note.created` events.
///
/// When Stripe issues a credit note on an invoice, Herald mirrors it as a local
/// Credit Note record and updates the parent invoice's `amount_refunded` /
/// `amount_remaining`. Non-Stripe invoices are skipped (warn + placeholder).
/// Missing local invoices are retried by returning an internal-server error so
/// Stripe redelivers the event (the credit note must apply to the invoice once
/// it syncs, otherwise it would be silently lost).
async fn handle_credit_note_created(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_credit_note_created_payload(&event)?;
    let event_id = payload.event_id.as_str();

    // Case 1: invoice not found for this Stripe invoice id — return a transient
    // error so the webhook layer surfaces 5xx and Stripe redelivers. If we
    // returned a placeholder OK here, the credit note would never be applied
    // even after invoice.payment_succeeded syncs the parent invoice.
    let Some(invoice) = app_state
        .invoice_repository
        .find_by_external_invoice_id(realm_id, &payload.stripe_invoice_id)
        .await?
    else {
        warn!(
            realm_id = %realm_id,
            event_id = %event_id,
            stripe_invoice_id = %payload.stripe_invoice_id,
            stripe_credit_note_id = %payload.stripe_credit_note_id,
            "Stripe credit_note.created: no matching local invoice — returning error to trigger Stripe retry"
        );
        return Err(CoreError::InternalServerError(format!(
            "Invoice {} not yet synced for credit note {} — request Stripe redelivery",
            payload.stripe_invoice_id, payload.stripe_credit_note_id
        )));
    };

    // Case 2: invoice belongs to a non-stripe provider — skip (warn + placeholder).
    if invoice.provider != InvoiceProvider::Stripe {
        warn!(
            realm_id = %realm_id,
            event_id = %event_id,
            invoice_id = %invoice.id,
            provider = %invoice.provider.as_str(),
            stripe_invoice_id = %payload.stripe_invoice_id,
            "Stripe credit_note.created: invoice provider is not stripe — skipping"
        );
        return Ok(create_placeholder_transaction(
            Uuid::now_v7(),
            realm_id,
            TransactionType::RefundRevoke,
        ));
    }

    // Case 3: idempotency — this Stripe credit note id was already processed.
    if let Some(existing) = app_state
        .credit_note_repository
        .find_by_external_id(&payload.stripe_credit_note_id)
        .await?
    {
        info!(
            realm_id = %realm_id,
            event_id = %event_id,
            invoice_id = %invoice.id,
            stripe_credit_note_id = %payload.stripe_credit_note_id,
            status = ?existing.status,
            "Stripe credit_note.created: already processed — skipping"
        );
        return Ok(create_placeholder_transaction(
            Uuid::now_v7(),
            realm_id,
            TransactionType::RefundRevoke,
        ));
    }

    // Case 4: normal create — verify currency matches the invoice before recording.
    // A mismatch indicates a Stripe config issue; failing loud is safer than
    // silently persisting a wrong-currency refund.
    if payload.currency.to_uppercase() != invoice.currency.to_uppercase() {
        warn!(
            realm_id = %realm_id,
            event_id = %event_id,
            invoice_id = %invoice.id,
            invoice_currency = %invoice.currency,
            payload_currency = %payload.currency,
            stripe_credit_note_id = %payload.stripe_credit_note_id,
            "Stripe credit_note.created: currency mismatch — rejecting"
        );
        return Err(CoreError::BadRequest(format!(
            "Credit note currency {} does not match invoice currency {}",
            payload.currency, invoice.currency
        )));
    }

    let input = NewCreditNote {
        invoice_id: invoice.id,
        realm_id: realm_id.to_string(),
        amount: payload.amount,
        currency: payload.currency.clone(),
        source: CreditNoteSource::Stripe,
        external_credit_note_id: Some(payload.stripe_credit_note_id.clone()),
        memo: None,
        created_by_user_id: None,
    };

    let created = app_state
        .credit_note_repository
        .create_credit_note_and_update_invoice(input)
        .await?;

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        invoice_id = %invoice.id,
        credit_note_id = %created.id,
        stripe_credit_note_id = %payload.stripe_credit_note_id,
        amount = payload.amount,
        currency = %payload.currency,
        "Stripe credit_note.created: credit note created and invoice refund totals updated"
    );

    Ok(create_placeholder_transaction(
        Uuid::now_v7(),
        realm_id,
        TransactionType::RefundRevoke,
    ))
}

/// Handle `credit_note.voided` events.
///
/// When Stripe voids a credit note, Herald marks the corresponding local credit
/// note as `voided` and reverses its amount on the parent invoice
/// (`amount_refunded -= amount`, `amount_remaining += amount`).
async fn handle_credit_note_voided(
    app_state: AppState,
    event: Value,
    realm_id: &str,
    _idempotency_key: &str,
) -> Result<PointsTransaction, CoreError> {
    let payload = parse_credit_note_voided_payload(&event)?;
    let event_id = payload.event_id.as_str();

    // Idempotency + out-of-order handling via the local credit note state.
    match app_state
        .credit_note_repository
        .find_by_external_id(&payload.stripe_credit_note_id)
        .await?
    {
        Some(existing) if existing.status == CreditNoteStatus::Voided => {
            info!(
                realm_id = %realm_id,
                event_id = %event_id,
                invoice_id = %existing.invoice_id,
                stripe_credit_note_id = %payload.stripe_credit_note_id,
                "Stripe credit_note.voided: already voided — skipping"
            );
            return Ok(create_placeholder_transaction(
                Uuid::now_v7(),
                realm_id,
                TransactionType::RefundRevoke,
            ));
        }
        Some(_) => {
            // Credit note exists and is active — void it now.
        }
        None => {
            // The credit_note.created event has not arrived yet. Return a
            // transient error so Stripe redelivers and the create-then-void
            // sequence applies in order. The invoice lookup is only needed in
            // this branch (for the diagnostic log) — avoid the SELECT on the
            // hot path where the credit note already exists.
            let invoice_present = app_state
                .invoice_repository
                .find_by_external_invoice_id(realm_id, &payload.stripe_invoice_id)
                .await?
                .is_some();
            warn!(
                realm_id = %realm_id,
                event_id = %event_id,
                stripe_invoice_id = %payload.stripe_invoice_id,
                stripe_credit_note_id = %payload.stripe_credit_note_id,
                invoice_present,
                "Stripe credit_note.voided: local credit note not found — returning error to trigger Stripe retry"
            );
            return Err(CoreError::InternalServerError(format!(
                "Credit note {} not yet created — request Stripe redelivery",
                payload.stripe_credit_note_id
            )));
        }
    }

    let voided = app_state
        .credit_note_repository
        .void_credit_note_by_external_id(realm_id, &payload.stripe_credit_note_id)
        .await?;

    info!(
        realm_id = %realm_id,
        event_id = %event_id,
        invoice_id = %voided.invoice_id,
        credit_note_id = %voided.id,
        stripe_credit_note_id = %payload.stripe_credit_note_id,
        amount = payload.amount,
        "Stripe credit_note.voided: credit note voided and invoice refund totals reversed"
    );

    Ok(create_placeholder_transaction(
        Uuid::now_v7(),
        realm_id,
        TransactionType::RefundRevoke,
    ))
}

/// Handle Stripe webhook events
///
/// Verifies signature, checks idempotency, routes to appropriate handler,
/// and returns 200 OK immediately. Processing happens synchronously (for now).
#[tracing::instrument(
    // Governance: `body` is the raw provider payload
    // (Stripe event bodies may carry PII / customer data); `headers` carries
    // the `stripe-signature` header; `realm_id` is conservatively skipped.
    // Only the low-cardinality route template is recorded.
    skip(app_state, realm_id, headers, body),
    fields(http.route = "/api/billing/webhook")
)]
pub async fn handle_stripe_webhook(
    State(app_state): State<AppState>,
    Path(realm_id): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Result<StatusCode, CoreError> {
    let event: Value = serde_json::from_str(&body).map_err(|e| {
        error!("Failed to parse webhook JSON: {}", e);
        CoreError::BadRequest(format!("Invalid JSON: {}", e))
    })?;

    // Note: realm_id is now extracted from URL path parameter, not from metadata

    let signature = headers
        .get("stripe-signature")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            error!("Missing stripe-signature header");
            CoreError::BadRequest("Missing signature".to_string())
        })?;

    let webhook_secret = app_state
        .realm_config_repository
        .get(
            realm_id.to_string(),
            "stripe".to_string(),
            "webhook_secret".to_string(),
        )
        .await
        .map_err(|e| {
            error!("Failed to load webhook secret from database: {}", e);
            CoreError::InternalServerError(format!("Database error: {}", e))
        })?
        .filter(|c| c.enabled)
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

    herald_core::infrastructure::stripe::StripeClient::verify_webhook_signature(
        body.as_bytes(),
        signature,
        &webhook_secret,
    )?;

    let event_id = event["id"]
        .as_str()
        .ok_or_else(|| CoreError::BadRequest("Missing event id".to_string()))?
        .to_string();

    let event_type = event["type"]
        .as_str()
        .ok_or_else(|| CoreError::BadRequest("Missing event type".to_string()))?
        .to_string();

    let new_payment_event = PaymentEvent {
        id: Uuid::now_v7(),
        realm_id: realm_id.clone(),
        external_event_id: event_id.clone(),
        payment_provider: "stripe".to_string(),
        event_type: event_type.clone(),
        subscription_id: None,
        payload: event.clone(),
        processed: false,
        processing_started_at: None,
        created_at: chrono::Utc::now(),
    };

    // Check for existing payment event (idempotency check)
    let existing_event = app_state
        .billing_repository
        .find_payment_event_by_external_id(&event_id, "stripe")
        .await?;

    let saved_event = match existing_event {
        Some(existing) if existing.processed => {
            info!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                "Duplicate webhook event - returning OK"
            );
            return Ok(StatusCode::OK);
        }
        Some(existing) => {
            // Previous attempt failed (processed=false): retry by reusing this row
            info!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                "Retrying unprocessed webhook event"
            );
            existing
        }
        None => {
            // Save new payment event (handle race condition from concurrent webhooks)
            match app_state
                .billing_repository
                .create_payment_event(new_payment_event)
                .await
            {
                Ok(saved_event) => saved_event,
                Err(CoreError::DatabaseError(msg))
                    if msg.contains("unique constraint") || msg.contains("duplicate key") =>
                {
                    // Concurrent webhook with same event_id already inserted - other worker is handling it
                    info!(
                        realm_id = %realm_id,
                        event_id = %event_id,
                        event_type = %event_type,
                        "Concurrent webhook event already inserted - returning OK"
                    );
                    return Ok(StatusCode::OK);
                }
                Err(e) => return Err(e),
            }
        }
    };

    let idempotency_key = format!("stripe_{}", event_id);

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

    let result = process_stripe_event_with_retries(
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
            idempotency_service
                .save_result(&realm_id, &idempotency_key, &transaction)
                .await?;
            app_state
                .billing_repository
                .mark_payment_event_processed(saved_event.id)
                .await?;
            info!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                transaction_id = %transaction.id,
                "Stripe webhook processed successfully"
            );
            Ok(StatusCode::OK)
        }
        Err(e) => {
            let _ = idempotency_service
                .mark_failed(&realm_id, &idempotency_key)
                .await;
            error!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                error = %e,
                "Failed to process Stripe webhook event"
            );
            Err(e)
        }
    }
}

async fn process_stripe_event_with_retries(
    app_state: AppState,
    event: &Value,
    realm_id: &str,
    idempotency_key: &str,
    event_id: &str,
    event_type: &str,
) -> Result<PointsTransaction, CoreError> {
    const MAX_ATTEMPTS: u32 = 3;

    for attempt in 1..=MAX_ATTEMPTS {
        let result = process_stripe_event_once(
            app_state.clone(),
            event,
            realm_id,
            idempotency_key,
            event_id,
            event_type,
        )
        .await;

        match result {
            Ok(transaction) => return Ok(transaction),
            Err(e) if attempt < MAX_ATTEMPTS => {
                // Only retry transient errors (database, internal, network), not permanent ones
                let is_transient = matches!(
                    &e,
                    CoreError::DatabaseError(_)
                        | CoreError::InternalServerError(_)
                        | CoreError::RateLimitExceeded
                );
                if !is_transient {
                    return Err(e);
                }
                warn!(
                    realm_id = %realm_id,
                    event_id = %event_id,
                    event_type = %event_type,
                    attempt,
                    error = %e,
                    "Stripe webhook handler failed with transient error, retrying"
                );
                tokio::time::sleep(Duration::from_millis(100 * attempt as u64)).await;
            }
            Err(e) => return Err(e),
        }
    }

    unreachable!("retry loop always returns")
}

async fn process_stripe_event_once(
    app_state: AppState,
    event: &Value,
    realm_id: &str,
    idempotency_key: &str,
    event_id: &str,
    event_type: &str,
) -> Result<PointsTransaction, CoreError> {
    match event_type {
        "checkout.session.completed" => {
            handle_checkout_session_completed(
                app_state.clone(),
                event.clone(),
                realm_id,
                idempotency_key,
            )
            .await
        }
        "checkout.session.expired" => {
            handle_checkout_session_expired(
                app_state.clone(),
                event.clone(),
                realm_id,
                idempotency_key,
            )
            .await
        }
        "checkout.session.async_payment_succeeded" => {
            handle_checkout_session_async_succeeded(
                app_state.clone(),
                event.clone(),
                realm_id,
                idempotency_key,
            )
            .await
        }
        "checkout.session.async_payment_failed" => {
            handle_checkout_session_async_failed(
                app_state.clone(),
                event.clone(),
                realm_id,
                idempotency_key,
            )
            .await
        }
        "customer.subscription.created" => {
            handle_subscription_created(app_state.clone(), event.clone(), realm_id, idempotency_key)
                .await
        }
        "customer.subscription.updated" => {
            handle_subscription_updated(app_state.clone(), event.clone(), realm_id, idempotency_key)
                .await
        }
        "customer.subscription.paused" | "customer.subscription.resumed" => {
            handle_subscription_status_change(
                app_state.clone(),
                event.clone(),
                realm_id,
                idempotency_key,
            )
            .await
        }
        "customer.subscription.deleted" => {
            handle_subscription_deleted(app_state.clone(), event.clone(), realm_id, idempotency_key)
                .await
        }
        "charge.refunded" => {
            handle_charge_refunded(app_state.clone(), event.clone(), realm_id, idempotency_key)
                .await
        }
        "charge.dispute.created" => {
            handle_charge_dispute_created(
                app_state.clone(),
                event.clone(),
                realm_id,
                idempotency_key,
            )
            .await
        }
        "charge.dispute.closed" => {
            handle_charge_dispute_closed(
                app_state.clone(),
                event.clone(),
                realm_id,
                idempotency_key,
            )
            .await
        }
        "invoice.payment_succeeded" => {
            handle_invoice_payment_succeeded(
                app_state.clone(),
                event.clone(),
                realm_id,
                idempotency_key,
            )
            .await
        }
        "invoice.payment_action_required" => {
            handle_invoice_payment_action_required(event.clone(), realm_id).await
        }
        "payment_intent.succeeded" => {
            handle_payment_intent_succeeded(
                app_state.clone(),
                event.clone(),
                realm_id,
                idempotency_key,
            )
            .await
        }
        "payment_intent.payment_failed" | "invoice.payment_failed" => {
            handle_payment_failed(app_state.clone(), event.clone(), realm_id, idempotency_key).await
        }
        "invoice.created" | "invoice.finalized" | "invoice.paid" | "invoice.voided" => {
            handle_stripe_invoice_event(app_state.clone(), event.clone(), realm_id, idempotency_key)
                .await
        }
        "credit_note.created" => {
            handle_credit_note_created(app_state.clone(), event.clone(), realm_id, idempotency_key)
                .await
        }
        "credit_note.voided" => {
            handle_credit_note_voided(app_state.clone(), event.clone(), realm_id, idempotency_key)
                .await
        }
        _ => {
            warn!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                "Unknown Stripe event type - ignoring"
            );
            Ok(create_placeholder_transaction(
                Uuid::now_v7(),
                realm_id,
                TransactionType::SubscriptionGrant,
            ))
        }
    }
}

/// Reprocess a single Stripe event that Herald missed (compensation path).
///
/// Unlike the normal webhook flow, this:
/// - Skips Redis idempotency checks entirely
/// - Skips signature verification (event comes from Stripe Events API, not webhook)
/// - Uses DB `payment_event` for idempotency only
/// - Reuses the same match routing from `process_stripe_event_once`
pub(crate) async fn reprocess_stripe_event(
    app_state: AppState,
    realm_id: &str,
    event: &Value,
    event_type: &str,
) -> Result<(), CoreError> {
    let event_id = event["id"]
        .as_str()
        .ok_or_else(|| CoreError::BadRequest("Missing event id".to_string()))?;

    let saved_event = if let Some(existing) = app_state
        .billing_repository
        .find_payment_event_by_external_id(event_id, "stripe")
        .await?
    {
        if existing.processed {
            info!(
                realm_id = %realm_id,
                event_id = %event_id,
                event_type = %event_type,
                "Stripe compensation: event already processed, skipping"
            );
            return Ok(());
        }
        // Exists but not processed: previous attempt failed — retry by reusing this row
        info!(
            realm_id = %realm_id,
            event_id = %event_id,
            event_type = %event_type,
            "Stripe compensation: retrying unprocessed event"
        );
        existing
    } else {
        let new_payment_event = PaymentEvent {
            id: Uuid::now_v7(),
            realm_id: realm_id.to_string(),
            external_event_id: event_id.to_string(),
            payment_provider: "stripe".to_string(),
            event_type: event_type.to_string(),
            subscription_id: None,
            payload: event.clone(),
            processed: false,
            processing_started_at: None,
            created_at: chrono::Utc::now(),
        };

        match app_state
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
                    "Stripe compensation: concurrent insert detected, event already handled"
                );
                return Ok(());
            }
            Err(e) => return Err(e),
        }
    };

    // Synthetic idempotency key (not used for Redis, only passed to handlers)
    let idempotency_key = format!("compensation_stripe_{}", event_id);

    let result = process_stripe_event_once(
        app_state.clone(),
        event,
        realm_id,
        &idempotency_key,
        event_id,
        event_type,
    )
    .await;

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
                    "Stripe compensation: handler succeeded but failed to mark payment_event as processed — event may be reprocessed on next run"
                );
            } else {
                tracing::info!(
                    realm_id = %realm_id,
                    event_id = %event_id,
                    event_type = %event_type,
                    "Stripe compensation: event reprocessed successfully"
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
                "Stripe compensation: failed to reprocess event"
            );
            Err(e)
        }
    }
}

/// Strategy for handling points issuance when an async payment method is used
/// (SEPA, ACH, BECS, Bacs) and `payment_status` is `unpaid` at checkout completion.
///
/// - `Conservative` (default): wait for `async_payment_succeeded` before issuing points.
/// - `Eager`: issue points immediately on `checkout.session.completed`; reclaim on failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncPointsStrategy {
    Conservative,
    Eager,
}

impl From<&str> for AsyncPointsStrategy {
    fn from(value: &str) -> Self {
        match value {
            "eager" => AsyncPointsStrategy::Eager,
            _ => AsyncPointsStrategy::Conservative,
        }
    }
}

/// Read the `async_points_strategy` config value for a given realm.
///
/// Returns `Conservative` when no config row exists or the value is not `"eager"`.
pub async fn read_async_points_strategy(state: &AppState, realm_id: &str) -> AsyncPointsStrategy {
    let result = state
        .realm_config_repository
        .get(
            realm_id.to_string(),
            "stripe".to_string(),
            "async_points_strategy".to_string(),
        )
        .await;

    match result {
        Ok(Some(config)) if config.enabled => {
            AsyncPointsStrategy::from(config.config_value.as_str())
        }
        _ => {
            if let Err(e) = &result {
                warn!(
                    realm_id = %realm_id,
                    error = %e,
                    "Failed to read async_points_strategy from DB, defaulting to Conservative"
                );
            }
            AsyncPointsStrategy::Conservative
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stripe fires `customer.subscription.created` for checkout-initiated
    /// subscriptions while the subscription is still `incomplete`, and in that
    /// state `current_period_end` is not set (Stripe populates it only after the
    /// first invoice is paid). If the parser treats it as required, the handler
    /// returns BadRequest (non-transient) BEFORE `handle_subscription_paid` runs,
    /// so `grantOnSubscribe` / `pointsPerPeriod` credits are never granted.
    /// This test guards the regression: a payload without current_period_end
    /// must parse successfully with `current_period_end == None`.
    #[test]
    fn parse_subscription_created_accepts_missing_current_period_end() {
        let event: Value = serde_json::json!({
            "id": "evt_test",
            "type": "customer.subscription.created",
            "data": {
                "object": {
                    "id": "sub_test",
                    "object": "subscription",
                    "status": "incomplete",
                    "cancel_at_period_end": false,
                    "metadata": {
                        "herald_user_id": "00000000-0000-0000-0000-000000000001",
                        "herald_entitlement_key": "ent-key",
                        "herald_client_app_id": "00000000-0000-0000-0000-000000000002"
                    },
                    "items": {
                        "data": [
                            {
                                "price": {
                                    "id": "price_test",
                                    "product": "prod_test"
                                }
                            }
                        ]
                    }
                }
            }
        });

        let payload = parse_subscription_created_payload(&event).expect(
            "checkout-initiated subscription.created must parse without current_period_end",
        );
        assert_eq!(payload.stripe_subscription_id, "sub_test");
        assert!(payload.current_period_end.is_none());
        assert_eq!(payload.status, SubscriptionStatus::Incomplete);
    }

    /// Sanity check: when Stripe DOES include current_period_end, it must still
    /// be parsed (so renewal/cancel flows keep their real period end).
    #[test]
    fn parse_subscription_created_uses_current_period_end_when_present() {
        let event: Value = serde_json::json!({
            "id": "evt_test",
            "type": "customer.subscription.created",
            "data": {
                "object": {
                    "id": "sub_test",
                    "object": "subscription",
                    "status": "active",
                    "cancel_at_period_end": false,
                    "current_period_end": 1_800_000_000_i64,
                    "current_period_start": 1_700_000_000_i64,
                    "metadata": {
                        "herald_user_id": "00000000-0000-0000-0000-000000000001",
                        "herald_entitlement_key": "ent-key"
                    },
                    "items": {
                        "data": [
                            {
                                "price": {
                                    "id": "price_test",
                                    "product": "prod_test"
                                }
                            }
                        ]
                    }
                }
            }
        });

        let payload = parse_subscription_created_payload(&event).expect("payload must parse");
        let end = payload
            .current_period_end
            .expect("current_period_end should be present");
        assert_eq!(end.timestamp(), 1_800_000_000);
    }

    // The period normalizer is the P0 prerequisite for subscription chained
    // pre-grant: when it cannot uniquely resolve the points entitlement's
    // billing period, the webhook handler must skip the grant and emit a
    // structured warning rather than guess from event time.
    // These four quadrants pin the contract:
    //   Q1 top-level (pre-basil API)         → Some
    //   Q2 item-level single item (basil+)   → Some
    //   Q3 multi-item disagreeing periods    → None  (cannot uniquely map)
    //   Q4 both top-level and item-level absent → None
    // Plus: multi-item unanimous period → Some (entitlement item is ambiguous
    // but the period is not, so the period itself is uniquely resolvable).

    fn ts(value: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(value, 0).unwrap()
    }

    #[test]
    fn normalize_stripe_period_top_level_fields_resolved() {
        // Pre-basil API: period lives at the subscription top level.
        let sub = serde_json::json!({
            "id": "sub_1",
            "current_period_start": 1_700_000_000,
            "current_period_end": 1_700_000_000 + 2_592_000,
        });
        let got = normalize_stripe_period(&sub).expect("top-level period must resolve");
        assert_eq!(got.0, ts(1_700_000_000));
        assert_eq!(got.1, ts(1_700_000_000 + 2_592_000));
    }

    #[test]
    fn normalize_stripe_period_item_level_single_item_resolved() {
        // 2025-03-31.basil: period lives on the subscription item. Top-level
        // fields are absent.
        let sub = serde_json::json!({
            "id": "sub_1",
            "items": {
                "data": [
                    {
                        "id": "si_1",
                        "current_period_start": 1_700_000_000,
                        "current_period_end": 1_700_000_000 + 2_592_000
                    }
                ]
            }
        });
        let got = normalize_stripe_period(&sub).expect("single-item period must resolve");
        assert_eq!(got.0, ts(1_700_000_000));
        assert_eq!(got.1, ts(1_700_000_000 + 2_592_000));
    }

    #[test]
    fn normalize_stripe_period_multi_item_disagreeing_periods_is_none() {
        // Two items with DIFFERENT billing periods. Without the entitlement
        // mapping in hand the normalizer cannot uniquely identify which item
        // owns the points entitlement → P0: return None, do not guess.
        let sub = serde_json::json!({
            "id": "sub_1",
            "items": {
                "data": [
                    {
                        "id": "si_1",
                        "current_period_start": 1_700_000_000,
                        "current_period_end": 1_700_000_000 + 2_592_000
                    },
                    {
                        "id": "si_2",
                        "current_period_start": 1_700_000_000,
                        "current_period_end": 1_700_000_000 + 777_6000
                    }
                ]
            }
        });
        assert!(
            normalize_stripe_period(&sub).is_none(),
            "disagreeing multi-item periods must NOT be resolved (P0)"
        );
    }

    #[test]
    fn normalize_stripe_period_both_levels_absent_is_none() {
        // No period fields anywhere. Must return None — the caller skips the
        // grant and awaits compensation (P0: never guess from event time).
        let sub = serde_json::json!({
            "id": "sub_1",
            "items": { "data": [ { "id": "si_1" } ] }
        });
        assert!(
            normalize_stripe_period(&sub).is_none(),
            "absent period fields must NOT be resolved (P0)"
        );
    }

    #[test]
    fn normalize_stripe_period_multi_item_unanimous_resolved() {
        // Multiple items that AGREE on the period: the period is uniquely
        // resolvable even though the entitlement item is ambiguous.
        let sub = serde_json::json!({
            "id": "sub_1",
            "items": {
                "data": [
                    {
                        "id": "si_1",
                        "current_period_start": 1_700_000_000,
                        "current_period_end": 1_700_000_000 + 2_592_000
                    },
                    {
                        "id": "si_2",
                        "current_period_start": 1_700_000_000,
                        "current_period_end": 1_700_000_000 + 2_592_000
                    }
                ]
            }
        });
        let got = normalize_stripe_period(&sub).expect("unanimous multi-item period must resolve");
        assert_eq!(got.0, ts(1_700_000_000));
        assert_eq!(got.1, ts(1_700_000_000 + 2_592_000));
    }

    #[test]
    fn normalize_stripe_period_top_level_takes_precedence_when_items_have_no_period() {
        // Items legitimately without period fields (e.g. one-time add-ons)
        // must not block top-level resolution on older API versions.
        let sub = serde_json::json!({
            "id": "sub_1",
            "current_period_start": 1_700_000_000,
            "current_period_end": 1_700_000_000 + 2_592_000,
            "items": { "data": [ { "id": "si_1" } ] }
        });
        let got = normalize_stripe_period(&sub).expect("top-level must win when items lack period");
        assert_eq!(got.0, ts(1_700_000_000));
    }

    #[test]
    fn normalize_stripe_period_inverted_window_is_none() {
        // start >= end is not a valid billing window.
        let sub = serde_json::json!({
            "id": "sub_1",
            "current_period_start": 1_700_000_000 + 2_592_000,
            "current_period_end": 1_700_000_000,
        });
        assert!(normalize_stripe_period(&sub).is_none());
    }

    // The invoice resolver is what unblocks Stripe `invoice.payment_succeeded`
    // renewal grants. A Stripe Invoice has NO top-level `current_period_*`
    // (those are Subscription fields) and uses `lines.data` (NOT `items.data`);
    // each subscription line's `period.{start,end}` IS the subscription
    // billing period being paid. These quadrants pin the invoice counterpart
    // of the P0 contract, mirroring the `normalize_stripe_period_*` tests:
    //   (1) single-line period resolved                → Some
    //   (2) multi-line unanimous period resolved       → Some
    //   (3) multi-line disagreeing periods             → None (cannot uniquely map)
    //   (4) line with no period (add-on) skipped       → Some via the other line
    //   (5) all lines lacking period                   → None
    //   (6) inverted window on a carrying line         → None

    #[test]
    fn normalize_stripe_invoice_period_single_line_resolved() {
        // A renewal invoice with a single subscription line: that line's
        // `period` IS the subscription period being paid.
        let invoice = serde_json::json!({
            "id": "in_1",
            "object": "invoice",
            "subscription": "sub_1",
            "lines": {
                "data": [
                    {
                        "id": "il_1",
                        "period": {
                            "start": 1_700_000_000,
                            "end": 1_700_000_000 + 2_592_000
                        }
                    }
                ]
            }
        });
        let got =
            normalize_stripe_invoice_period(&invoice).expect("single-line period must resolve");
        assert_eq!(got.0, ts(1_700_000_000));
        assert_eq!(got.1, ts(1_700_000_000 + 2_592_000));
    }

    #[test]
    fn normalize_stripe_invoice_period_multi_line_unanimous_resolved() {
        // Two subscription lines that AGREE on the period (e.g. base plan +
        // metered add-on billed in the same window): the period is uniquely
        // resolvable even though the entitlement line is ambiguous.
        let invoice = serde_json::json!({
            "id": "in_1",
            "object": "invoice",
            "subscription": "sub_1",
            "lines": {
                "data": [
                    {
                        "id": "il_1",
                        "period": {
                            "start": 1_700_000_000,
                            "end": 1_700_000_000 + 2_592_000
                        }
                    },
                    {
                        "id": "il_2",
                        "period": {
                            "start": 1_700_000_000,
                            "end": 1_700_000_000 + 2_592_000
                        }
                    }
                ]
            }
        });
        let got = normalize_stripe_invoice_period(&invoice)
            .expect("unanimous multi-line period must resolve");
        assert_eq!(got.0, ts(1_700_000_000));
        assert_eq!(got.1, ts(1_700_000_000 + 2_592_000));
    }

    #[test]
    fn normalize_stripe_invoice_period_multi_line_disagreeing_is_none() {
        // Two subscription lines with DIFFERENT periods (e.g. a proration line
        // whose window does not match the base subscription window). Without
        // the entitlement mapping in hand we cannot uniquely identify which
        // line owns the points entitlement → P0: return None, do not guess.
        let invoice = serde_json::json!({
            "id": "in_1",
            "object": "invoice",
            "subscription": "sub_1",
            "lines": {
                "data": [
                    {
                        "id": "il_1",
                        "period": {
                            "start": 1_700_000_000,
                            "end": 1_700_000_000 + 2_592_000
                        }
                    },
                    {
                        "id": "il_2",
                        "period": {
                            "start": 1_700_000_000,
                            "end": 1_700_000_000 + 777_6000
                        }
                    }
                ]
            }
        });
        assert!(
            normalize_stripe_invoice_period(&invoice).is_none(),
            "disagreeing multi-line periods must NOT be resolved (P0)"
        );
    }

    #[test]
    fn normalize_stripe_invoice_period_skips_line_without_period() {
        // A line legitimately without a `period` (e.g. a one-time add-on line
        // Stripe may emit without a period) must be skipped, and resolution
        // must still succeed via the subscription line that DOES carry a
        // period.
        let invoice = serde_json::json!({
            "id": "in_1",
            "object": "invoice",
            "subscription": "sub_1",
            "lines": {
                "data": [
                    {
                        "id": "il_1",
                        "period": {
                            "start": 1_700_000_000,
                            "end": 1_700_000_000 + 2_592_000
                        }
                    },
                    {
                        "id": "il_2"
                        // no `period` — one-time add-on style line
                    }
                ]
            }
        });
        let got = normalize_stripe_invoice_period(&invoice)
            .expect("period must resolve via the line that carries one");
        assert_eq!(got.0, ts(1_700_000_000));
        assert_eq!(got.1, ts(1_700_000_000 + 2_592_000));
    }

    #[test]
    fn normalize_stripe_invoice_period_no_line_with_period_is_none() {
        // No line carries a `period` field. Must return None — the caller skips
        // the renewal grant and awaits compensation (P0: never guess).
        let invoice = serde_json::json!({
            "id": "in_1",
            "object": "invoice",
            "subscription": "sub_1",
            "lines": { "data": [ { "id": "il_1" } ] }
        });
        assert!(
            normalize_stripe_invoice_period(&invoice).is_none(),
            "absent period on all lines must NOT be resolved (P0)"
        );
    }

    #[test]
    fn normalize_stripe_invoice_period_inverted_window_is_none() {
        // A carrying line with `start >= end` is a malformed period — reject
        // as unresolvable (P0). Mirrors the subscription-side inverted
        // window guard.
        let invoice = serde_json::json!({
            "id": "in_1",
            "object": "invoice",
            "subscription": "sub_1",
            "lines": {
                "data": [
                    {
                        "id": "il_1",
                        "period": {
                            "start": 1_700_000_000 + 2_592_000,
                            "end": 1_700_000_000
                        }
                    }
                ]
            }
        });
        assert!(normalize_stripe_invoice_period(&invoice).is_none());
    }

    #[test]
    fn normalize_stripe_invoice_period_skips_malformed_sibling_line() {
        // A renewal invoice carrying (a) a valid subscription line and (b) a
        // sibling proration/credit line whose `period.start` is null. The
        // malformed sibling must NOT doom the whole resolution — the valid
        // subscription line still resolves the period. (Earlier behavior
        // short-circuited the whole function to None on the null start,
        // skipping a renewal grant the user paid for.)
        let invoice = serde_json::json!({
            "id": "in_1",
            "object": "invoice",
            "subscription": "sub_1",
            "lines": {
                "data": [
                    {
                        "id": "il_sub",
                        "period": {
                            "start": 1_700_000_000,
                            "end": 1_700_000_000 + 2_592_000
                        }
                    },
                    {
                        "id": "il_proration",
                        "period": {
                            "start": null,
                            "end": 1_700_000_000 + 2_592_000
                        }
                    }
                ]
            }
        });
        let got = normalize_stripe_invoice_period(&invoice)
            .expect("valid subscription line must resolve despite malformed sibling");
        assert_eq!(got.0, ts(1_700_000_000));
        assert_eq!(got.1, ts(1_700_000_000 + 2_592_000));
    }

    #[test]
    fn normalize_stripe_invoice_period_all_lines_malformed_is_none() {
        // Every carrying line is malformed (null end / inverted window) — no
        // line resolves, so the whole function returns None (P0 still
        // holds when nothing resolves).
        let invoice = serde_json::json!({
            "id": "in_1",
            "object": "invoice",
            "subscription": "sub_1",
            "lines": {
                "data": [
                    {
                        "id": "il_1",
                        "period": { "start": 1_700_000_000, "end": null }
                    },
                    {
                        "id": "il_2",
                        "period": {
                            "start": 1_700_000_000 + 2_592_000,
                            "end": 1_700_000_000
                        }
                    }
                ]
            }
        });
        assert!(
            normalize_stripe_invoice_period(&invoice).is_none(),
            "all-malformed lines must yield None (P0)"
        );
    }
}
