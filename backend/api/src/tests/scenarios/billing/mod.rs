// =============================================================================
// Billing Security Tests
// =============================================================================
//
// User Story: docs/user-stories/06-billing-user-stories.md
//
// =============================================================================

pub mod entitlement_mapping_crud_scenarios;

// WeChat manual-price mapping scenario tests (wechat-support PRD: no hosted
// catalog, price configured by hand in the mapping)
pub mod wechat_entitlement_mapping_create_scenarios;

// Paywall M1 role-grant config dimension scenario tests (support-paywall)
// User Story: US-PW-001 (entitlement→role mapping configuration)
//         §4.3.2 (UUID[] column), §5.2 (three-state + RoleNotInRealm), §6.1 M1, §6.3
pub mod paywall_m1_config_dimension_scenarios;

// Paywall W1 + M2 payment-driven role grant + idempotency scenario tests
// (support-paywall)
// User Story: US-PW-002 (W1 one-time no-points no 500), US-PW-003 (payment grant
//             + source traceability + idempotency; manual grants untouched)
//         expires_at + GrantRoleOutcome::AlreadyExists), §6.1 W1+M2, §6.3
pub mod paywall_w1_m2_grant_scenarios;

// Paywall M3 one-time+role anti-repeat scenario tests (support-paywall)
// User Story: US-PW-004 (one-time+role one-per-user; points repeatable;
//             frontend grantsRole/alreadyOwned signals; concurrent invariant)
//         PurchaseOptionView.grantsRole/alreadyOwned), §4.3.2 (unique-constraint
//         backstop), §5.4 (ownership predicate OR), §6.1 M3, §6.3
pub mod paywall_m3_anti_repeat_scenarios;

// Paywall M4 subscription-class role revoke + out-of-order renewal +
// processed=false sweep scenario tests (support-paywall)
// User Story: US-PW-005 (subscriptions canceled/expired/refunded auto-revoke the
//             payment-granted role, eventually-consistent and idempotent)
//         §5.5 (convergence-point mount; RevokeRoleOutcome idempotency;
//         out-of-order renewal upsert; one-time refunds don't route through
//         handle_subscription_cancel), §5.5.1 (PaymentEventRetryJob sweep + backoff),
//         §6.1 M4, §6.3 (source='manual' + one-time refund decoupled regression),
//         §7 P0 (kill-criteria: never permanently miss a revoke)
pub mod paywall_m4_revoke_sweep_scenarios;

// Provider product sync metadata scenario tests (sync-payment feature)
// User Story: US-BL-SYNC-001
// Covers: Stripe product/price metadata propagation into provider_product_info
//         JSONB; re-sync takes the latest metadata.
pub mod provider_product_sync_scenarios;

pub mod webhook_entitlement_scenarios;

// Creem webhook one-time dispatch scenarios
// User Story: US-PA-003, US-PU-006
pub mod creem_webhook_one_time_scenarios;

// One-time purchase fulfillment scenarios
// User Story: US-PU-006, US-PA-001, US-PA-003
pub mod one_time_fulfillment_scenarios;

pub mod entitlement_subscription_scenarios;

// Stripe webhook mode dispatch scenarios (payment vs subscription)
// User Story: US-PA-003, US-PU-006
pub mod stripe_webhook_mode_scenarios;

pub mod subscription_points_entitlement_scenarios;

pub mod invoice_admin_scenarios;

pub mod invoice_pdf_scenarios;

pub mod invoice_provider_policy_scenarios;

pub mod invoice_external_sync_scenarios;

pub mod invoice_user_scenarios;

pub mod invoice_overdue_scenarios;

// One-time API endpoint scenario tests
// User Story: US-EM-001, US-PU-006, US-PU-007, US-PA-001
pub mod one_time_api_scenarios;

// Stripe webhook patch scenario tests (new webhook event handlers)
// Covers: checkout.session.expired/async_*, dispute.created/closed,
//         subscription paused/resumed/updated/deleted
pub mod stripe_webhook_patch_scenarios;

// Creem webhook patch scenario tests (new webhook event handlers)
// Covers: subscription lifecycle events, dispute.created
pub mod creem_webhook_patch_scenarios;

// Webhook compensation job scenario tests
// User Story: US-WC-001, US-WC-002
pub mod webhook_compensation_scenarios;

// Async payment points strategy scenario tests
// User Story: US-AP-001, US-AP-002
pub mod async_payment_points_strategy_scenarios;

// Async payment revocation scenario tests
// User Story: US-AP-002 (idempotency), US-AP-003 (revocation), US-AP-004 (debt)
pub mod async_payment_revocation_scenarios;

// Refund incremental revocation scenario tests: per-refund incremental
// points revocation + refund-id idempotency, the cumulative-full-refund
// gate for one-time role revocation, dispute retention, and the Stripe
// missing-refunds fail-loud contract.
pub mod refund_incremental_scenarios;

pub mod payment_attempt_status_guard_scenarios;

// Webhook grant idempotency scenario tests
// Covers: Duplicate webhook delivery must not cause double point grants
pub mod webhook_grant_idempotency_scenarios;

