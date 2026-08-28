//! WeChat Pay v3 payment-result callback handler.
//!
//! Public endpoint (no auth): trust root is the WeChat platform certificate
//! signature. Protocol exception — responses follow WeChat's
//! `{"code":"SUCCESS"|"FAIL",...}` shape, not the unified API response wrapper.
//!
//! Flow: verify signature (platform cert by `Wechatpay-Serial`) → decrypt
//! `resource` (APIv3 Key) → `payment_event` idempotency by WeChat `id` → reverse
//! `payment_attempts.provider_reference = out_trade_no` → amount check → on
//! `trade_state=SUCCESS` fulfil via the shared unified pipeline. Verification /
//! decrypt / amount / lookup failures never mutate attempt state; they reply
//! `FAIL` so WeChat retries, and `compensation::reprocess_wechat_event` can
//! replay later.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use uuid::Uuid;

use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::billing::{BillingRepository, PaymentEvent};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::infrastructure::wechatpay::get_wechat_client_for_realm;
use herald_infra_wechatpay::{EncryptedResource, WechatPayError};

use crate::shared_fulfillment::fulfill_provider_event;

/// WeChat's required response body for callbacks.
#[derive(Debug, Serialize)]
pub struct WechatWebhookResponse {
    code: &'static str,
    message: String,
}

impl WechatWebhookResponse {
    fn success() -> Self {
        Self {
            code: "SUCCESS",
            message: "成功".to_string(),
        }
    }
    fn fail(message: impl Into<String>) -> Self {
        Self {
            code: "FAIL",
            message: message.into(),
        }
    }
}

type WebhookResult =
    Result<(StatusCode, Json<WechatWebhookResponse>), (StatusCode, Json<WechatWebhookResponse>)>;

/// Top-level WeChat notification envelope (only the fields this handler needs).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct WechatNotification {
    id: String,
    event_type: String,
    resource: EncryptedResource,
}

