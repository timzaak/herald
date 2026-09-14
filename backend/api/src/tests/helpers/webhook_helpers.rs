// =============================================================================
// Webhook Test Helpers
// =============================================================================
//
// Shared helpers for webhook testing.
// Provides functions for building webhook events and sending them to test servers.
//
// Updated for product_reduce: herald_* metadata builders replace plan_id builders.
// Old plan_id-based builders have been removed.
//
// =============================================================================

#![allow(dead_code)]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hex;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use tower::ServiceExt;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// ============================================================================
/// Creem Webhook Event Builders (herald_* metadata)
/// ============================================================================
///
/// Build a Creem checkout.completed webhook event with all herald_* metadata keys.
pub fn build_creem_checkout_completed_with_herald_metadata(
    event_id: &str,
    entitlement_key: &str,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Uuid,
) -> serde_json::Value {
    json!({
        "id": event_id,
        "eventType": "checkout.completed",
        "object": {
            "id": format!("checkout_{}", event_id),
            "status": "completed",
            "product": {
                "id": format!("prod_creem_{}", entitlement_key),
            },
            "metadata": {
                "herald_entitlement_key": entitlement_key,
                "herald_realm_id": realm_id,
                "herald_user_id": user_id.to_string(),
                "herald_client_app_id": client_app_id.to_string(),
                "clientAppId": client_app_id.to_string(),
            }
        }
    })
}

/// Build a Creem checkout.completed webhook without herald_entitlement_key
/// (for fallback chain testing).
pub fn build_creem_checkout_completed_without_entitlement_key(
    event_id: &str,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Uuid,
) -> serde_json::Value {
    json!({
        "id": event_id,
        "eventType": "checkout.completed",
        "object": {
            "id": format!("checkout_{}", event_id),
            "status": "completed",
            "product": {
                "id": format!("prod_creem_fallback_{}", event_id),
            },
            "metadata": {
                "herald_realm_id": realm_id,
                "herald_user_id": user_id.to_string(),
                "herald_client_app_id": client_app_id.to_string(),
                "clientAppId": client_app_id.to_string(),
            }
        }
    })
}

/// Build a Creem subscription.paid webhook event with herald_* metadata.
#[allow(clippy::too_many_arguments)]
pub fn build_creem_subscription_paid_with_herald_metadata(
    event_id: &str,
    entitlement_key: &str,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Option<Uuid>,
    external_subscription_id: &str,
    external_product_id: &str,
    is_renewal: bool,
) -> serde_json::Value {
    // P0: `normalize_creem_period` requires BOTH `currentPeriodStart` and
    // `currentPeriodEnd` (start < end) to resolve the billing period; otherwise
    // the grant is skipped with `reason="period_uniquely_unresolvable"`. Real
    // Creem subscription.paid payloads always carry both bounds, so emit a
    // realistic 30-day window (mirrors creem_webhook_patch_scenarios.rs and the
    // legacy build_subscription_paid_event_full helper).
    let period_start = chrono::Utc::now();
    let period_end = period_start + chrono::Duration::days(30);
    let mut metadata = json!({
        "herald_entitlement_key": entitlement_key,
        "herald_realm_id": realm_id,
        "herald_user_id": user_id.to_string(),
    });
    if let Some(caid) = client_app_id {
        metadata["herald_client_app_id"] = json!(caid.to_string());
        metadata["clientAppId"] = json!(caid.to_string());
    }

    json!({
        "id": event_id,
        "eventType": "subscription.paid",
        "data": {
            "object": {
                "subscriptionId": external_subscription_id,
                "productId": external_product_id,
                "userId": user_id.to_string(),
                "isRenewal": is_renewal,
                "currentPeriodStart": period_start.to_rfc3339(),
                "currentPeriodEnd": period_end.to_rfc3339(),
                "status": "active",
                "cancelAtPeriodEnd": false,
                "metadata": metadata,
            }
        },
        "herald_entitlement_key": entitlement_key,
    })
}