// Manual Credit Note scenario tests
// User Story: US-IF-010 (admin records offline refund), US-IF-008 (refund visibility)
pub mod credit_note_manual_scenarios;

// Stripe credit_note.created Webhook scenario tests
// User Story: US-IF-007 (Stripe credit note sync), US-IF-008 (refund visibility)
pub mod credit_note_stripe_webhook_scenarios;

// Invoice refund field query scenario tests
// User Story: US-IF-008 (admin refund visibility), US-IF-009 (user refund annotation)
pub mod invoice_refund_query_scenarios;

// Feature-availability invoice eligibility scenario tests
// User Story: docs/user-stories/billing/invoice-fallback.md
// Covers: P0-2 Phase A -- realm-level invoice eligibility via feature-availability
//         so frontend can gate Create/Apply invoice buttons before submit.
pub mod feature_availability_invoice_eligibility_scenarios;

// Invoice per-resource apply-eligibility scenario tests
// User Story: docs/user-stories/billing/invoice-fallback.md
// Covers: P0-2 Phase B -- read-only, context-level apply-eligibility verdict
//         for a specific payment_attempt/subscription, so frontend can gate the
//         Apply Invoice button on that resource before submit.
pub mod invoice_apply_eligibility_scenarios;

// Creem subscription renewal scenario tests (payment-invoice-mapping)
// User Story: US-PM-001 (renewal payment_attempt), US-PM-002 (renewal invoice)
pub mod creem_subscription_renewal_scenarios;

// Stripe subscription renewal scenario tests (payment-invoice-mapping)
// User Story: US-PM-001 (renewal payment_attempt), US-PM-003 (invoice attribution)
//         §6.1 (Stripe renewal cases), §6.3 (hosted_url/pdf_url non-regression)
pub mod stripe_subscription_renewal_scenarios;

// External invoice attribution + regression scenario tests (payment-invoice-mapping)
// User Story: US-PM-003 (external invoice attribution)
//         §6.1 (attribution + regression cases), §6.3 (COALESCE non-clobber)
pub mod external_invoice_attribution_scenarios;

// =============================================================================
// IAP (App Store / Google Play) scenario tests (support-iap)
// =============================================================================

// IAP receipt submission scenario tests (Apple jwsRepresentation + Google
// purchaseToken → verify → resolve mapping → create attempt → fulfil +
// Google ack/consume; §6.3 ack-failure rollback).
// User Story: US-IAP-003 (client credential submission triggers fulfilment)
//         §6.1, §6.3 (Google ack-failure rollback regression).
pub mod iap_receipt_scenarios;

// Apple SSV V2 webhook scenario tests (always-200 receiver; JWS verification
// is the trust root; §6.3 tampered-leaf regression).
// User Story: US-IAP-004 (Apple server notifications drive lifecycle + catch-up)
pub mod apple_webhook_scenarios;

// WeChat Pay v3 webhook scenario tests (verify → decrypt → idempotency →
// amount-guard → unified fulfilment; OneTime + NonRenewing; openid requirement).
// User Story: docs/user-stories/billing/wechat-support.md (US-WP-002 / US-WP-003 / US-WP-004)
pub mod wechat_webhook_scenarios;

// IAP entitlement mapping create scenario tests (POST /entitlement-mappings;
// 201 / 409 duplicate / 403 billing.manage / 403 credit fields w/o points.manage).
// User Story: US-IAP-002 (build IAP product → entitlement mapping)
pub mod iap_entitlement_mapping_create_scenarios;

// IAP reconciliation job scenario tests (IapReconciliationJob::run with a
// MockProcessor; failure isolation structural contract).
// User Story: US-IAP-006 (scheduled reconciliation)
pub mod iap_reconciliation_scenarios;

// IAP provider CHECK constraint migration regression (post-migration state
// User Story: n/a (DB regression)
pub mod iap_provider_check_migration_scenarios;

// =============================================================================
// =============================================================================

// Pay-model fulfillment scenario tests: buyout IAP → permanent role (Google
// ack-only), restore-purchase idempotent re-grant, non-renewing fixed-duration
// subscription (billing_type snapshot + cancel_at), mapping 400 contracts
// (US-PM-002), migration 0011 DB regression, Apple non-renewing no-local-expiry
// accepted risk.
// User Story: US-PM-004 (buyout IAP), US-PM-005 (restore idempotent),
//             US-PM-006 (non-renewing fixed-duration + repurchase not blocked),
//             US-PM-002 scenario 2 (non_renewing 400 contract),
//             US-PM-009 (Apple non-renewing accepted risk).
//         §5.2 (fulfill_non_renewing_purchase), §5.4 (Google ack-only),
pub mod pay_model_fulfillment_scenarios;

// Multiple-currency resolution scenario tests: api-ext currency set +
// by-currency default-price resolution (hit/404/409), purchase creation
// fail-loud on price-less mapping rows.
// User Story: docs/user-stories/billing/payment-attempt.md (US-PA-001),
//             docs/user-stories/billing/entitlement-mapping.md (US-EM-008)
pub mod multiple_currency_scenarios;
