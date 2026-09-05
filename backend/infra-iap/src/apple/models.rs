//! Apple type re-exports.
//!
//! The decoded payloads (`JWSTransactionDecodedPayload`,
//! `ResponseBodyV2DecodedPayload`) and the request builders
//! (`NotificationHistoryRequest`, `TransactionHistoryRequest`) are re-exported
//! to depend on the upstream crate directly.
//!
//! We intentionally do not wrap these in new structs: they are stable,
//! `Option`-heavy payloads whose surface area is exactly what Apple documents,
//! and re-wrapping them would just hide fields the reconciliation job needs.

pub use app_store_server_library::primitives::auto_renew_status::AutoRenewStatus;
pub use app_store_server_library::primitives::environment::Environment;
pub use app_store_server_library::primitives::history_response::HistoryResponse;
pub use app_store_server_library::primitives::jws_renewal_info_decoded_payload::JWSRenewalInfoDecodedPayload;
pub use app_store_server_library::primitives::jws_transaction_decoded_payload::JWSTransactionDecodedPayload;
pub use app_store_server_library::primitives::notification_history_request::NotificationHistoryRequest;
pub use app_store_server_library::primitives::notification_history_response::NotificationHistoryResponse;
pub use app_store_server_library::primitives::notification_type_v2::NotificationTypeV2;
pub use app_store_server_library::primitives::product_type::ProductType;
pub use app_store_server_library::primitives::response_body_v2_decoded_payload::ResponseBodyV2DecodedPayload;
pub use app_store_server_library::primitives::status::Status;
pub use app_store_server_library::primitives::status_response::StatusResponse;
pub use app_store_server_library::primitives::subtype::Subtype;
pub use app_store_server_library::primitives::transaction_history_request::TransactionHistoryRequest;