/// Build a Creem `subscription.paid` **renewal** webhook event carrying the
/// renewal-invoice attribution fields (`last_transaction_id` / `amount` /
/// `currency`). This mirrors
/// `build_creem_subscription_paid_with_herald_metadata` but emits a
/// renewal-period payload (`isRenewal=true`) and the three new fields so the
/// renewal branch in `handle_subscription_paid` can record a Succeeded attempt
/// and upsert a Paid invoice.
///
/// `last_transaction_id` is `Option<&str>`: pass `None` to exercise the
/// `current_period_start` fallback or the all-missing warn
/// branch. `amount` is in **smallest currency units** (e.g. cents); `0`
/// exercises the zero-yuan cycle skip.
#[allow(clippy::too_many_arguments)]
pub fn build_creem_subscription_paid_renewal_with_invoice(
    event_id: &str,
    entitlement_key: &str,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Option<Uuid>,
    external_subscription_id: &str,
    external_product_id: &str,
    last_transaction_id: Option<&str>,
    amount: Option<i64>,
    currency: Option<&str>,
) -> serde_json::Value {
    // P0: `normalize_creem_period` requires BOTH `currentPeriodStart` and
    // `currentPeriodEnd` (start < end) to resolve the billing period; without
    // both bounds the points grant is skipped. Real Creem renewal payloads
    // always carry both bounds, so emit a realistic 30-day window.
    let period_start = chrono::Utc::now();
    let period_end = period_start + chrono::Duration::days(30);
    let mut metadata = json!({
        "herald_entitlement_key": entitlement_key,
        "herald_realm_id": realm_id,
        "herald_user_id": user_id.to_string(),
    });
    if let Some(caid) = client_app_id {
        metadata["herald_client_app_id"] = json!(caid.to_string());
        metadata["clientAppId"] = json!(caid.to_string());
    }

    let mut object = json!({
        "subscriptionId": external_subscription_id,
        "productId": external_product_id,
        "userId": user_id.to_string(),
        "isRenewal": true,
        "currentPeriodStart": period_start.to_rfc3339(),
        "currentPeriodEnd": period_end.to_rfc3339(),
        "status": "active",
        "cancelAtPeriodEnd": false,
        "metadata": metadata,
    });
    if let Some(tx) = last_transaction_id {
        object["last_transaction_id"] = json!(tx);
    }
    if let Some(amt) = amount {
        object["amount"] = json!(amt);
    }
    if let Some(cur) = currency {
        object["currency"] = json!(cur);
    }

    json!({
        "id": event_id,
        "eventType": "subscription.paid",
        "data": {
            "object": object,
        },
        "herald_entitlement_key": entitlement_key,
    })
}

/// Build a Creem subscription.paid webhook without herald_entitlement_key
/// (for fallback chain testing).
#[allow(clippy::too_many_arguments)]
pub fn build_creem_subscription_paid_without_entitlement_key(
    event_id: &str,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Option<Uuid>,
    external_subscription_id: &str,
    external_product_id: &str,
    is_renewal: bool,
) -> serde_json::Value {
    // P0: emit BOTH period bounds (see build_creem_subscription_paid_with_herald_metadata).
    let period_start = chrono::Utc::now();
    let period_end = period_start + chrono::Duration::days(30);
    let mut metadata = json!({
        "herald_realm_id": realm_id,
        "herald_user_id": user_id.to_string(),
    });
    if let Some(caid) = client_app_id {
        metadata["herald_client_app_id"] = json!(caid.to_string());
        metadata["clientAppId"] = json!(caid.to_string());
    }

    json!({
        "id": event_id,
        "eventType": "subscription.paid",
        "data": {
            "object": {
                "subscriptionId": external_subscription_id,
                "productId": external_product_id,
                "userId": user_id.to_string(),
                "isRenewal": is_renewal,
                "currentPeriodStart": period_start.to_rfc3339(),
                "currentPeriodEnd": period_end.to_rfc3339(),
                "status": "active",
                "cancelAtPeriodEnd": false,
                "metadata": metadata,
            }
        },
    })
}

/// Build a Creem subscription.canceled webhook with entitlement_key.
#[allow(clippy::too_many_arguments)]
pub fn build_creem_subscription_canceled_with_entitlement(
    event_id: &str,
    entitlement_key: &str,
    realm_id: &str,
    user_id: Uuid,
    external_subscription_id: &str,
    external_product_id: &str,
    cancel_at_period_end: bool,
) -> serde_json::Value {
    let period_end = chrono::Utc::now() + chrono::Duration::days(30);
    json!({
        "id": event_id,
        "eventType": "subscription.canceled",
        "data": {
            "object": {
                "subscriptionId": external_subscription_id,
                "productId": external_product_id,
                "userId": user_id.to_string(),
                "cancelAtPeriodEnd": cancel_at_period_end,
                "currentPeriodEnd": period_end.to_rfc3339(),
                "status": if cancel_at_period_end { "active" } else { "canceled" },
            }
        },
        "herald_entitlement_key": entitlement_key,
        "metadata": {
            "herald_entitlement_key": entitlement_key,
            "herald_realm_id": realm_id,
            "herald_user_id": user_id.to_string(),
        },
    })
}

/// Build a Creem checkout.completed webhook missing herald_realm_id.
pub fn build_creem_checkout_missing_realm_id(
    event_id: &str,
    entitlement_key: &str,
    user_id: Uuid,
    client_app_id: Uuid,
) -> serde_json::Value {
    json!({
        "id": event_id,
        "eventType": "checkout.completed",
        "object": {
            "id": format!("checkout_{}", event_id),
            "status": "completed",
            "product": {
                "id": format!("prod_creem_{}", entitlement_key),
            },
            "metadata": {
                "herald_entitlement_key": entitlement_key,
                "herald_user_id": user_id.to_string(),
                "herald_client_app_id": client_app_id.to_string(),
                "clientAppId": client_app_id.to_string(),
            }
        }
    })
}

/// Build a Creem checkout.completed webhook with empty herald_entitlement_key.
pub fn build_creem_checkout_empty_entitlement_key(
    event_id: &str,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Uuid,
) -> serde_json::Value {
    json!({
        "id": event_id,
        "eventType": "checkout.completed",
        "object": {
            "id": format!("checkout_{}", event_id),
            "status": "completed",
            "product": {
                "id": format!("prod_creem_empty_{}", event_id),
            },
            "metadata": {
                "herald_entitlement_key": "",
                "herald_realm_id": realm_id,
                "herald_user_id": user_id.to_string(),
                "herald_client_app_id": client_app_id.to_string(),
                "clientAppId": client_app_id.to_string(),
            }
        }
    })
}

