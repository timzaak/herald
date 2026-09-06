use axum::{
    Router,
    middleware::from_fn,
    routing::{get, post, put},
};

use crate::credit_bucket_handlers::{
    create_credit_bucket_handler, delete_credit_bucket_handler, get_bucket_overview_handler,
    get_credit_bucket_handler, list_credit_buckets_handler, update_credit_bucket_handler,
};
use crate::entitlement_mapping_handlers::{
    batch_update_entitlement_mappings, create_entitlement_mapping, get_entitlement_mapping,
    list_entitlement_mappings, list_one_time_mappings, sync_provider_products,
    update_entitlement_mapping,
};
use crate::feature_availability::{get_feature_availability, get_user_feature_availability};
use crate::handlers::{
    cancel_subscription_for_client_app, get_subscription, get_subscription_for_client_app,
    list_purchase_options, list_subscriptions,
};
use crate::handlers_history::{
    get_my_subscription_history, get_subscription_history, list_my_subscription_history,
    list_subscription_history,
};
use crate::iap_handlers::{handle_apple_webhook, submit_iap_receipt};
use crate::invoice_handlers::{
    apply_invoice, create_credit_note, create_invoice, download_invoice_pdf,
    download_my_invoice_pdf, get_invoice, get_invoice_apply_eligibility, get_my_invoice_scoped,
    get_seller_config, issue_invoice, list_attribution_anomalies, list_invoices, list_my_invoices,
    mark_paid, update_invoice, upsert_seller_config, void_invoice,
};
use crate::provider_handlers::list_payment_providers;
use crate::purchase_handlers::{
    cancel_payment_attempt, create_payment_attempt, fulfill_payment, get_payment_attempt_status,
    get_purchase_history, get_realm_purchase_history,
};
use crate::stripe_webhook_handlers::handle_stripe_webhook;
use crate::webhook_handlers::handle_creem_webhook;
use crate::wechat_webhook_handlers::handle_wechat_webhook;
use herald_api_base::application::http::internal_auth::internal_api_key_middleware;
use herald_api_base::application::http::state::AppState;

pub fn billing_public_routes() -> Router<AppState> {
    Router::new()
        // ===== Webhooks =====
        .route(
            "/api/third/pay/{realmId}/creem/webhooks",
            post(handle_creem_webhook),
        )
        .route(
            "/api/third/pay/{realmId}/stripe/webhooks",
            post(handle_stripe_webhook),
        )
        // Apple App Store Server Notifications V2 (design support-iap §5.5).
        // Unauthenticated HTTP; the JWS signature is the trust root.
        .route(
            "/api/third/pay/{realmId}/apple/webhooks",
            post(handle_apple_webhook),
        )
        // WeChat Pay v3 payment-result callback. Unauthenticated HTTP; the
        // WeChat platform certificate signature is the trust root.
        .route(
            "/api/third/pay/{realmId}/wechat/webhooks",
            post(handle_wechat_webhook),
        )
        // ===== Internal Fulfillment Webhook =====
        .route(
            "/api/internal/bill/purchase/payment-attempts/{attemptId}/fulfill",
            post(fulfill_payment).layer(from_fn(internal_api_key_middleware)),
        )
}

