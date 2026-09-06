pub mod compensation;
pub(crate) mod webhook_common;
pub(crate) mod webhook_subscription_helpers;
mod webhooks;

pub mod credit_bucket_handlers;
pub mod entitlement_mapping_handlers;
pub mod feature_availability;
pub mod handlers;
pub mod handlers_history;
pub mod iap_handlers;
pub mod invoice_eligibility;
pub mod invoice_handlers;
pub mod invoice_types;
mod payment_email;
pub mod provider_common_types;
pub mod provider_handlers;
pub mod purchase_handlers;
pub mod routes;
pub mod shared_fulfillment;
pub mod stripe_webhook_handlers;
pub mod types;
pub mod types_history;
pub mod webhook_handlers;
pub mod wechat_webhook_handlers;

/// OpenAPI specification for billing module
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::credit_bucket_handlers::list_credit_buckets_handler,
        crate::credit_bucket_handlers::get_credit_bucket_handler,
        crate::credit_bucket_handlers::create_credit_bucket_handler,
        crate::credit_bucket_handlers::update_credit_bucket_handler,
        crate::credit_bucket_handlers::delete_credit_bucket_handler,
        crate::credit_bucket_handlers::get_bucket_overview_handler,
        crate::entitlement_mapping_handlers::list_entitlement_mappings,
        crate::entitlement_mapping_handlers::get_entitlement_mapping,
        crate::entitlement_mapping_handlers::update_entitlement_mapping,
        crate::entitlement_mapping_handlers::sync_provider_products,
        crate::entitlement_mapping_handlers::list_one_time_mappings,
        crate::entitlement_mapping_handlers::batch_update_entitlement_mappings,
        crate::entitlement_mapping_handlers::create_entitlement_mapping,
        crate::iap_handlers::submit_iap_receipt,
        crate::iap_handlers::handle_apple_webhook,
        crate::wechat_webhook_handlers::handle_wechat_webhook,
        crate::handlers::list_subscriptions,
        crate::handlers::get_subscription,
        crate::handlers::get_subscription_for_client_app,
        crate::handlers::cancel_subscription_for_client_app,
        crate::handlers::list_purchase_options,
        crate::handlers_history::get_subscription_history,
        crate::handlers_history::list_subscription_history,
        crate::handlers_history::get_my_subscription_history,
        crate::handlers_history::list_my_subscription_history,
        crate::feature_availability::get_feature_availability,
        crate::feature_availability::get_user_feature_availability,
        crate::provider_handlers::list_payment_providers,
        crate::purchase_handlers::create_payment_attempt,
        crate::purchase_handlers::get_payment_attempt_status,
        crate::purchase_handlers::cancel_payment_attempt,
        crate::purchase_handlers::fulfill_payment,
        crate::purchase_handlers::get_purchase_history,
        crate::purchase_handlers::get_realm_purchase_history,
        crate::invoice_handlers::get_seller_config,
        crate::invoice_handlers::upsert_seller_config,
        crate::invoice_handlers::create_invoice,
        crate::invoice_handlers::list_invoices,
        crate::invoice_handlers::list_attribution_anomalies,
        crate::invoice_handlers::get_invoice,
        crate::invoice_handlers::update_invoice,
        crate::invoice_handlers::issue_invoice,
        crate::invoice_handlers::void_invoice,
        crate::invoice_handlers::mark_paid,
        crate::invoice_handlers::create_credit_note,
        crate::invoice_handlers::apply_invoice,
        crate::invoice_handlers::list_my_invoices,
        crate::invoice_handlers::get_my_invoice_scoped,
        crate::invoice_handlers::get_invoice_apply_eligibility,
        crate::invoice_handlers::download_invoice_pdf,
        crate::invoice_handlers::download_my_invoice_pdf,
    ),
    components(schemas(
        crate::credit_bucket_handlers::BucketResponse,
        crate::credit_bucket_handlers::BucketDetailResponse,
        crate::credit_bucket_handlers::ClientAppRef,
        crate::credit_bucket_handlers::CreateCreditBucketRequest,
        crate::credit_bucket_handlers::UpdateCreditBucketRequest,
        crate::credit_bucket_handlers::BucketOverviewResponse,
        crate::credit_bucket_handlers::OverviewRowResponse,
        crate::credit_bucket_handlers::ByCreditTypeResponse,
        crate::credit_bucket_handlers::BucketKeyDuplicateErrorBody,
        crate::credit_bucket_handlers::BucketInUseErrorBody,
        crate::types::EntitlementMappingResponse,
        crate::types::EntitlementMappingListResponse,
        crate::types::PointDistributionRuleResponse,
        crate::types::PointDistributionRuleWrite,
        herald_api_base::application::http::server::api_entities::DistributionRuleErrorResponse,
        crate::types::DistributionRuleReferenceResponse,
        crate::types::EntitlementMappingQuery,
        crate::types::UpdateEntitlementMappingRequest,
        crate::types::SyncProviderRequest,
        crate::types::SyncProviderResponse,
        crate::types::PartialSyncError,
        crate::types::OneTimeMappingItem,
        crate::types::OneTimeMappingListResponse,
        crate::types::PriceMappingUpdate,
        crate::types::QuotaWindowInput,
        crate::types::EntitlementQuotaWindowResponse,
        crate::types::BatchUpdateEntitlementMappingsRequest,
        crate::types::BatchUpdateEntitlementMappingsResponse,
        crate::types::CreateEntitlementMappingRequest,
        crate::iap_handlers::IapReceiptRequest,
        crate::iap_handlers::IapReceiptResponse,
        crate::types::PurchaseOptionView,
        crate::types::PurchaseOptionListResponse,
        crate::types::SubscriptionDetailResponse,
        crate::types::SubscriptionListItemResponse,
        crate::types::SubscriptionListResponse,
        crate::types::SubscriptionListQuery,
        crate::types::CancelSubscriptionRequest,
        crate::types::CancelSubscriptionResponse,
        crate::types_history::SubscriptionHistoryEventResponse,
        crate::types_history::SubscriptionHistoryResponse,
        crate::types_history::SubscriptionHistoryListResponse,
        crate::types_history::SubscriptionHistoryEventWithUser,
        crate::types_history::SubscriptionSummary,
        crate::types_history::UserInfo,
        crate::provider_common_types::PaymentProvidersResponse,
        crate::provider_common_types::PaymentProviderInfo,
        crate::provider_common_types::ValidationErrorResponse,
        crate::provider_common_types::ValidationErrorDetail,
        crate::provider_common_types::GenericErrorResponse,
        crate::feature_availability::FeatureAvailabilityResponse,
        crate::feature_availability::UserFeatureAvailabilityResponse,
        crate::feature_availability::AdminFeatureAvailability,
        crate::feature_availability::UserFeatureAvailability,
        crate::feature_availability::FeatureAvailabilityFacts,
        crate::invoice_eligibility::InvoiceEligibilitySummary,
        crate::purchase_handlers::CreatePaymentAttemptRequest,
        crate::purchase_handlers::CreatePaymentAttemptResponse,
        crate::purchase_handlers::PaymentContextResponse,
        herald_core::domain::payment_attempt::entities::WechatJsapiParams,
        crate::purchase_handlers::PaymentAttemptStatusResponse,
        crate::purchase_handlers::FulfillmentResultResponse,
        crate::purchase_handlers::PointGrantResponse,
        crate::purchase_handlers::FulfillPaymentResponse,
        crate::purchase_handlers::PurchaseHistoryResponse,
        crate::purchase_handlers::PurchaseHistoryItem,
        crate::purchase_handlers::FulfillPaymentRequest,
        crate::invoice_types::SellerConfigRequest,
        crate::invoice_types::SellerConfigResponse,
        crate::invoice_types::LineItemRequest,
        crate::invoice_types::InvoiceLineItemResponse,
        crate::invoice_types::InvoiceHistoryResponse,
        crate::invoice_types::CreateInvoiceRequest,
        crate::invoice_types::UpdateInvoiceRequest,
        crate::invoice_types::IssueInvoiceRequest,
        crate::invoice_types::VoidInvoiceRequest,
        crate::invoice_types::MarkPaidRequest,
        crate::invoice_types::ApplyInvoiceRequest,
        crate::invoice_types::InvoiceApplyEligibilityResponse,
        crate::invoice_types::InvoiceApplyEligibilityQuery,
        crate::invoice_types::InvoiceListQuery,
        crate::invoice_types::InvoiceResponse,
        crate::invoice_types::InvoiceDetailResponse,
        crate::invoice_types::InvoiceListResponse,
        crate::invoice_types::CreateCreditNoteRequest,
        crate::invoice_types::CreditNoteResponse,
        crate::invoice_types::PaymentWithoutInvoiceResponse,
        crate::invoice_types::AttributionAnomaliesResponse,
    ))
)]
pub struct ApiDoc;

pub use routes::{billing_browser_routes, billing_public_routes, billing_routes};

pub use routes::billing_test_routes;

pub use compensation::WebhookEventProcessorImpl;