/// Build a Creem checkout.completed webhook with invalid entitlement_key format.
pub fn build_creem_checkout_invalid_entitlement_key(
    event_id: &str,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Uuid,
    invalid_key: &str,
) -> serde_json::Value {
    json!({
        "id": event_id,
        "eventType": "checkout.completed",
        "object": {
            "id": format!("checkout_{}", event_id),
            "status": "completed",
            "product": {
                "id": format!("prod_creem_invalid_{}", event_id),
            },
            "metadata": {
                "herald_entitlement_key": invalid_key,
                "herald_realm_id": realm_id,
                "herald_user_id": user_id.to_string(),
                "herald_client_app_id": client_app_id.to_string(),
                "clientAppId": client_app_id.to_string(),
            }
        }
    })
}

/// ============================================================================
/// Stripe Webhook Event Builders (herald_* metadata)
/// ============================================================================
///
/// Build a Stripe checkout.session.completed webhook with herald_* metadata.
#[allow(clippy::too_many_arguments)]
pub fn build_stripe_checkout_completed_with_herald_metadata(
    event_id: &str,
    entitlement_key: &str,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Uuid,
) -> serde_json::Value {
    json!({
        "id": event_id,
        "object": "event",
        "type": "checkout.session.completed",
        "api_version": "2020-08-27",
        "created": chrono::Utc::now().timestamp(),
        "data": {
            "object": {
                "id": format!("cs_test_{}", Uuid::now_v7()),
                "object": "checkout.session",
                "status": "complete",
                "payment_status": "paid",
                "customer": format!("cus_test_{}", Uuid::now_v7()),
                "subscription": format!("sub_test_{}", event_id),
                "payment_intent": format!("pi_test_{}", Uuid::now_v7()),
                "metadata": {
                    "herald_entitlement_key": entitlement_key,
                    "herald_realm_id": realm_id,
                    "herald_user_id": user_id.to_string(),
                    "herald_client_app_id": client_app_id.to_string(),
                    "clientAppId": client_app_id.to_string(),
                    "userId": user_id.to_string(),
                },
                "display_items": [{
                    "price": {
                        "product": format!("prod_stripe_{}", entitlement_key)
                    }
                }]
            }
        }
    })
}

/// Build a Stripe checkout.session.completed webhook without herald_entitlement_key
/// (for fallback chain testing).
pub fn build_stripe_checkout_completed_without_entitlement_key(
    event_id: &str,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Uuid,
) -> serde_json::Value {
    json!({
        "id": event_id,
        "object": "event",
        "type": "checkout.session.completed",
        "api_version": "2020-08-27",
        "created": chrono::Utc::now().timestamp(),
        "data": {
            "object": {
                "id": format!("cs_test_{}", Uuid::now_v7()),
                "object": "checkout.session",
                "status": "complete",
                "payment_status": "paid",
                "customer": format!("cus_test_{}", Uuid::now_v7()),
                "subscription": format!("sub_test_{}", event_id),
                "payment_intent": format!("pi_test_{}", Uuid::now_v7()),
                "metadata": {
                    "herald_realm_id": realm_id,
                    "herald_user_id": user_id.to_string(),
                    "herald_client_app_id": client_app_id.to_string(),
                    "clientAppId": client_app_id.to_string(),
                    "userId": user_id.to_string(),
                },
                "display_items": [{
                    "price": {
                        "product": format!("prod_stripe_fallback_{}", event_id)
                    }
                }]
            }
        }
    })
}

/// Build a Stripe customer.subscription.updated webhook with herald_entitlement_key.
#[allow(clippy::too_many_arguments)]
pub fn build_stripe_subscription_updated_with_entitlement(
    event_id: &str,
    stripe_subscription_id: &str,
    realm_id: &str,
    user_id: Uuid,
    previous_entitlement_key: &str,
    current_entitlement_key: &str,
    external_product_id: &str,
) -> serde_json::Value {
    let period_end = (chrono::Utc::now() + chrono::Duration::days(30)).timestamp();
    json!({
        "id": event_id,
        "object": "event",
        "type": "customer.subscription.updated",
        "api_version": "2020-08-27",
        "created": chrono::Utc::now().timestamp(),
        "data": {
            "object": {
                "id": stripe_subscription_id,
                "object": "subscription",
                "status": "active",
                "current_period_end": period_end,
                "items": {
                    "data": [{
                        "price": {
                            "product": external_product_id,
                            "metadata": {
                                "herald_entitlement_key": current_entitlement_key,
                            }
                        }
                    }]
                },
                "metadata": {
                    "herald_realm_id": realm_id,
                    "herald_user_id": user_id.to_string(),
                    "userId": user_id.to_string(),
                }
            },
            "previous_attributes": {
                "items": {
                    "data": [{
                        "price": {
                            "product": external_product_id,
                            "metadata": {
                                "herald_entitlement_key": previous_entitlement_key,
                            }
                        }
                    }]
                }
            }
        }
    })
}

