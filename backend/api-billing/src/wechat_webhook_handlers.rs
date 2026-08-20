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
        .find_payment_event_by_external_id(&notification.id, "wechat")
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

/// Reverse-lookup the attempt by `out_trade_no`, enforce the amount check, and
/// fulfil on `trade_state = SUCCESS` via the shared provider-agnostic pipeline.
async fn process_decided(
    app_state: &AppState,
    realm_id: &str,
    event_id: &str,
    decrypted: &herald_infra_wechatpay::DecryptedResource,
) -> Result<(), CoreError> {
    let attempt = app_state
        .payment_attempt_service
        .get_payment_attempt_by_provider_reference("wechat", &decrypted.out_trade_no)
        .await
        .map_err(|e| {
            CoreError::InternalServerError(format!("Failed to load WeChat payment attempt: {e}"))
        })?
        .ok_or_else(|| {
            CoreError::BadRequest(format!(
                "WeChat callback: no attempt for out_trade_no {}",
                decrypted.out_trade_no
            ))
        })?;

    if attempt.realm_id != realm_id {
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
            transaction_id,
            chrono::Utc::now(),
            None,
        )
        .await?;
        info!(
            realm_id = %realm_id,
            event_id = %event_id,
            attempt_id = %attempt.id,
            out_trade_no = %decrypted.out_trade_no,
            "WeChat payment fulfilled"
        );
    } else {
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

    process_decided(&app_state, &realm_id, &notification.id, &decrypted).await
}