pub fn billing_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/bill/{realmId}/purchase/history",
            get(get_realm_purchase_history),
        )
        // ===== Feature Availability =====
        .route(
            "/api/realms/{realmId}/feature-availability",
            get(get_feature_availability),
        )
        // ===== Credit Bucket Directory =====
        // NOTE: `/overview` is registered BEFORE `/{bucketId}` so the static
        // segment is matched unambiguously (Axum matchit prefers static over
        // dynamic, but explicit ordering keeps the intent legible).
        .route(
            "/api/realms/{realmId}/billing/credit-buckets",
            get(list_credit_buckets_handler).post(create_credit_bucket_handler),
        )
        .route(
            "/api/realms/{realmId}/billing/credit-buckets/overview",
            get(get_bucket_overview_handler),
        )
        .route(
            "/api/realms/{realmId}/billing/credit-buckets/{bucketId}",
            get(get_credit_bucket_handler)
                .put(update_credit_bucket_handler)
                .delete(delete_credit_bucket_handler),
        )
        // ===== Entitlement Mapping =====
        .route(
            "/api/bill/{realmId}/entitlement-mappings",
            get(list_entitlement_mappings).post(create_entitlement_mapping),
        )
        .route(
            "/api/bill/{realmId}/one-time-mappings",
            get(list_one_time_mappings),
        )
        .route(
            "/api/bill/{realmId}/entitlement-mappings/sync",
            post(sync_provider_products),
        )
        // Static `batch` segment is registered BEFORE `/{mappingId}` so it is
        // matched unambiguously (same convention as `/overview` above).
        .route(
            "/api/bill/{realmId}/entitlement-mappings/batch",
            put(batch_update_entitlement_mappings),
        )
        .route(
            "/api/bill/{realmId}/entitlement-mappings/{mappingId}",
            get(get_entitlement_mapping).patch(update_entitlement_mapping),
        )
        // ===== Subscription List/Detail =====
        .route("/api/bill/{realmId}/subscriptions", get(list_subscriptions))
        .route(
            "/api/bill/{realmId}/subscriptions/{subscriptionId}",
            get(get_subscription),
        )
        // ===== Subscription History =====
        .route(
            "/api/bill/{realmId}/subscriptions/{subscriptionId}/history",
            get(get_subscription_history),
        )
        .route(
            "/api/bill/{realmId}/subscriptions/history",
            get(list_subscription_history),
        )
        // ===== Invoice Management =====
        .route(
            "/api/bill/{realmId}/invoice-seller-config",
            get(get_seller_config).put(upsert_seller_config),
        )
        .route(
            "/api/bill/{realmId}/invoice-attribution/anomalies",
            get(list_attribution_anomalies),
        )
        .route(
            "/api/bill/{realmId}/invoices",
            get(list_invoices).post(create_invoice),
        )
        .route(
            "/api/bill/{realmId}/invoices/{invoiceId}",
            get(get_invoice).patch(update_invoice),
        )
        .route(
            "/api/bill/{realmId}/invoices/{invoiceId}/issue",
            post(issue_invoice),
        )
        .route(
            "/api/bill/{realmId}/invoices/{invoiceId}/void",
            post(void_invoice),
        )
        .route(
            "/api/bill/{realmId}/invoices/{invoiceId}/mark-paid",
            post(mark_paid),
        )
        .route(
            "/api/bill/{realmId}/invoices/{invoiceId}/pdf",
            get(download_invoice_pdf),
        )
        .route(
            "/api/bill/{realmId}/invoices/{invoiceId}/credit-notes",
            post(create_credit_note),
        )
}

/// Browser-token billing endpoints from the CustomUserUi allowlist.
///
/// Mounted separately from `billing_routes` so a CustomUserUi credential can
/// never enter the admin billing router.
pub fn billing_browser_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/bill/{realmId}/client/{clientAppId}/subscription",
            get(get_subscription_for_client_app),
        )
        // User self-service cancel: calls provider cancel API (Stripe/Creem),
        // local status updated via webhook. Mounted here so a CustomUserUi
        // browser token with the SubscriptionCancel scope may cancel the user's
        // own subscription; admin console no longer cancels directly.
        .route(
            "/api/bill/{realmId}/client/{clientAppId}/subscription/cancel",
            post(cancel_subscription_for_client_app),
        )
        .route(
            "/api/bill/{realmId}/client/{clientAppId}/purchase-options",
            get(list_purchase_options),
        )
        .route(
            "/api/bill/{realmId}/purchase/payment-attempts",
            post(create_payment_attempt),
        )
        .route(
            "/api/bill/{realmId}/purchase/payment-attempts/{attemptId}",
            get(get_payment_attempt_status),
        )
        // User self-service attempt cancel: the purchase page renders a cancel
        // entry on the pending payment step (QR / redirect prompt). The handler
        // enforces realm membership + attempt ownership, so it belongs on the
        // browser router alongside create/get — the admin router mount 403'd
        // CustomUserUi tokens.
        .route(
            "/api/bill/{realmId}/purchase/payment-attempts/{attemptId}/cancel",
            post(cancel_payment_attempt),
        )
        // IAP receipt submission (design support-iap §5.2). CustomUserUi token
        // + `PurchaseInitiate` scope enforced inside the handler.
        .route(
            "/api/bill/{realmId}/purchase/iap/receipt",
            post(submit_iap_receipt),
        )
        .route(
            "/api/third/pay/{realmId}/providers",
            get(list_payment_providers),
        )
        .route(
            "/api/bill/{realmId}/my/subscriptions/history",
            get(list_my_subscription_history),
        )
        .route(
            "/api/bill/{realmId}/my/subscriptions/{subscriptionId}/history",
            get(get_my_subscription_history),
        )
}

pub fn billing_user_routes() -> Router<AppState> {
    Router::new()
        .route("/feature-availability", get(get_user_feature_availability))
        .route("/bill/purchase/history", get(get_purchase_history))
        .route(
            "/bill/invoices/apply-eligibility",
            get(get_invoice_apply_eligibility),
        )
        .route("/bill/invoices", get(list_my_invoices).post(apply_invoice))
        .route("/bill/invoices/{invoiceId}", get(get_my_invoice_scoped))
        .route(
            "/bill/invoices/{invoiceId}/pdf",
            get(download_my_invoice_pdf),
        )
}

/// Test routes for billing integration tests
/// Always compiled but only used in test builds
pub fn billing_test_routes() -> Router<AppState> {
    Router::new()
}