/// Build a Stripe customer.subscription.deleted webhook with entitlement_key.
pub fn build_stripe_subscription_deleted_with_entitlement(
    event_id: &str,
    stripe_subscription_id: &str,
    realm_id: &str,
    user_id: Uuid,
    entitlement_key: &str,
) -> serde_json::Value {
    let period_end = (chrono::Utc::now() + chrono::Duration::days(30)).timestamp();
    json!({
        "id": event_id,
        "object": "event",
        "type": "customer.subscription.deleted",
        "api_version": "2020-08-27",
        "created": chrono::Utc::now().timestamp(),
        "data": {
            "object": {
                "id": stripe_subscription_id,
                "object": "subscription",
                "status": "canceled",
                "cancel_at_period_end": false,
                "current_period_end": period_end,
                "metadata": {
                    "herald_realm_id": realm_id,
                    "herald_user_id": user_id.to_string(),
                    "herald_entitlement_key": entitlement_key,
                    "userId": user_id.to_string(),
                }
            }
        }
    })
}

/// Build a Stripe invoice.payment_succeeded webhook with herald_* metadata.
#[allow(clippy::too_many_arguments)]
pub fn build_stripe_invoice_with_herald_metadata(
    event_id: &str,
    stripe_subscription_id: &str,
    realm_id: &str,
    user_id: Uuid,
    entitlement_key: &str,
    amount_paid: i64,
) -> serde_json::Value {
    let period_start = chrono::Utc::now().timestamp();
    let period_end = (chrono::Utc::now() + chrono::Duration::days(30)).timestamp();
    json!({
        "id": event_id,
        "object": "event",
        "type": "invoice.payment_succeeded",
        "api_version": "2020-08-27",
        "created": chrono::Utc::now().timestamp(),
        "data": {
            "object": {
                "id": format!("in_test_{}", Uuid::now_v7()),
                "object": "invoice",
                "status": "paid",
                "subscription": stripe_subscription_id,
                "amount_paid": amount_paid,
                "currency": "usd",
                "current_period_start": period_start,
                "current_period_end": period_end,
                // P0: `normalize_stripe_invoice_period` resolves the billing
                // period from `lines.data[].period.{start,end}` (NOT the invoice
                // top-level current_period_* fields, which Stripe invoices do not
                // carry). Without a resolvable line period the renewal grant is
                // skipped with `reason="period_uniquely_unresolvable"`. Real
                // Stripe subscription renewal invoices populate each line's
                // `period` with the subscription billing period being paid.
                "lines": {
                    "data": [{
                        "id": format!("il_test_{}", Uuid::now_v7()),
                        "object": "line_item",
                        "type": "subscription",
                        "subscription": stripe_subscription_id,
                        "period": {
                            "start": period_start,
                            "end": period_end,
                        }
                    }]
                },
                "metadata": {
                    "herald_realm_id": realm_id,
                    "herald_user_id": user_id.to_string(),
                    "herald_entitlement_key": entitlement_key,
                    "userId": user_id.to_string(),
                }
            }
        }
    })
}

/// Build a Stripe `invoice.payment_succeeded` event payload shaped for a
/// **subscription renewal**.
///
/// Unlike `build_stripe_invoice_with_herald_metadata` (which only carries
/// `amount_paid` and generates the `in_` id internally), this builder exposes
/// the fields the renewal branch and the invoice re-upsert actually read:
///
/// - `total` (smallest currency unit) — drives the `amount > 0` gate and the
///   renewal `payment_attempt.amount`;
/// - `hosted_invoice_url` / `invoice_pdf` — captured into
///   `invoice.external_hosted_url` / `invoice.external_pdf_url` and asserted
///   unchanged across the renewal re-upsert;
/// - a controllable `stripe_invoice_id` (`in_`) — used for idempotency and
///   coexistence assertions;
/// - `payment_intent` — flows into `invoice.external_order_id`;
/// - `lines.data[].period.{start,end}` — the renewal grant depends on a
///   resolvable line period (see `normalize_stripe_invoice_period`).
///
/// Set `total = 0` to exercise the zero-yuan skip path.
pub fn build_stripe_invoice_payment_succeeded_renewal(
    event_id: &str,
    stripe_subscription_id: &str,
    stripe_invoice_id: &str,
    realm_id: &str,
    user_id: Uuid,
    entitlement_key: &str,
    total: i64,
    hosted_invoice_url: Option<&str>,
    invoice_pdf: Option<&str>,
) -> serde_json::Value {
    let period_start = chrono::Utc::now().timestamp();
    let period_end = (chrono::Utc::now() + chrono::Duration::days(30)).timestamp();
    let mut object = json!({
        "id": stripe_invoice_id,
        "object": "invoice",
        "status": "paid",
        "subscription": stripe_subscription_id,
        "total": total,
        "currency": "usd",
        "payment_intent": format!("pi_renewal_{}", Uuid::now_v7()),
        // See `build_stripe_invoice_with_herald_metadata`: the renewal grant
        // resolves the billing period from each line's `period`, NOT the
        // invoice top-level current_period_* fields (which Stripe invoices do
        // not carry).
        "lines": {
            "data": [{
                "id": format!("il_renewal_{}", Uuid::now_v7()),
                "object": "line_item",
                "type": "subscription",
                "subscription": stripe_subscription_id,
                "period": {
                    "start": period_start,
                    "end": period_end,
                }
            }]
        },
        "metadata": {
            "herald_realm_id": realm_id,
            "herald_user_id": user_id.to_string(),
            "herald_entitlement_key": entitlement_key,
            "userId": user_id.to_string(),
        }
    });

    if let Some(url) = hosted_invoice_url {
        object["hosted_invoice_url"] = json!(url);
    }
    if let Some(url) = invoice_pdf {
        object["invoice_pdf"] = json!(url);
    }

    json!({
        "id": event_id,
        "object": "event",
        "type": "invoice.payment_succeeded",
        "api_version": "2020-08-27",
        "created": chrono::Utc::now().timestamp(),
        "data": {
            "object": object
        }
    })
}

