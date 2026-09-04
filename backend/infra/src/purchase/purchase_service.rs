// Moved from domain/purchase/services.rs to eliminate domain -> infrastructure dependency

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::PgPool;
use uuid::Uuid;

use herald_domain::billing::BillingRepository;
use herald_domain::billing::entities::BillingType;
use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::payment_attempt::entities::{PaymentAttempt, PaymentContext};
use herald_domain::payment_attempt::{
    CreatePaymentAttemptInput, PaymentAttemptRepository, PaymentAttemptService, PurchasableTarget,
};
use herald_domain::purchase::errors::{PurchaseErrorExt, PurchaseResult};
use herald_domain::purchase::ports::{FulfillmentResult, FulfillmentService};
use herald_domain::purchase::services::{
    CompletePaymentAttemptInput, CreateIapAttemptInput, CreatedPaymentAttempt,
    PaymentCompletionSource, PaymentFlow, PreparePaymentAttemptInput, PreparedPaymentAttempt,
    PurchaseTargetSnapshot, metadata_keys,
};
use herald_domain::user::UserRoleRepository;

fn build_herald_metadata(
    realm_id: &str,
    user_id: Uuid,
    target_type: &str,
    target_id: Uuid,
    attempt_id: Uuid,
) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    metadata.insert(
        metadata_keys::HERALD_REALM_ID.to_string(),
        realm_id.to_string(),
    );
    metadata.insert(
        metadata_keys::HERALD_USER_ID.to_string(),
        user_id.to_string(),
    );
    metadata.insert(
        metadata_keys::TARGET_TYPE.to_string(),
        target_type.to_string(),
    );
    metadata.insert(metadata_keys::TARGET_ID.to_string(), target_id.to_string());
    metadata.insert(
        metadata_keys::ATTEMPT_ID.to_string(),
        attempt_id.to_string(),
    );
    metadata
}

use herald_infra_creem::{CreateCheckoutRequest as CreemCreateCheckoutRequest, CreemClient};
use herald_infra_stripe::{
    CreateCheckoutRequest as StripeCreateCheckoutRequest, CreatePaymentIntentRequest, StripeClient,
};

/// flow × provider × billing_type combination gate. `PaymentIntent` is a
/// Stripe-only, one-time-only escape hatch from the hosted checkout journey.
fn validate_flow_combination(
    payment_provider: &str,
    billing_type: Option<BillingType>,
    flow: PaymentFlow,
) -> PurchaseResult<()> {
    if flow == PaymentFlow::PaymentIntent {
        if payment_provider != "stripe" {
            return Err(CoreError::BadRequest(
                "payment_intent flow is only supported for stripe".into(),
            ));
        }
        if billing_type != Some(BillingType::OneTime) {
            return Err(CoreError::BadRequest(
                "payment_intent flow is only supported for one-time purchases".into(),
            ));
        }
    }
    Ok(())
}

pub struct PurchaseService<B, PA, F, UR>
where
    B: BillingRepository,
    PA: PaymentAttemptRepository,
    F: FulfillmentService,
    UR: UserRoleRepository,
{
    pool: PgPool,
    public_base_url: String,
    billing_repository: Arc<B>,
    payment_attempt_service: Arc<PaymentAttemptService<PA>>,
    /// Direct handle to the payment-attempt repo for the M3 ownership gate
    /// (`has_succeeded_attempt`). `PaymentAttemptService` keeps its repo
    /// private, so this shares the same `Arc<PA>` instance constructed at
    /// startup — no second connection or duplicate state.
    payment_attempt_repository: Arc<PA>,
    /// User-role repo for the M3 ownership gate
    /// (`user_has_any_role`).
    user_role_repository: Arc<UR>,
    fulfillment_service: Arc<F>,
}

