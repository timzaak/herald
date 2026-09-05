// Herald API Points Module
// Points, wallets, transactions, registration rules

pub mod auth_middleware;
pub mod grant;
pub mod internal_quota;
pub mod registration_rules;
pub mod routes;
pub mod transactions;
pub mod types;
pub mod wallets;

/// OpenAPI specification for points module
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::wallets::list_wallets,
        crate::wallets::list_user_wallets,
        crate::wallets::get_wallet,
        crate::wallets::update_wallet_status,
        crate::transactions::list_transactions,
        crate::transactions::list_user_transactions,
        crate::registration_rules::get_registration_rules,
        crate::registration_rules::upsert_registration_rules,
        crate::grant::grant_points,
        crate::internal_quota::grant_quota_entitlement,
        crate::internal_quota::revoke_quota_entitlement,
    ),
    components(schemas(
        crate::types::ConsumePointsRequest,
        crate::types::ConsumePointsResponse,
        crate::types::ListTransactionsQuery,
        crate::types::ListWalletsQuery,
        crate::types::PointsWalletResponse,
        crate::types::UpdateWalletStatusRequest,
        crate::types::PointsBalanceResponse,
        crate::types::PointsTransactionResponse,
        crate::types::BalancesByType,
        crate::types::WalletByBucketResponse,
        crate::types::ListWalletsByBucketResponse,
        crate::types::RegistrationRuleResponse,
        crate::types::RegistrationRuleWrite,
        crate::types::RegistrationRulesResponse,
        crate::types::UpsertRegistrationRulesRequest,
        herald_api_base::application::http::server::api_entities::DistributionRuleErrorResponse,
        crate::types::GrantPointsRequest,
        crate::types::GrantPointsResponse,
        crate::internal_quota::GrantQuotaEntitlementRequest,
        crate::internal_quota::GrantQuotaEntitlementResponse,
        crate::internal_quota::RevokeQuotaEntitlementRequest,
        crate::internal_quota::RevokeQuotaEntitlementResponse,
        crate::internal_quota::InternalQuotaWindowInput,
    ))
)]
pub struct ApiDoc;

// Re-export routes for use by the main api crate
pub use routes::internal_public_routes;
pub use routes::points_router;
// Re-export auth_middleware for server routing
pub use auth_middleware::flexible_auth_middleware;