/// Build a Stripe checkout.session.completed webhook missing herald_realm_id.
pub fn build_stripe_checkout_missing_realm_id(
    event_id: &str,
    entitlement_key: &str,
    user_id: Uuid,
    client_app_id: Uuid,
) -> serde_json::Value {
    json!({
        "id": event_id,
        "object": "event",
        "type": "checkout.session.completed",
        "api_version": "2020-08-27",
        "created": chrono::Utc::now().timestamp(),
        "data": {
            "object": {
                "id": format!("cs_test_{}", Uuid::now_v7()),
                "object": "checkout.session",
                "status": "complete",
                "payment_status": "paid",
                "customer": format!("cus_test_{}", Uuid::now_v7()),
                "subscription": format!("sub_test_{}", event_id),
                "payment_intent": format!("pi_test_{}", Uuid::now_v7()),
                "metadata": {
                    "herald_entitlement_key": entitlement_key,
                    "herald_user_id": user_id.to_string(),
                    "herald_client_app_id": client_app_id.to_string(),
                    "clientAppId": client_app_id.to_string(),
                    "userId": user_id.to_string(),
                },
                "display_items": [{
                    "price": {
                        "product": format!("prod_stripe_{}", entitlement_key)
                    }
                }]
            }
        }
    })
}

/// ============================================================================
/// Webhook Sending Helpers
/// ============================================================================
/// Send a webhook event with signature
pub async fn send_webhook_with_signature(
    app: &axum::Router,
    realm_id: &str,
    payload: serde_json::Value,
    webhook_secret: &str,
) -> axum::response::Response {
    let payload_str = serde_json::to_string(&payload).unwrap();

    // Generate signature
    let mut mac = HmacSha256::new_from_slice(webhook_secret.as_bytes()).unwrap();
    mac.update(payload_str.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/third/pay/{}/creem/webhooks", realm_id))
                .header("creem-signature", signature)
                .header("Content-Type", "application/json")
                .body(Body::from(payload_str))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Send a webhook event with invalid signature
pub async fn send_webhook_with_invalid_signature(
    app: &axum::Router,
    realm_id: &str,
    payload: serde_json::Value,
) -> axum::response::Response {
    let payload_str = serde_json::to_string(&payload).unwrap();
    let invalid_signature = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabb";

    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/third/pay/{}/creem/webhooks", realm_id))
                .header("creem-signature", invalid_signature)
                .header("Content-Type", "application/json")
                .body(Body::from(payload_str))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Send a webhook event without signature
pub async fn send_webhook_without_signature(
    app: &axum::Router,
    realm_id: &str,
    payload: serde_json::Value,
) -> axum::response::Response {
    let payload_str = serde_json::to_string(&payload).unwrap();

    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/third/pay/{}/creem/webhooks", realm_id))
                .header("Content-Type", "application/json")
                .body(Body::from(payload_str))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// ============================================================================
/// Webhook Assertion Helpers
/// ============================================================================
/// Assert webhook response is successful (200 OK or 202 Accepted)
pub fn assert_webhook_success(response: &axum::response::Response) {
    let status = response.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::ACCEPTED,
        "Expected webhook success (200 or 202), got {}",
        status
    );
}

/// Assert webhook response is an error
pub fn assert_webhook_error(response: &axum::response::Response, expected_status: StatusCode) {
    assert_eq!(
        response.status(),
        expected_status,
        "Expected webhook error {}, got {}",
        expected_status,
        response.status()
    );
}

/// ============================================================================
/// Idempotency Helpers
/// ============================================================================
/// Generate idempotency key for Creem webhook
pub fn generate_webhook_idempotency_key(event_id: &str) -> String {
    format!("creem_{}", event_id)
}

/// ============================================================================
/// Test Data Generators
/// ============================================================================
/// Generate a unique event ID for testing
pub fn generate_test_event_id() -> String {
    format!("evt_test_{}", Uuid::now_v7())
}

/// Generate a unique user ID for testing
pub fn generate_test_user_id() -> Uuid {
    Uuid::now_v7()
}

/// Generate a unique plan ID for testing
pub fn generate_test_plan_id() -> Uuid {
    Uuid::now_v7()
}

/// ============================================================================
/// Legacy Builders (retained for existing tests that have not yet been migrated)
/// ============================================================================
/// These builders reference the pre-product_reduce schema (plan_id, planId).
/// New tests should use the herald_* metadata builders above.
/// Pending work will migrate or remove the tests that depend on these.
///
/// Build a subscription.paid webhook event (legacy, uses plan_id)
pub fn build_subscription_paid_event(
    event_id: String,
    user_id: Uuid,
    plan_id: Uuid,
    is_renewal: bool,
    realm_id: &str,
) -> serde_json::Value {
    build_subscription_paid_event_with_client(
        event_id, user_id, plan_id, is_renewal, realm_id, None,
    )
}

/// Build a subscription.paid webhook event with client_app_id (legacy)
pub fn build_subscription_paid_event_with_client(
    event_id: String,
    user_id: Uuid,
    plan_id: Uuid,
    is_renewal: bool,
    realm_id: &str,
    client_app_id: Option<Uuid>,
) -> serde_json::Value {
    build_subscription_paid_event_with_client_and_status(
        event_id,
        user_id,
        plan_id,
        is_renewal,
        realm_id,
        client_app_id,
        "active",
    )
}

/// Build a subscription.paid webhook event with client_app_id and custom status (legacy)
pub fn build_subscription_paid_event_with_client_and_status(
    event_id: String,
    user_id: Uuid,
    plan_id: Uuid,
    is_renewal: bool,
    realm_id: &str,
    client_app_id: Option<Uuid>,
    status: &str,
) -> serde_json::Value {
    build_subscription_paid_event_full(
        event_id,
        user_id,
        plan_id,
        is_renewal,
        realm_id,
        client_app_id,
        status,
        "monthly",
    )
}

/// Build a subscription.paid webhook event with full customization (legacy)
pub fn build_subscription_paid_event_full(
    event_id: String,
    user_id: Uuid,
    plan_id: Uuid,
    is_renewal: bool,
    realm_id: &str,
    client_app_id: Option<Uuid>,
    status: &str,
    billing_period: &str,
) -> serde_json::Value {
    let now = chrono::Utc::now();
    let (period_start, period_end) = match billing_period {
        "yearly" => (now, now + chrono::Duration::days(365)),
        "quarterly" => (now, now + chrono::Duration::days(90)),
        _ => (now, now + chrono::Duration::days(30)),
    };

    let amount = match billing_period {
        "yearly" => 25000,
        "quarterly" => 7500,
        _ => 2500,
    };

    let mut metadata = json!({
        "realmId": realm_id
    });

    if let Some(client_id) = client_app_id {
        metadata["clientAppId"] = json!(client_id.to_string());
    }

    json!({
        "id": event_id,
        "eventType": "subscription.paid",
        "data": {
            "object": {
                "subscriptionId": format!("sub_{}", event_id),
                "productId": format!("prod_test_{}", billing_period),
                "userId": user_id.to_string(),
                "planId": plan_id.to_string(),
                "entitlementKey": plan_id.to_string(),
                "isRenewal": is_renewal,
                "currentPeriodStart": period_start.to_rfc3339(),
                "currentPeriodEnd": period_end.to_rfc3339(),
                "amount": amount,
                "currency": "USD",
                "status": status,
                "cancelAtPeriodEnd": false,
                "billingPeriod": billing_period,
                "metadata": metadata
            }
        },
        "metadata": metadata
    })
}

/// Build a subscription.canceled webhook event (legacy)
pub fn build_subscription_canceled_event(
    event_id: String,
    user_id: Uuid,
    cancel_at_period_end: bool,
    realm_id: &str,
) -> serde_json::Value {
    let period_end = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();

    json!({
        "id": event_id,
        "eventType": "subscription.canceled",
        "data": {
            "object": {
                "subscriptionId": format!("sub_{}", event_id),
                "productId": "prod_test_monthly",
                "userId": user_id.to_string(),
                "cancelAtPeriodEnd": cancel_at_period_end,
                "currentPeriodEnd": period_end
            }
        },
        "metadata": {
            "realmId": realm_id
        }
    })
}

/// Build a subscription.update webhook event (legacy)
///
/// Uses the shared `prod_test_monthly` product id. Only safe for single-plan
/// realms — when two plans coexist, the price-aware webhook resolver cannot
/// distinguish them by product. Multi-plan upgrade/downgrade scenarios must use
/// `build_subscription_updated_event_with_product` so the new plan resolves to
/// its own entitlement mapping.
pub fn build_subscription_updated_event(
    event_id: String,
    user_id: Uuid,
    previous_plan_id: Uuid,
    current_plan_id: Uuid,
    realm_id: &str,
) -> serde_json::Value {
    build_subscription_updated_event_with_product(
        event_id,
        user_id,
        previous_plan_id,
        current_plan_id,
        realm_id,
        "prod_test_monthly",
    )
}

/// Build a subscription.update webhook event with a custom product id.
///
/// Required for multi-plan scenarios: each plan's mapping has a distinct
/// `external_product_id`, and the price-aware webhook resolver (US-EM-008)
/// keys off `(provider, product, price)`. Passing the new plan's product id
/// lets the resolver land on the correct plan-specific mapping instead of
/// colliding on the shared `prod_test_monthly` fallback.
pub fn build_subscription_updated_event_with_product(
    event_id: String,
    user_id: Uuid,
    previous_plan_id: Uuid,
    current_plan_id: Uuid,
    realm_id: &str,
    product_id: &str,
) -> serde_json::Value {
    let period_end = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();

    json!({
        "id": event_id,
        "eventType": "subscription.update",
        "data": {
            "object": {
                "subscriptionId": format!("sub_{}", event_id),
                "productId": product_id,
                "userId": user_id.to_string(),
                "planId": current_plan_id.to_string(),
                "entitlementKey": current_plan_id.to_string(),
                "currentPeriodEnd": period_end
            },
            "previousAttributes": {
                "planId": previous_plan_id.to_string(),
                "entitlementKey": previous_plan_id.to_string()
            }
        },
        "metadata": {
            "realmId": realm_id
        }
    })
}

/// Build a refund.created webhook event (legacy)
pub fn build_refund_created_event(
    event_id: String,
    refund_id: String,
    payment_id: String,
    amount: i64,
    original_amount: i64,
    realm_id: &str,
) -> serde_json::Value {
    build_refund_created_event_with_user(
        event_id,
        refund_id,
        payment_id,
        amount,
        original_amount,
        realm_id,
        Uuid::now_v7(),
    )
}

/// Build a refund.created webhook event with explicit user ID (legacy)
pub fn build_refund_created_event_with_user(
    event_id: String,
    refund_id: String,
    payment_id: String,
    amount: i64,
    original_amount: i64,
    realm_id: &str,
    user_id: Uuid,
) -> serde_json::Value {
    build_refund_created_event_with_user_and_type(
        event_id,
        refund_id,
        payment_id,
        amount,
        original_amount,
        realm_id,
        user_id,
        "topup",
    )
}

/// Build a refund.created webhook event with explicit user ID and refund type (legacy)
pub fn build_refund_created_event_with_user_and_type(
    event_id: String,
    refund_id: String,
    payment_id: String,
    amount: i64,
    original_amount: i64,
    realm_id: &str,
    user_id: Uuid,
    refund_type: &str,
) -> serde_json::Value {
    json!({
        "id": event_id,
        "eventType": "refund.created",
        "data": {
            "object": {
                "id": refund_id,
                "paymentId": payment_id,
                "amount": amount,
                "originalAmount": original_amount,
                "currency": "USD",
                "reason": "customer_request",
                "metadata": {
                    "userId": user_id.to_string(),
                    "refundType": refund_type
                }
            }
        },
        "metadata": {
            "realmId": realm_id
        }
    })
}

/// Setup a test plan config by creating a provider_entitlement_mappings entry.
pub async fn setup_test_plan_config(
    ctx: &crate::tests::schema_test_context::SchemaTestContext,
    realm_id: &str,
    plan_id: Uuid,
) {
    setup_test_plan_config_with_points(ctx, realm_id, plan_id, 1000).await;
}

/// Setup a test plan config by creating two `provider_entitlement_mappings`
/// entries (plan-specific + generic `prod_test_monthly`) for webhook resolver
/// coverage.
///
/// Distribution-rules model: grant config lives in `points_distribution_rules`,
/// so the mapping rows carry no grant columns. Both rows are bare; callers that
/// need points grants seed a distribution rule separately. The existing callers
/// (invoice attribution/sync, role-grant paywall tests) assert invoice rows or
/// role grants, not points balances, so no rule is seeded here.
/// `points_per_period` is retained in the signature for call-site stability.
pub async fn setup_test_plan_config_with_points(
    ctx: &crate::tests::schema_test_context::SchemaTestContext,
    realm_id: &str,
    plan_id: Uuid,
    _points_per_period: i64,
) {
    let external_product_id = format!("prod_test_{}", plan_id);
    let entitlement_key = plan_id.to_string();

    // Create mapping with plan-specific external_product_id (for events that include entitlementKey)
    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key,
             billing_type, billing_period, enabled)
         VALUES ($1, $2, 'creem', $3, $4, 'recurring', 'monthly', true)
         ON CONFLICT DO NOTHING",
    )
    .bind(plan_id)
    .bind(realm_id)
    .bind(&external_product_id)
    .bind(&entitlement_key)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test entitlement mapping");

    // Create additional mapping with generic "prod_test_monthly" for update/cancel events
    // that resolve entitlement_key via product ID lookup
    let generic_mapping_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key,
             billing_type, billing_period, enabled)
         VALUES ($1, $2, 'creem', 'prod_test_monthly', $3, 'recurring', 'monthly', true)
         ON CONFLICT DO NOTHING",
    )
    .bind(generic_mapping_id)
    .bind(realm_id)
    .bind(&entitlement_key)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create generic test entitlement mapping");
}