impl<B, PA, F, UR> PurchaseService<B, PA, F, UR>
where
    B: BillingRepository,
    PA: PaymentAttemptRepository,
    F: FulfillmentService,
    UR: UserRoleRepository,
{
    pub fn new(
        pool: PgPool,
        public_base_url: String,
        billing_repository: Arc<B>,
        payment_attempt_service: Arc<PaymentAttemptService<PA>>,
        payment_attempt_repository: Arc<PA>,
        user_role_repository: Arc<UR>,
        fulfillment_service: Arc<F>,
    ) -> Self {
        Self {
            pool,
            public_base_url,
            billing_repository,
            payment_attempt_service,
            payment_attempt_repository,
            user_role_repository,
            fulfillment_service,
        }
    }

    pub async fn prepare_payment_attempt(
        &self,
        input: PreparePaymentAttemptInput,
    ) -> PurchaseResult<PreparedPaymentAttempt> {
        if input.user_email.is_none() {
            return Err(CoreError::BadRequest(
                "A formal user email is required for payment providers".to_string(),
            ));
        }

        let target = self
            .resolve_target(
                &input.realm_id,
                input.user_id,
                &input.target_type,
                input.target_id,
                &input.payment_provider,
            )
            .await?;

        // CHECK(amount > 0) on `payment_attempts` rejects 0. A mapping without
        // `provider_product_info.price` resolves to amount=0 (see `resolve_target`);
        // coerce to the sentinel `1` so the row satisfies the constraint without
        // fabricating a real price. Mirrors `create_iap_payment_attempt`.
        let amount = if target.amount > 0 { target.amount } else { 1 };

        let (attempt, _) = self
            .payment_attempt_service
            .create_payment_attempt(
                CreatePaymentAttemptInput {
                    realm_id: input.realm_id,
                    user_id: input.user_id,
                    payment_provider: input.payment_provider,
                    target_type: target.target_type.to_string(),
                    target_id: target.target_id,
                    billing_type: target.billing_type.clone().ok_or_else(|| {
                        CoreError::BillingError(format!(
                            "Entitlement mapping '{}' has no billing_type set",
                            target.target_id
                        ))
                    })?,
                    amount,
                    currency: target.currency.clone(),
                    provider_reference: None,
                    metadata: input.metadata,
                    is_one_time_role: target.is_one_time_role,
                },
                PaymentContext {
                    stripe_checkout_url: None,
                    creem_checkout_url: None,
                    client_secret: None,
                    wechat_code_url: None,
                    wechat_jsapi_params: None,
                },
            )
            .await?;

        Ok(PreparedPaymentAttempt { attempt, target })
    }

    pub async fn create_payment_attempt(
        &self,
        input: PreparePaymentAttemptInput,
    ) -> PurchaseResult<CreatedPaymentAttempt> {
        let user_email = input.user_email.clone();
        let flow = input.flow;
        let payment_scene = input.payment_scene.clone();
        let openid = input.openid.clone();
        let prepared = self.prepare_payment_attempt(input).await?;
        let attempt_expires_at = prepared.attempt.expires_at;
        let (provider_reference, context) = self
            .build_payment_context(
                &prepared.attempt.realm_id,
                prepared.attempt.user_id,
                &prepared.attempt.target_type.to_string(),
                prepared.attempt.target_id,
                &prepared.attempt.payment_provider,
                &prepared.target,
                prepared.attempt.id,
                flow,
                user_email.as_deref(),
                payment_scene.as_deref(),
                openid.as_deref(),
                attempt_expires_at,
            )
            .await?;

        let attempt = self
            .payment_attempt_service
            .update_provider_reference(
                &prepared.attempt.realm_id,
                prepared.attempt.id,
                provider_reference,
            )
            .await?;

        Ok(CreatedPaymentAttempt { attempt, context })
    }

    /// support-iap §5.2).
    ///
    /// IAP (Apple App Store / Google Play) is a reverse-payment semantic: the
    /// purchase has already happened on the store and the client submits a
    /// credential (Apple `jwsRepresentation` / Google `purchaseToken`). This
    /// method therefore reuses `resolve_target` (mapping lookup + M3
    /// one-time-role ownership gate) and the attempt-row creation path, but
    /// **skips** `build_payment_context` (IAP returns no checkout URL) and
    /// binds the store-side transaction id as `provider_reference` up-front.
    ///
    /// The attempt is created in `Pending` status; the caller drives it to
    /// `Succeeded` via `complete_succeeded_payment_attempt` inside the
    /// fulfillment transaction (which for Google also performs the in-tx
    pub async fn create_iap_payment_attempt(
        &self,
        input: CreateIapAttemptInput,
    ) -> PurchaseResult<PaymentAttempt> {
        let target = self
            .resolve_target(
                &input.realm_id,
                input.user_id,
                &input.target_type.to_string(),
                input.target_id,
                &input.payment_provider,
            )
            .await?;

        // IAP amount semantics: Apple/Google are the merchant-of-record, so
        // the store-side price is not authoritative for Herald's
        // `payment_attempts` row (the column exists for the Stripe/Creem
        // checkout flow). The `payment_attempts.amount CHECK(amount > 0)`
        // constraint, however, rejects 0. When the entitlement mapping has no
        // optional / manually entered), `resolve_target` returns amount=0; we
        // coerce to a sentinel `1` so the row satisfies the CHECK without
        // pretending to know the real store price. A non-zero mapping price is
        // still passed through verbatim when present.
        let amount = if target.amount > 0 { target.amount } else { 1 };

        let (attempt, _) = self
            .payment_attempt_service
            .create_payment_attempt(
                CreatePaymentAttemptInput {
                    realm_id: input.realm_id,
                    user_id: input.user_id,
                    payment_provider: input.payment_provider,
                    target_type: target.target_type.to_string(),
                    target_id: target.target_id,
                    billing_type: target.billing_type.clone().ok_or_else(|| {
                        CoreError::BillingError(format!(
                            "Entitlement mapping '{}' has no billing_type set",
                            target.target_id
                        ))
                    })?,
                    amount,
                    currency: target.currency.clone(),
                    // Bind the store-side transaction id up-front (Apple
                    // originalTransactionId / Google purchaseToken). Unlike
                    // Stripe/Creem, IAP has no intermediate checkout session.
                    provider_reference: Some(input.provider_reference),
                    metadata: input.metadata,
                    is_one_time_role: target.is_one_time_role,
                },
                PaymentContext {
                    stripe_checkout_url: None,
                    creem_checkout_url: None,
                    client_secret: None,
                    wechat_code_url: None,
                    wechat_jsapi_params: None,
                },
            )
            .await?;

        Ok(attempt)
    }

    /// Fulfill a payment attempt based on billing type from entitlement mapping.
    /// When `billing_type_override` is provided, it takes precedence.
    /// When absent, resolves from the entitlement mapping — returns an error if
    /// the mapping is missing or has no billing_type set.
    pub async fn fulfill_payment_attempt(
        &self,
        attempt: PaymentAttempt,
        provider_transaction_id: String,
        _completed_at: chrono::DateTime<chrono::Utc>,
        billing_type_override: Option<BillingType>,
    ) -> Result<FulfillmentResult, CoreError> {
        let billing_type = if let Some(bt) = billing_type_override {
            bt
        } else {
            let mapping = self
                .billing_repository
                .find_entitlement_mapping_by_id(attempt.target_id)
                .await?
                .filter(|m| m.realm_id == attempt.realm_id)
                .ok_or_else(|| {
                    CoreError::BillingError(format!(
                        "Entitlement mapping '{}' not found for realm '{}'",
                        attempt.target_id, attempt.realm_id
                    ))
                })?;

            mapping.billing_type.ok_or_else(|| {
                CoreError::BillingError(format!(
                    "Entitlement mapping '{}' has no billing_type set",
                    attempt.target_id
                ))
            })?
        };

        match billing_type {
            BillingType::OneTime => {
                self.fulfillment_service
                    .fulfill_one_time_purchase(&attempt, provider_transaction_id)
                    .await
            }
            BillingType::Recurring => {
                self.fulfillment_service
                    .fulfill_subscription_purchase(&attempt, provider_transaction_id)
                    .await
            }
            // Subscription with a fixed service period that does not auto-renew;
            // `service_duration_days` from the mapping drives current_period_end.
            BillingType::NonRenewing => {
                self.fulfillment_service
                    .fulfill_non_renewing_purchase(&attempt, provider_transaction_id)
                    .await
            }
        }
    }

    pub async fn complete_succeeded_payment_attempt(
        &self,
        input: CompletePaymentAttemptInput,
    ) -> PurchaseResult<FulfillmentResult> {
        self.validate_completion_source(&input.source)?;

        let attempt_for_realm = self
            .payment_attempt_service
            .get_payment_attempt_by_id_only(input.attempt_id)
            .await?;

        // Realm binding: provider events carry the attempt id in
        // client-influenced metadata, and the lookup above is deliberately
        // realm-free. An event signed for realm A must never complete an
        // attempt belonging to realm B.
        if let Some(expected_realm_id) = &input.expected_realm_id
            && &attempt_for_realm.realm_id != expected_realm_id
        {
            return Err(CoreError::Forbidden(format!(
                "payment attempt {} does not belong to realm {}",
                input.attempt_id, expected_realm_id
            )));
        }

        let marked_attempt = self
            .payment_attempt_service
            .mark_payment_succeeded(
                &attempt_for_realm.realm_id,
                input.attempt_id,
                input.provider_status,
                input.provider_transaction_id.clone(),
                input.completed_at,
            )
            .await?;

        self.fulfill_payment_attempt(
            marked_attempt,
            input.provider_transaction_id,
            input.completed_at,
            input.billing_type_override,
        )
        .await
    }

    async fn resolve_target(
        &self,
        realm_id: &str,
        user_id: Uuid,
        target_type: &str,
        target_id: Uuid,
        payment_provider: &str,
    ) -> PurchaseResult<PurchaseTargetSnapshot> {
        let parsed_target_type = target_type.parse::<PurchasableTarget>()?;

        // All purchasable targets are now EntitlementMapping
        let mapping = self
            .billing_repository
            .find_entitlement_mapping_by_id(target_id)
            .await?
            .ok_or(CoreError::NotFound)?;

        if mapping.realm_id != realm_id || mapping.payment_provider != payment_provider {
            return Err(CoreError::Conflict(format!(
                "No entitlement mapping found for provider '{payment_provider}' target '{}' in realm '{}'",
                target_id, realm_id
            )));
        }

        if !mapping.enabled {
            return Err(CoreError::Conflict(format!(
                "Entitlement mapping for provider '{payment_provider}' product '{}' is disabled",
                target_id
            )));
        }

        // `billing_type=one_time` + non-empty `granted_role_ids` combo is
        // one-per-user. Points packages and subscriptions remain
        // repeatable/renewable. A user who already owns it — holds any of the
        // granted roles from a `payment` source, OR has a succeeded attempt for
        // this target — is blocked at purchase creation with a distinguishable
        // `already_owned:<entitlement_key>` conflict (parsed by the API handler
        // into a structured 409 body).
        if mapping.billing_type == Some(BillingType::OneTime)
            && !mapping.granted_role_ids.is_empty()
        {
            let has_role = self
                .user_role_repository
                .user_has_any_role(realm_id, user_id, &mapping.granted_role_ids)
                .await
                .map_err(CoreError::from)?;
            let has_attempt = self
                .payment_attempt_repository
                .has_succeeded_attempt(user_id, mapping.id)
                .await?;
            if has_role || has_attempt {
                return Err(CoreError::already_owned(&mapping.entitlement_key));
            }
        }

        // Price resolution is provider-scoped. Stripe is the only provider
        // whose mapping price drives the charge (Checkout line price), so a
        // Stripe row without readable price/currency is a data anomaly and
        // fails loud (422) instead of fabricating an amount/currency.
        // Store/provider-priced flows (apple/google/wechat, creem product
        // pricing) legitimately map without price info — e.g.
        // `create_iap_payment_attempt` coerces amount 0→1 as a sentinel — and
        // keep the historical zero-amount snapshot. The stored currency code
        // passes through as-is (Stripe stores lowercase like "usd").
        let stripe_priced = mapping.payment_provider == "stripe";
        let (amount, currency, title) =
            match mapping.provider_product_info.as_ref().and_then(|info| {
                let price = info.get("price")?.as_i64()?;
                let curr = info.get("currency")?.as_str()?.to_string();
                let name = info
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&mapping.entitlement_key)
                    .to_string();
                Some((price, curr, name))
            }) {
                Some(triple) => triple,
                None if stripe_priced => {
                    return Err(CoreError::PriceInfoMissing {
                        entitlement_key: mapping.entitlement_key.clone(),
                    });
                }
                None => (0, "usd".to_string(), mapping.entitlement_key.clone()),
            };

        Ok(PurchaseTargetSnapshot {
            target_type: parsed_target_type,
            target_id,
            amount,
            currency,
            title,
            provider_external_product_id: Some(mapping.external_product_id.clone()),
            provider_external_price_id: mapping.external_price_id.clone(),
            billing_period: mapping.billing_period.clone(),
            // one_time + non-empty granted_role_ids drives the anti-repeat guard.
            is_one_time_role: mapping.billing_type == Some(BillingType::OneTime)
                && !mapping.granted_role_ids.is_empty(),
            billing_type: mapping.billing_type,
        })
    }

    async fn build_payment_context(
        &self,
        realm_id: &str,
        user_id: Uuid,
        target_type: &str,
        target_id: Uuid,
        payment_provider: &str,
        target: &PurchaseTargetSnapshot,
        attempt_id: Uuid,
        flow: PaymentFlow,
        user_email: Option<&str>,
        payment_scene: Option<&str>,
        openid: Option<&str>,
        attempt_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> PurchaseResult<(Option<String>, PaymentContext)> {
        validate_flow_combination(payment_provider, target.billing_type.clone(), flow)?;
        match payment_provider {
            "creem" => {
                self.build_creem_payment_context(
                    realm_id,
                    user_id,
                    target_type,
                    target_id,
                    target,
                    attempt_id,
                    user_email,
                )
                .await
            }
            "stripe" => {
                self.build_stripe_payment_context(
                    realm_id,
                    user_id,
                    target_type,
                    target_id,
                    target,
                    attempt_id,
                    flow,
                    user_email,
                )
                .await
            }
            "wechat" => {
                self.build_wechat_payment_context(
                    realm_id,
                    target,
                    payment_scene,
                    openid,
                    attempt_expires_at,
                )
                .await
            }
            _ => Err(CoreError::BadRequest(
                "Unsupported payment provider".to_string(),
            )),
        }
    }

    async fn build_creem_payment_context(
        &self,
        realm_id: &str,
        user_id: Uuid,
        target_type: &str,
        target_id: Uuid,
        target: &PurchaseTargetSnapshot,
        attempt_id: Uuid,
        user_email: Option<&str>,
    ) -> PurchaseResult<(Option<String>, PaymentContext)> {
        let product_id = target.provider_external_product_id.clone().ok_or_else(|| {
            CoreError::Conflict("Creem product mapping missing external_product_id".into())
        })?;
        let client = self.get_creem_client_for_realm(realm_id).await?;
        let metadata = build_herald_metadata(realm_id, user_id, target_type, target_id, attempt_id);

        let session = client
            .create_checkout_session(&CreemCreateCheckoutRequest {
                product_id,
                // Redirect the user back to the purchase page (this is a UX
                // bounce only — payment status is confirmed via webhook). The
                // `public_base_url` field holds the frontend base URL (set from
                // `config.frontend.url`), and `attemptId` lets the page resume
                // its processing-step polling. Creem has no cancel_url.
                success_url: Some(format!(
                    "{}/{}/user/purchase-points?attemptId={}&status=success",
                    self.public_base_url, realm_id, attempt_id
                )),
                customer: herald_infra_creem::CreemCheckoutCustomer {
                    email: Some(
                        user_email
                            .expect("validated in prepare_payment_attempt")
                            .to_owned(),
                    ),
                },
                metadata: Some(metadata),
            })
            .await
            .map_err(|e| {
                CoreError::InternalServerError(format!(
                    "Failed to create Creem checkout session: {e}"
                ))
            })?;

        Ok((
            Some(session.id),
            PaymentContext {
                stripe_checkout_url: None,
                creem_checkout_url: Some(session.checkout_url),
                client_secret: None,
                wechat_code_url: None,
                wechat_jsapi_params: None,
            },
        ))
    }

    async fn build_stripe_payment_context(
        &self,
        realm_id: &str,
        user_id: Uuid,
        target_type: &str,
        target_id: Uuid,
        target: &PurchaseTargetSnapshot,
        attempt_id: Uuid,
        flow: PaymentFlow,
        user_email: Option<&str>,
    ) -> PurchaseResult<(Option<String>, PaymentContext)> {
        let client = self.get_stripe_client_for_realm(realm_id).await?;

        let metadata = build_herald_metadata(realm_id, user_id, target_type, target_id, attempt_id);

        match flow {
            // Mobile wallet flow: return a raw PaymentIntent client_secret;
            // the native Stripe SDK (PassKit / Google Pay) confirms
            // client-side. The metadata already carries `attemptId`, so the
            // existing `payment_intent.succeeded` webhook dispatch keys back
            // to the attempt — no new webhook code.
            PaymentFlow::PaymentIntent => {
                let intent = client
                    .create_payment_intent(&CreatePaymentIntentRequest {
                        amount: target.amount,
                        currency: target.currency.clone(),
                        receipt_email: user_email.map(|e| e.to_string()),
                        metadata,
                    })
                    .await
                    .map_err(|e| {
                        CoreError::InternalServerError(format!(
                            "Failed to create Stripe payment intent: {e}"
                        ))
                    })?;

                Ok((
                    Some(intent.id),
                    PaymentContext {
                        stripe_checkout_url: None,
                        creem_checkout_url: None,
                        client_secret: Some(intent.client_secret),
                        wechat_code_url: None,
                        wechat_jsapi_params: None,
                    },
                ))
            }
            PaymentFlow::Hosted => {
                let mode = match target.billing_type {
                    Some(BillingType::OneTime) => Some("payment".to_string()),
                    _ => None, // defaults to "subscription" in the client
                };

                let session = client
                    .create_checkout_session(&StripeCreateCheckoutRequest {
                        client_app_id: target_id,
                        mapping_id: target_id,
                        user_id: Some(user_id),
                        customer_email: Some(
                            user_email
                                .expect("validated in prepare_payment_attempt")
                                .to_owned(),
                        ),
                        // Redirect the user back to the purchase page (UX bounce only;
                        // payment status is confirmed via webhook). `public_base_url`
                        // holds the frontend base URL (set from `config.frontend.url`),
                        // and `attemptId` lets the page resume processing-step polling.
                        success_url: format!(
                            "{}/{}/user/purchase-points?attemptId={}&status=success",
                            self.public_base_url, realm_id, attempt_id
                        ),
                        cancel_url: format!(
                            "{}/{}/user/purchase-points?attemptId={}&status=cancel",
                            self.public_base_url, realm_id, attempt_id
                        ),
                        billing_period: target
                            .billing_period
                            .clone()
                            .unwrap_or_else(|| "monthly".to_string()),
                        trial_days: None,
                        price_amount: target.amount,
                        currency: target.currency.clone(),
                        plan_name: target.title.clone(),
                        // Reference the real Stripe Price when the mapping carries one;
                        // None falls back to price_data in the client.
                        price_id: target.provider_external_price_id.clone(),
                        realm_id: realm_id.to_string(),
                        webhook_url: Some(format!(
                            "{}/api/third/pay/{}/stripe/webhooks",
                            self.public_base_url, realm_id
                        )),
                        metadata: Some(metadata),
                        mode,
                    })
                    .await
                    .map_err(|e| {
                        CoreError::InternalServerError(format!(
                            "Failed to create Stripe checkout session: {e}"
                        ))
                    })?;

                Ok((
                    Some(session.id),
                    PaymentContext {
                        stripe_checkout_url: Some(session.url),
                        creem_checkout_url: None,
                        // A checkout session exposes no client_secret; the
                        // old fallback stuffed the PI id (or session id)
                        // here, which is not a secret and misled mobile
                        // integrators.
                        client_secret: None,
                        wechat_code_url: None,
                        wechat_jsapi_params: None,
                    },
                ))
            }
        }
    }

    async fn build_wechat_payment_context(
        &self,
        realm_id: &str,
        target: &PurchaseTargetSnapshot,
        payment_scene: Option<&str>,
        openid: Option<&str>,
        attempt_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> PurchaseResult<(Option<String>, PaymentContext)> {
        use herald_domain::payment_attempt::entities::WechatJsapiParams;
        use herald_infra_wechatpay::{CreateOrderResult, CreateOrderScene, generate_out_trade_no};

        let client = crate::wechatpay::get_wechat_client_for_realm(&self.pool, realm_id).await?;
        let out_trade_no = generate_out_trade_no(realm_id);

        // Unknown scene values are rejected, not silently coerced to Native —
        // a typo like "jsapi " would otherwise produce a QR-code order that
        // can never be paid inside WeChat (same fail-loud stance as the
        // wallet `flow` allow-list).
        let scene = match payment_scene.unwrap_or("native") {
            "native" => CreateOrderScene::Native,
            "jsapi" => {
                let openid = openid.ok_or_else(|| {
                    CoreError::BadRequest("openid is required for WeChat jsapi payment".to_string())
                })?;
                CreateOrderScene::Jsapi {
                    openid: openid.to_string(),
                }
            }
            other => {
                return Err(CoreError::BadRequest(format!(
                    "unsupported WeChat payment_scene '{other}': expected 'native' or 'jsapi'"
                )));
            }
        };

        let amount_fen = if target.amount > 0 {
            target.amount
        } else {
            return Err(CoreError::BadRequest(
                "WeChat order requires a positive amount".to_string(),
            ));
        };

        let description = if target.title.is_empty() {
            "Herald purchase".to_string()
        } else {
            target.title.clone()
        };

        let result = client
            .create_order(
                scene,
                &out_trade_no,
                &description,
                amount_fen,
                &target.currency,
                attempt_expires_at,
            )
            .await
            .map_err(|e| {
                CoreError::InternalServerError(format!("WeChat create_order failed: {e}"))
            })?;

        let (wechat_code_url, wechat_jsapi_params) = match result {
            CreateOrderResult::Native { code_url } => (Some(code_url), None),
            CreateOrderResult::Jsapi(params) => (
                None,
                Some(WechatJsapiParams {
                    app_id: params.app_id,
                    time_stamp: params.time_stamp,
                    nonce_str: params.nonce_str,
                    package: params.package,
                    sign_type: params.sign_type,
                    pay_sign: params.pay_sign,
                }),
            ),
        };

        Ok((
            Some(out_trade_no),
            PaymentContext {
                stripe_checkout_url: None,
                creem_checkout_url: None,
                client_secret: None,
                wechat_code_url,
                wechat_jsapi_params,
            },
        ))
    }

    pub async fn get_creem_client_for_realm(&self, realm_id: &str) -> PurchaseResult<CreemClient> {
        let api_key = sqlx::query_scalar::<_, String>(
            "SELECT config_value
             FROM realm_config
             WHERE realm_id = $1 AND config_type = 'creem' AND config_key = 'api_key' AND enabled = true
             LIMIT 1",
        )
        .bind(realm_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            CoreError::InternalServerError(format!("Creem not configured for realm: {realm_id}"))
        })?;

        let timeout = sqlx::query_scalar::<_, String>(
            "SELECT config_value
             FROM realm_config
             WHERE realm_id = $1 AND config_type = 'creem' AND config_key = 'timeout' AND enabled = true
             LIMIT 1",
        )
        .bind(realm_id)
        .fetch_optional(&self.pool)
        .await?
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);

        CreemClient::new(api_key, timeout)
    }

    pub async fn get_stripe_client_for_realm(
        &self,
        realm_id: &str,
    ) -> PurchaseResult<StripeClient> {
        let api_key = sqlx::query_scalar::<_, String>(
            "SELECT config_value
             FROM realm_config
             WHERE realm_id = $1 AND config_type = 'stripe' AND config_key = 'api_key' AND enabled = true
             LIMIT 1",
        )
        .bind(realm_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            CoreError::InternalServerError(format!("Stripe not configured for realm: {realm_id}"))
        })?;

        let timeout = sqlx::query_scalar::<_, String>(
            "SELECT config_value
             FROM realm_config
             WHERE realm_id = $1 AND config_type = 'stripe' AND config_key = 'timeout' AND enabled = true
             LIMIT 1",
        )
        .bind(realm_id)
        .fetch_optional(&self.pool)
        .await?
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);

        StripeClient::new(api_key, timeout)
    }

    fn validate_completion_source(&self, source: &PaymentCompletionSource) -> PurchaseResult<()> {
        match source {
            PaymentCompletionSource::InternalApi => Ok(()),
            // IAP providers (apple / google) reach this path via the IAP
            // receipt handler and Apple SSV V2 webhook
            // (`shared_fulfillment::fulfill_provider_event`), in addition to
            // boundary #2: the short names `apple` / `google` flow through
            // every `payment_provider` match arm alongside `stripe` / `creem`.
            PaymentCompletionSource::ProviderWebhook { provider }
                if matches!(
                    provider.as_str(),
                    "stripe" | "creem" | "apple" | "google" | "wechat"
                ) =>
            {
                Ok(())
            }
            PaymentCompletionSource::ProviderWebhook { provider } => Err(CoreError::BadRequest(
                format!("Unsupported payment completion source provider: {provider}"),
            )),
        }
    }
}