#[utoipa::path(
    post,
    path = "/api/third/pay/{realmId}/wechat/webhooks",
    params(
        ("realmId" = String, Path, description = "Realm id")
    ),
    responses(
        (status = 200, description = "Callback processed (SUCCESS)", content_type = "application/json"),
        (status = 422, description = "Callback rejected (FAIL — signature/decrypt/amount/lookup)"),
        (status = 500, description = "Internal error (FAIL — WeChat will retry)")
    ),
    tag = "billing.wechat-webhooks",
    operation_id = "wechat_webhook_handler"
)]
pub async fn handle_wechat_webhook(
    State(app_state): State<AppState>,
    Path(realm_id): Path<String>,
    headers: axum::http::HeaderMap,
    body: String,
) -> WebhookResult {
    let result = process_wechat_callback(&app_state, &realm_id, &headers, &body).await;
    match result {
        Ok(()) => Ok((StatusCode::OK, Json(WechatWebhookResponse::success()))),
        Err(e) => {
            // Security rejections and client/protocol errors → 422 FAIL (no
            // state mutated). Internal/transport errors → 500 FAIL so WeChat
            // retries and `compensation` can replay. Security classification
            // happens at the `WechatPayError → CoreError` conversion points
            // (`wechat_err_to_core`, `get_wechat_client_for_realm`).
            let status = if matches!(&e, CoreError::BadRequest(_)) {
                StatusCode::UNPROCESSABLE_ENTITY
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            let message = e.to_string();
            error!(realm_id = %realm_id, error = %message, "WeChat callback rejected");
            Err((status, Json(WechatWebhookResponse::fail(message))))
        }
    }
}

async fn process_wechat_callback(
    app_state: &AppState,
    realm_id: &str,
    headers: &axum::http::HeaderMap,
    body: &str,
) -> Result<(), CoreError> {
    let timestamp = read_header(headers, "Wechatpay-Timestamp")?;
    let nonce = read_header(headers, "Wechatpay-Nonce")?;
    let signature = read_header(headers, "Wechatpay-Signature")?;
    let serial = read_header(headers, "Wechatpay-Serial")?;

    // Replay protection mirroring the Stripe gate: the signature stays valid
    // forever, so a captured request could otherwise be replayed indefinitely
    // (event-id idempotency only prevents double fulfilment, not re-execution).
    // WeChat re-delivers notifications as fresh requests with a new timestamp,
    // so a live sender is never outside the window.
    let timestamp_i64: i64 = timestamp
        .parse()
        .map_err(|_| CoreError::BadRequest("invalid Wechatpay-Timestamp".to_string()))?;
    let age_seconds = chrono::Utc::now().timestamp() - timestamp_i64;
    if !(-900..=900).contains(&age_seconds) {
        return Err(CoreError::BadRequest(format!(
            "Wechatpay-Timestamp outside the accepted window: {age_seconds} seconds"
        )));
    }

    let client = get_wechat_client_for_realm(&app_state.pool, realm_id).await?;

    // Verify the request signature using the platform certificate for `serial`
    // (auto-downloaded + cached, or the manual override). Failure must NOT
    // mutate state.
    client
        .verify_callback(realm_id, &timestamp, &nonce, &signature, &serial, body)
        .await
        .map_err(wechat_err_to_core)?;

    let notification: WechatNotification = serde_json::from_str(body)
        .map_err(|e| CoreError::BadRequest(format!("invalid wechat payload: {e}")))?;

    let decrypted = client
        .decrypt_resource(&notification.resource)
        .map_err(wechat_err_to_core)?;

    // Idempotency by WeChat notification `id` + provider "wechat".
    let existing_event = app_state
        .billing_repository
        .find_payment_event_by_external_id(realm_id, &notification.id, "wechat")
        .await?;
    if existing_event.as_ref().is_some_and(|e| e.processed) {
        info!(
            realm_id = %realm_id,
            event_id = %notification.id,
            "WeChat callback already processed (idempotent)"
        );
        return Ok(());
    }

    let saved_event = match existing_event {
        Some(existing) => existing,
        None => {
            let new_event = PaymentEvent {
                id: Uuid::now_v7(),
                realm_id: realm_id.to_string(),
                external_event_id: notification.id.clone(),
                payment_provider: "wechat".to_string(),
                event_type: notification.event_type.clone(),
                subscription_id: None,
                payload: serde_json::from_str(body).unwrap_or(serde_json::Value::Null),
                processed: false,
                processing_started_at: None,
                created_at: chrono::Utc::now(),
            };
            match app_state
                .billing_repository
                .create_payment_event(new_event)
                .await
            {
                Ok(saved) => saved,
                Err(CoreError::DatabaseError(msg))
                    if msg.contains("unique constraint") || msg.contains("duplicate key") =>
                {
                    // Concurrent delivery with the same id; another worker handles it.
                    info!(
                        realm_id = %realm_id,
                        event_id = %notification.id,
                        "Concurrent WeChat callback already inserted"
                    );
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        }
    };

    let outcome = process_decided(app_state, realm_id, &notification.id, &decrypted).await;

    // Only mark processed on success; failures leave processed=false so the
    // compensation sweep can replay, and WeChat's own retry will re-attempt.
    if let Err(e) = &outcome {
        warn!(
            realm_id = %realm_id,
            event_id = %notification.id,
            error = %e,
            "WeChat callback fulfillment failed; leaving event unprocessed for retry"
        );
        return Err(e.clone());
    }

    if let Err(e) = app_state
        .billing_repository
        .mark_payment_event_processed(saved_event.id)
        .await
    {
        error!(
            realm_id = %realm_id,
            event_id = %notification.id,
            error = %e,
            "Failed to mark WeChat payment event processed"
        );
        return Err(e);
    }

    Ok(())
}

/// Best-effort audit write for WeChat payment operations (PRD wechat-support
/// §4.1: "所有 WeChat 配置变更与支付操作必须记录审计日志"). The actor is the
/// system: the webhook is unauthenticated (trust root is the platform
/// signature) and replays are initiated by the compensation sweep. An audit
/// failure must never fail the payment operation itself. Early rejections
/// before the payment event row exists (replay window / signature / decrypt /
/// idempotent skip) stay tracing-only — they mutate no state and auditing
/// them would let unauthenticated noise flood the audit table.
async fn audit_wechat_payment_event(
    app_state: &AppState,
    realm_id: &str,
    event_id: &str,
    action: AuditAction,
    result: AuditResult,
    details: serde_json::Value,
) {
    if let Err(e) = app_state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.to_string(),
            category: AuditCategory::Billing,
            action,
            actor_id: "wechat-webhook".to_string(),
            actor_type: Some(ActorType::System),
            actor_name: None,
            target_type: AuditTargetType::Payment,
            target_id: event_id.to_string(),
            target_name: None,
            result,
            details: Some(details),
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(
            error = %e,
            realm_id = %realm_id,
            event_id = %event_id,
            "Failed to record WeChat payment audit event"
        );
    }
}

fn webhook_audit_details(
    event_id: &str,
    out_trade_no: &str,
    extra: &[(&str, serde_json::Value)],
) -> serde_json::Value {
    let mut details = serde_json::json!({
        "provider": "wechat",
        "external_event_id": event_id,
        "out_trade_no": out_trade_no,
    });
    if let Some(map) = details.as_object_mut() {
        for (key, value) in extra {
            map.insert((*key).to_string(), value.clone());
        }
    }
    details
}

/// Reverse-lookup the attempt by `out_trade_no`, enforce the amount check, and
/// fulfil on `trade_state = SUCCESS` via the shared provider-agnostic pipeline.
async fn process_decided(
    app_state: &AppState,
    realm_id: &str,
    event_id: &str,
    decrypted: &herald_infra_wechatpay::DecryptedResource,
) -> Result<(), CoreError> {
    let attempt = match app_state
        .payment_attempt_service
        .get_payment_attempt_by_provider_reference("wechat", &decrypted.out_trade_no)
        .await
    {
        Ok(Some(found)) => found,
        Ok(None) => {
            audit_wechat_payment_event(
                app_state,
                realm_id,
                event_id,
                AuditAction::PaymentWebhook,
                AuditResult::Failure,
                webhook_audit_details(
                    event_id,
                    &decrypted.out_trade_no,
                    &[("reason", serde_json::json!("attempt_not_found"))],
                ),
            )
            .await;
            return Err(CoreError::BadRequest(format!(
                "WeChat callback: no attempt for out_trade_no {}",
                decrypted.out_trade_no
            )));
        }
        Err(e) => {
            return Err(CoreError::InternalServerError(format!(
                "Failed to load WeChat payment attempt: {e}"
            )));
        }
    };

    if attempt.realm_id != realm_id {
        audit_wechat_payment_event(
            app_state,
            realm_id,
            event_id,
            AuditAction::PaymentWebhook,
            AuditResult::Failure,
            webhook_audit_details(
                event_id,
                &decrypted.out_trade_no,
                &[("reason", serde_json::json!("realm_mismatch"))],
            ),
        )
        .await;
        return Err(CoreError::BadRequest(format!(
            "WeChat callback realm mismatch for out_trade_no {}",
            decrypted.out_trade_no
        )));
    }

    let expected = attempt.amount;
    let actual = decrypted.amount.as_ref().map(|a| a.total).unwrap_or(0);
    if actual != expected {
        error!(
            realm_id = %realm_id,
            event_id = %event_id,
            out_trade_no = %decrypted.out_trade_no,
            expected,
            actual,
            "WeChat callback amount mismatch"
        );
        audit_wechat_payment_event(
            app_state,
            realm_id,
            event_id,
            AuditAction::PaymentWebhook,
            AuditResult::Failure,
            webhook_audit_details(
                event_id,
                &decrypted.out_trade_no,
                &[
                    ("reason", serde_json::json!("amount_mismatch")),
                    ("expected", serde_json::json!(expected)),
                    ("actual", serde_json::json!(actual)),
                ],
            ),
        )
        .await;
        return Err(CoreError::BadRequest(format!(
            "WeChat amount mismatch: expected {expected}, got {actual}"
        )));
    }

    if decrypted.trade_state == "SUCCESS" {
        let transaction_id = decrypted
            .transaction_id
            .clone()
            .unwrap_or_else(|| decrypted.out_trade_no.clone());
        fulfill_provider_event(
            app_state,
            realm_id,
            attempt.id,
            "wechat",
            "succeeded",
            transaction_id.clone(),
            chrono::Utc::now(),
            None,
        )
        .await?;
        audit_wechat_payment_event(
            app_state,
            realm_id,
            event_id,
            AuditAction::PaymentWebhook,
            AuditResult::Success,
            webhook_audit_details(
                event_id,
                &decrypted.out_trade_no,
                &[
                    ("outcome", serde_json::json!("fulfilled")),
                    ("attempt_id", serde_json::json!(attempt.id.to_string())),
                    ("transaction_id", serde_json::json!(transaction_id)),
                    ("trade_state", serde_json::json!(decrypted.trade_state)),
                ],
            ),
        )
        .await;
        info!(
            realm_id = %realm_id,
            event_id = %event_id,
            attempt_id = %attempt.id,
            out_trade_no = %decrypted.out_trade_no,
            "WeChat payment fulfilled"
        );
    } else {
        audit_wechat_payment_event(
            app_state,
            realm_id,
            event_id,
            AuditAction::PaymentWebhook,
            AuditResult::Success,
            webhook_audit_details(
                event_id,
                &decrypted.out_trade_no,
                &[
                    ("outcome", serde_json::json!("no_fulfillment")),
                    ("trade_state", serde_json::json!(decrypted.trade_state)),
                ],
            ),
        )
        .await;
        info!(
            realm_id = %realm_id,
            event_id = %event_id,
            out_trade_no = %decrypted.out_trade_no,
            trade_state = %decrypted.trade_state,
            "WeChat callback non-success trade_state; no fulfillment"
        );
    }

    Ok(())
}

fn read_header(headers: &axum::http::HeaderMap, name: &str) -> Result<String, CoreError> {
    headers
        .get(name)
        .and_then(|h| h.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| CoreError::BadRequest(format!("missing {name} header")))
}

/// Map a `WechatPayError` to a `CoreError`, preserving whether it is a
/// security/protocol rejection (BadRequest) vs. a retryable fault (internal).
fn wechat_err_to_core(e: WechatPayError) -> CoreError {
    if e.is_security_rejection() {
        CoreError::BadRequest(e.to_string())
    } else {
        CoreError::InternalServerError(e.to_string())
    }
}

/// Compensation entry: replay a stored WeChat `payment_event` payload. Skips
/// signature verification and idempotency-row accounting (the caller owns both)
/// and re-runs the decided fulfilment path. Mirrors `reprocess_creem_event`.
pub async fn reprocess_wechat_event(
    app_state: AppState,
    realm_id: String,
    payload: serde_json::Value,
    _event_type: String,
) -> Result<(), CoreError> {
    let notification: WechatNotification =
        serde_json::from_value(payload.clone()).map_err(|e| {
            CoreError::InternalServerError(format!("invalid wechat replay payload: {e}"))
        })?;

    let client = get_wechat_client_for_realm(&app_state.pool, &realm_id).await?;
    let decrypted = client
        .decrypt_resource(&notification.resource)
        .map_err(wechat_err_to_core)?;

    let out_trade_no = decrypted.out_trade_no.clone();
    let outcome = process_decided(&app_state, &realm_id, &notification.id, &decrypted).await;

    // Manual compensation replays are audited separately from the webhook
    // events above so administrators can distinguish replay outcomes.
    let (result, extra) = match &outcome {
        Ok(()) => (AuditResult::Success, Vec::new()),
        Err(e) => (
            AuditResult::Failure,
            vec![("reason", serde_json::Value::String(e.to_string()))],
        ),
    };
    let mut extra = extra;
    extra.push(("outcome", serde_json::json!("replayed")));
    audit_wechat_payment_event(
        &app_state,
        &realm_id,
        &notification.id,
        AuditAction::PaymentReplay,
        result,
        webhook_audit_details(&notification.id, &out_trade_no, &extra),
    )
    .await;

    outcome
}