/// ============================================================================
/// Database Setup Helpers (entitlement mapping for webhook tests)
/// ============================================================================
///
/// Create a test entitlement mapping for webhook tests via direct SQL.
///
/// This is self-contained in webhook_helpers.rs (not dependent on billing_helpers.rs)
/// because the parallel test groups run independently.
///
/// Returns the mapping ID.
#[allow(clippy::too_many_arguments)]
pub async fn setup_test_entitlement_mapping_for_webhook(
    ctx: &crate::tests::schema_test_context::SchemaTestContext,
    realm_id: &str,
    provider: &str,
    external_product_id: &str,
    entitlement_key: &str,
    _points_per_period: i64,
    _grant_on_subscribe: bool,
    enabled: bool,
) -> Uuid {
    let mapping_id = Uuid::now_v7();

    // Distribution-rules model: grant config lives in `points_distribution_rules`
    // (owner = entitlement_mapping), so the mapping row carries no grant columns.
    // Mirrors `setup_test_entitlement_mapping` (bare row). The existing callers
    // (role-grant paywall tests) assert role grants driven by `granted_role_ids`,
    // not points balances, so no rule is seeded here. `points_per_period` /
    // `grant_on_subscribe` are retained in the signature for call-site stability.
    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key, enabled)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .bind(provider)
    .bind(external_product_id)
    .bind(entitlement_key)
    .bind(enabled)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test entitlement mapping for webhook");

    mapping_id
}