#[cfg(test)]
mod flow_combination_tests {
    use super::validate_flow_combination;
    use herald_domain::billing::entities::BillingType;
    use herald_domain::common::entities::app_errors::CoreError;
    use herald_domain::purchase::services::PaymentFlow;

    // The combination gate protects the mobile wallet escape hatch: a raw
    // PaymentIntent exists only on Stripe's one-time rails. Anything else
    // must 400 before a provider call is made — silently accepting would
    // strand non-stripe/recurring buyers with a client_secret no wallet
    // SDK can confirm.
    #[test]
    fn stripe_one_time_payment_intent_is_ok() {
        assert!(
            validate_flow_combination(
                "stripe",
                Some(BillingType::OneTime),
                PaymentFlow::PaymentIntent
            )
            .is_ok()
        );
    }

    #[test]
    fn non_stripe_payment_intent_is_rejected() {
        let err = validate_flow_combination(
            "creem",
            Some(BillingType::OneTime),
            PaymentFlow::PaymentIntent,
        )
        .unwrap_err();
        match err {
            CoreError::BadRequest(msg) => {
                assert!(
                    msg.contains("only supported for stripe"),
                    "unexpected: {msg}"
                )
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    #[test]
    fn recurring_payment_intent_is_rejected() {
        let err = validate_flow_combination(
            "stripe",
            Some(BillingType::Recurring),
            PaymentFlow::PaymentIntent,
        )
        .unwrap_err();
        match err {
            CoreError::BadRequest(msg) => {
                assert!(msg.contains("one-time"), "unexpected: {msg}")
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    #[test]
    fn hosted_flow_is_always_ok() {
        for (provider, billing_type) in [
            ("stripe", Some(BillingType::OneTime)),
            ("stripe", Some(BillingType::Recurring)),
            ("stripe", None),
            ("creem", Some(BillingType::OneTime)),
            ("wechat", None),
        ] {
            assert!(
                validate_flow_combination(provider, billing_type.clone(), PaymentFlow::Hosted)
                    .is_ok(),
                "hosted must stay valid for {provider}/{billing_type:?}"
            );
        }
    }
}