/// Build a `charge.refunded` Stripe webhook event for a one-time (topup)
/// refund with the full incremental shape: an explicit `refunds.data[0]`
/// entry carrying THIS refund's `id` and `amount`, plus the cumulative
/// `amount_refunded` on the charge. The two are deliberately independent
/// parameters — Stripe sends `amount_refunded` as the running total across
/// all refunds of the charge, while `refunds.data[0]` is the single refund
/// object that triggered this event.
///
/// `amount` is the original charge total.
#[allow(clippy::too_many_arguments)]
pub fn build_stripe_charge_refunded_topup_event(
    event_id: &str,
    realm_id: &str,
    user_id: Uuid,
    charge_id: &str,
    amount: i64,
    amount_refunded: i64,
    refund_id: &str,
    refund_amount: i64,
) -> serde_json::Value {
    json!({
        "id": event_id,
        "object": "event",
        "type": "charge.refunded",
        "api_version": "2020-08-27",
        "created": chrono::Utc::now().timestamp(),
        "data": {
            "object": {
                "id": charge_id,
                "object": "charge",
                "amount": amount,
                "amount_refunded": amount_refunded,
                "refunds": {
                    "object": "list",
                    "data": [
                        {
                            "id": refund_id,
                            "object": "refund",
                            "amount": refund_amount,
                            "status": "succeeded",
                            "created": chrono::Utc::now().timestamp(),
                        }
                    ]
                },
                "metadata": {
                    "herald_realm_id": realm_id,
                    "herald_user_id": user_id.to_string(),
                    "userId": user_id.to_string(),
                    "refundType": "topup",
                },
                "created": chrono::Utc::now().timestamp(),
            }
        }
    })
}

/// ============================================================================
/// Stripe Webhook Sending Helpers
/// ============================================================================
/// Build a Stripe webhook signature for testing
///
/// Follows Stripe's webhook signature format: t={timestamp},v1={signature}
pub fn build_stripe_webhook_signature(payload: &str, secret: &str) -> String {
    let timestamp = chrono::Utc::now().timestamp();
    let signed_payload = format!("{}.{}", timestamp, payload);

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(signed_payload.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    format!("t={},v1={}", timestamp, signature)
}

/// Send a Stripe webhook event with signature
pub async fn send_stripe_webhook_with_signature(
    app: &axum::Router,
    realm_id: &str,
    payload: serde_json::Value,
    webhook_secret: &str,
) -> axum::response::Response {
    let payload_str = serde_json::to_string(&payload).unwrap();
    let signature = build_stripe_webhook_signature(&payload_str, webhook_secret);
    let uri = &format!("/api/third/pay/{}/stripe/webhooks", realm_id);

    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("stripe-signature", signature)
                .header("Content-Type", "application/json")
                .body(Body::from(payload_str))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Send a Stripe webhook event with invalid signature
pub async fn send_stripe_webhook_with_invalid_signature(
    app: &axum::Router,
    realm_id: &str,
    payload: serde_json::Value,
) -> axum::response::Response {
    let payload_str = serde_json::to_string(&payload).unwrap();
    let invalid_signature = "t=1234567890,v1=invalid_signature_here_aabbccddeeff";
    let uri = &format!("/api/third/pay/{}/stripe/webhooks", realm_id);

    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("stripe-signature", invalid_signature)
                .header("Content-Type", "application/json")
                .body(Body::from(payload_str))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Send a Stripe webhook event without signature
pub async fn send_stripe_webhook_without_signature(
    app: &axum::Router,
    realm_id: &str,
    payload: serde_json::Value,
) -> axum::response::Response {
    let payload_str = serde_json::to_string(&payload).unwrap();
    let uri = &format!("/api/third/pay/{}/stripe/webhooks", realm_id);

    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("Content-Type", "application/json")
                .body(Body::from(payload_str))
                .unwrap(),
        )
        .await
        .unwrap()
}
