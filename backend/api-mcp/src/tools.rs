// The nine read-only Herald MCP tools.
//
// Common structure (the three-check contract: authenticate → authorize → read):
// 1. The protocol middleware authenticated the caller (Identity in Parts)
//    against the MCP credential face — an OAuth user token bound to this
//    realm's canonical resource.
// 2. `ensure_permission` is the FIRST business statement of every admin tool
//    — the user and points services do not gate unscoped reads, so the tool
//    layer is the only RBAC defense on this surface. Self tools instead
//    verify the transport preflight marker (the scope check already ran
//    outside, before any read).
// 3. realm always comes from the credential; no tool accepts a realmId
//    argument, so cross-realm reads are structurally inexpressible (a user
//    of another realm simply reads as not_found).
//
// Field minimization is part of the contract, not style: agents may carry
// tool output into third-party models, so audit details (ip, user agent,
// trace id, details), config values, payment metadata and ledger attribution
// fields are deliberately absent from the DTOs.

use http::request::Parts;
use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;

use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{AuditEventFilters, AuditEventRepository};
use herald_core::domain::billing::BillingRepository;
use herald_core::domain::points::PointsRepository;
use herald_core::domain::points::entities::{Paginated, PointsBalance, PointsTransaction};
use herald_core::domain::points::ports::TransactionFilters;
use herald_core::domain::realm_config::RealmConfigRepository;
use herald_core::domain::user::UserRepository;
use herald_core::domain::user::ports::UserService;
use rmcp::{tool, tool_handler, tool_router};

use crate::dto;
use crate::tool_error::{
    ToolError, ensure_permission, identity_from_parts, map_core_error, map_user_lookup_error,
};

pub struct HeraldMcpService {
    state: AppState,
}

fn json_success<T: Serialize>(value: &T) -> Result<CallToolResult, rmcp::ErrorData> {
    let text = serde_json::to_string(value).map_err(|e| {
        tracing::error!("Failed to serialize MCP tool output: {e}");
        rmcp::ErrorData::internal_error("Failed to serialize tool output", None)
    })?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

/// Uniform tool exit: success serializes to JSON text, business failures
/// become agent-readable tool errors (HTTP stays 200).
fn finish_tool<T: Serialize>(
    result: Result<T, ToolError>,
) -> Result<CallToolResult, rmcp::ErrorData> {
    match result {
        Ok(value) => json_success(&value),
        Err(e) => Ok(e.to_call_tool_result()),
    }
}

/// The wire name of a serde string enum (audit category/action/… have no
/// Display impl); the empty-string fallback is unreachable for these enums.
fn enum_str<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn user_item(user: herald_core::domain::user::User) -> dto::UserItem {
    dto::UserItem {
        id: user.id.to_string(),
        email: user.email,
        nickname: user.nickname,
        status: i16::from(user.status) as i32,
        created_at: user.created_at.to_rfc3339(),
    }
}

fn balance_view(balance: PointsBalance) -> dto::PointsBalanceView {
    dto::PointsBalanceView {
        user_id: balance.user_id.to_string(),
        scope: "realm".to_string(),
        balance: balance.balance,
        topup_balance: balance.topup_balance,
        subscription_balance: balance.subscription_balance,
        granted_balance: balance.granted_balance,
        registration_balance: balance.registration_balance,
        free_periodic_balance: balance.free_periodic_balance,
        updated_at: balance.updated_at.to_rfc3339(),
    }
}

fn transactions_page(
    paginated: Paginated<PointsTransaction>,
    page: u64,
    page_size: u64,
) -> dto::TransactionsPage {
    dto::TransactionsPage {
        transactions: paginated
            .data
            .into_iter()
            .map(|tx| dto::TransactionItem {
                transaction_id: tx.id.to_string(),
                transaction_type: tx.transaction_type.to_string(),
                amount: tx.amount,
                balance_after: tx.balance_after,
                description: tx.description,
                created_at: tx.created_at.to_rfc3339(),
            })
            .collect(),
        page,
        page_size,
        total: paginated.total,
    }
}

/// Verify the transport preflight marker for a self tool. The middleware ran
/// the scope check before any read and tagged the request; a missing or
/// drifted marker means the middleware contract was violated — an internal
/// protocol error, never a silently-downgraded tool message.
fn require_preflight(parts: &Parts, tool: &'static str) -> Result<(), rmcp::ErrorData> {
    let ok = parts
        .extensions
        .get::<crate::mcp_oauth_auth::ScopePreflight>()
        .is_some_and(|marker| marker.tool == tool);
    if ok {
        Ok(())
    } else {
        tracing::error!(
            tool = tool,
            "MCP self tool reached without a matching scope preflight marker"
        );
        Err(rmcp::ErrorData::internal_error(
            "The request could not be completed. Please retry later.",
            None,
        ))
    }
}

impl HeraldMcpService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// The streamable-HTTP factory builds a service per request, and
    /// `#[tool_handler]` hits the router on every tools/call and tools/list —
    /// the router is stateless (fn pointers + cached schemas), so build it
    /// once per process instead of per request.
    fn shared_router() -> &'static ToolRouter<Self> {
        static ROUTER: std::sync::LazyLock<ToolRouter<HeraldMcpService>> =
            std::sync::LazyLock::new(HeraldMcpService::tool_router);
        &ROUTER
    }

    /// Verify a target user exists in the credential's realm WITHOUT any
    /// users.view grant: a direct repository read plus a realm comparison.
    /// `user_service.get_user` would demand the admin view permission,
    /// which points.view targets must not need; cross-realm and missing
    /// targets are indistinguishable and both read as not_found.
    async fn ensure_target_user_exists(
        &self,
        realm_id: &str,
        user_id: uuid::Uuid,
    ) -> Result<(), ToolError> {
        let user = self
            .state
            .user_repository
            .get_user_by_id(user_id)
            .await
            .map_err(|e| map_user_lookup_error(e, &user_id.to_string()))?;
        if user.realm_id != realm_id {
            return Err(ToolError::not_found(format!(
                "User {} was not found in this realm.",
                user_id
            )));
        }
        Ok(())
    }
}

#[tool_router]
impl HeraldMcpService {
    #[tool(
        description = "List or look up users in your Herald realm. Omit 'userId' to page \
        through all users (optionally filtered by exact 'email'); provide 'userId' (UUID) \
        to fetch a single user's detail. Requires the users.view permission. User status \
        codes: 0=wait_verified, 1=normal, 2=forbidden, 3=deleted."
    )]
    async fn query_users(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<dto::QueryUsersInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let identity = identity_from_parts(&parts)?;
        let realm_id = identity.realm_id();

        if let Err(e) = ensure_permission(&self.state, &identity, "users", "view").await {
            return Ok(e.to_call_tool_result());
        }

        let result: Result<dto::UsersPage, ToolError> = async {
            let (page, page_size) = dto::normalize_page(input.page, input.page_size)?;

            if let Some(user_id) = input.user_id.as_deref() {
                let uuid = dto::parse_uuid("userId", user_id)?;
                let user = self
                    .state
                    .service
                    .user_service()
                    .get_user(identity.clone(), uuid)
                    .await
                    .map_err(|e| map_user_lookup_error(e, user_id))?;
                Ok(dto::UsersPage {
                    users: vec![user_item(user)],
                    page,
                    page_size,
                    total: 1,
                })
            } else {
                let (users, total) = self
                    .state
                    .service
                    .user_service()
                    .list_users(
                        identity.clone(),
                        realm_id.clone(),
                        page,
                        page_size,
                        input.email,
                        None,
                    )
                    .await
                    .map_err(|e| map_core_error(e, "users.view"))?;
                Ok(dto::UsersPage {
                    users: users.into_iter().map(user_item).collect(),
                    page,
                    page_size,
                    total: total.max(0) as u64,
                })
            }
        }
        .await;

        finish_tool(result)
    }

    #[tool(description = "Get a user's points balance in your realm, summed \
        across every account bucket in the realm (not narrowed by client app). \
        Requires the points.view permission.")]
    async fn get_points_balance(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<dto::GetPointsBalanceInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let identity = identity_from_parts(&parts)?;
        let realm_id = identity.realm_id();

        if let Err(e) = ensure_permission(&self.state, &identity, "points", "view").await {
            return Ok(e.to_call_tool_result());
        }

        let result: Result<dto::PointsBalanceView, ToolError> = async {
            let user_id = dto::parse_uuid("userId", &input.user_id)?;
            // Existence first: the balance path synthesizes zero balances for
            // wallet-less users and would misreport "no such user" as 0.
            self.ensure_target_user_exists(&realm_id, user_id).await?;

            let balance = self
                .state
                .points_service
                .get_balance_for_admin_tool(&realm_id, user_id)
                .await
                .map_err(|e| map_core_error(e, "points.view"))?;

            Ok(balance_view(balance))
        }
        .await;

        finish_tool(result)
    }

    #[tool(description = "List a user's points transactions in your realm, \
        newest first, with optional transactionType and time-range filters. \
        Requires the points.view permission. 'amount' is signed: consumption \
        is negative, grants are positive.")]
    async fn list_points_transactions(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<dto::ListPointsTransactionsInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let identity = identity_from_parts(&parts)?;
        let realm_id = identity.realm_id();

        if let Err(e) = ensure_permission(&self.state, &identity, "points", "view").await {
            return Ok(e.to_call_tool_result());
        }

        let result: Result<dto::TransactionsPage, ToolError> = async {
            let user_id = dto::parse_uuid("userId", &input.user_id)?;
            self.ensure_target_user_exists(&realm_id, user_id).await?;

            let (page, page_size) = dto::normalize_page(input.page, input.page_size)?;
            let (start_time, end_time) =
                dto::parse_time_range(input.start_time.as_deref(), input.end_time.as_deref())?;
            // Direct repository read with the target user and realm pinned:
            // PointsService::list_transactions narrows non-points.manage
            // users to their own rows (the REST rule), which would silently
            // blank this admin query. The two filters are hard overrides —
            // the tool surface never accepts a clientAppId.
            let filters = TransactionFilters {
                user_id: Some(user_id),
                transaction_type: input
                    .transaction_type
                    .as_deref()
                    .map(|v| dto::parse_transaction_type("transactionType", v))
                    .transpose()?,
                start_time,
                end_time,
                client_app_id: None,
                page: Some(page),
                page_size: Some(page_size),
                ..Default::default()
            };

            let paginated = self
                .state
                .points_repository
                .find_transactions(&realm_id, filters)
                .await
                .map_err(|e| map_core_error(e, "points.view"))?;

            Ok(transactions_page(paginated, page, page_size))
        }
        .await;

        finish_tool(result)
    }

    #[tool(description = "List audit log events for your realm with optional \
        category, action, actorId and time-range filters. Requires the \
        audit.view permission. Categories: user_management, rbac, \
        realm_management, auth, billing, oauth, compliance.")]
    async fn list_audit_logs(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<dto::ListAuditLogsInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let identity = identity_from_parts(&parts)?;

        if let Err(e) = ensure_permission(&self.state, &identity, "audit", "view").await {
            return Ok(e.to_call_tool_result());
        }

        let result: Result<dto::AuditEventsPage, ToolError> = async {
            let realm_id = identity.realm_id();
            let (page, page_size) = dto::normalize_page(input.page, input.page_size)?;
            let (start_time, end_time) =
                dto::parse_time_range(input.start_time.as_deref(), input.end_time.as_deref())?;
            // The audit repository paginates 0-based (offset = page*size);
            // the tool surface is 1-based like every other tool.
            let filters = AuditEventFilters {
                category: input
                    .category
                    .as_deref()
                    .map(|v| dto::parse_audit_category("category", v))
                    .transpose()?,
                action: input
                    .action
                    .as_deref()
                    .map(|v| dto::parse_audit_action("action", v))
                    .transpose()?,
                actor_id: input.actor_id,
                start_time,
                end_time,
                page: page - 1,
                page_size,
            };

            let paginated = self
                .state
                .audit_event_repository
                .list_paginated(&realm_id, filters)
                .await
                .map_err(|e| {
                    tracing::error!("MCP audit listing failed: {e}");
                    ToolError::internal()
                })?;

            Ok(dto::AuditEventsPage {
                events: paginated
                    .items
                    .into_iter()
                    .map(|event| dto::AuditEventItem {
                        id: event.id.to_string(),
                        category: enum_str(&event.category),
                        action: enum_str(&event.action),
                        actor_id: event.actor_id,
                        actor_name: event.actor_name,
                        target_type: enum_str(&event.target_type),
                        target_id: event.target_id,
                        result: enum_str(&event.result),
                        created_at: event.created_at.to_rfc3339(),
                    })
                    .collect(),
                page,
                page_size,
                total: paginated.total,
            })
        }
        .await;

        finish_tool(result)
    }

    #[tool(description = "Get a configuration status overview for your realm: \
        which settings exist and whether they are enabled. Values are never \
        returned. Requires the settings.view permission. This is the lightest \
        tool and doubles as an end-to-end connectivity self-check.")]
    async fn get_realm_config_status(
        &self,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let identity = identity_from_parts(&parts)?;

        if let Err(e) = ensure_permission(&self.state, &identity, "settings", "view").await {
            return Ok(e.to_call_tool_result());
        }

        let result: Result<dto::RealmConfigStatus, ToolError> = async {
            let realm_id = identity.realm_id();
            // Direct repository read: realm_config_service's policy check keys on
            // identity.user_id(), which is empty for the service's expected
            // callers — the permission gate above is the real check.
            let configs = self
                .state
                .realm_config_repository
                .get_all(realm_id.clone())
                .await
                .map_err(|e| {
                    tracing::error!("MCP realm config listing failed: {e}");
                    ToolError::internal()
                })?;
            Ok(dto::RealmConfigStatus {
                realm_id,
                configs: configs
                    .into_iter()
                    .map(|config| dto::ConfigStatusItem {
                        config_type: enum_str(&config.config_type),
                        config_key: config.config_key,
                        enabled: config.enabled,
                        is_secret: config.is_secret,
                    })
                    .collect(),
            })
        }
        .await;

        finish_tool(result)
    }

    // ========================================================================
    // Self-face tools: the operating user is derived from the verified
    // identity; no tool accepts a userId, so reading another user is
    // structurally inexpressible. The scope gate ran in the transport
    // preflight (HTTP 403 challenge before any read); the marker check below
    // is the contract guard against drift.
    // ========================================================================

    #[tool(description = "Get your own account profile. Requires the \
        mcp:profile:read scope (granted by default on MCP authorization).")]
    async fn get_my_profile(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(_): Parameters<dto::NoInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_preflight(&parts, "get_my_profile")?;
        let identity = identity_from_parts(&parts)?;
        let result: Result<dto::UserItem, ToolError> = async {
            let user = identity.as_user().ok_or_else(ToolError::internal)?;
            Ok(user_item(user.clone()))
        }
        .await;

        finish_tool(result)
    }

    #[tool(description = "Get your own points balance, summed across every \
        account bucket in the realm. Requires the mcp:points:read scope.")]
    async fn get_my_points_balance(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(_): Parameters<dto::NoInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_preflight(&parts, "get_my_points_balance")?;
        let identity = identity_from_parts(&parts)?;
        let realm_id = identity.realm_id();

        let result: Result<dto::PointsBalanceView, ToolError> = async {
            let user = identity.as_user().ok_or_else(ToolError::internal)?;
            let balance = self
                .state
                .points_service
                .get_balance(identity.clone(), &realm_id, user.id)
                .await
                .map_err(|e| map_core_error(e, "mcp:points:read"))?;

            Ok(balance_view(balance))
        }
        .await;

        finish_tool(result)
    }

    #[tool(description = "List your own points transactions, newest first, \
        with optional transactionType and time-range filters. An empty list \
        is a normal result. Requires the mcp:transactions:read scope.")]
    async fn list_my_points_transactions(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<dto::MyTransactionsInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_preflight(&parts, "list_my_points_transactions")?;
        let identity = identity_from_parts(&parts)?;
        let realm_id = identity.realm_id();

        let result: Result<dto::TransactionsPage, ToolError> = async {
            let user = identity.as_user().ok_or_else(ToolError::internal)?;
            let (page, page_size) = dto::normalize_page(input.page, input.page_size)?;
            let (start_time, end_time) =
                dto::parse_time_range(input.start_time.as_deref(), input.end_time.as_deref())?;
            // The self path through PointsService applies exactly the right
            // rule: the requesting user's own rows only.
            let filters = TransactionFilters {
                user_id: Some(user.id),
                transaction_type: input
                    .transaction_type
                    .as_deref()
                    .map(|v| dto::parse_transaction_type("transactionType", v))
                    .transpose()?,
                start_time,
                end_time,
                client_app_id: None,
                page: Some(page),
                page_size: Some(page_size),
                ..Default::default()
            };

            let paginated = self
                .state
                .points_service
                .list_transactions(identity.clone(), &realm_id, filters)
                .await
                .map_err(|e| map_core_error(e, "mcp:transactions:read"))?;

            Ok(transactions_page(paginated, page, page_size))
        }
        .await;

        finish_tool(result)
    }

    #[tool(description = "List your own subscriptions with status and the \
        entitlement each one currently grants. Expired and canceled \
        subscriptions stay visible with hasAccess=false; an empty list with \
        hasSubscription=false is a normal result. Requires the \
        mcp:subscriptions:read scope.")]
    async fn list_my_subscriptions(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<dto::MySubscriptionsInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_preflight(&parts, "list_my_subscriptions")?;
        let identity = identity_from_parts(&parts)?;

        let result: Result<dto::MySubscriptionsPage, ToolError> = async {
            let user = identity.as_user().ok_or_else(ToolError::internal)?;
            let realm_id = identity.realm_id();
            let (page, page_size) = dto::normalize_page(input.page, input.page_size)?;

            let (subscriptions, total, has_active) = self
                .state
                .billing_repository
                .list_user_subscriptions(&realm_id, user.id, page, page_size)
                .await
                .map_err(|e| map_core_error(e, "mcp:subscriptions:read"))?;

            let items: Vec<dto::SubscriptionItem> = subscriptions
                .into_iter()
                .map(|sub| {
                    let has_access = sub.status.has_access();
                    dto::SubscriptionItem {
                        id: sub.id.to_string(),
                        status: sub.status.as_str().to_string(),
                        billing_type: sub.billing_type.as_str().to_string(),
                        entitlement_key: sub.entitlement_key.clone(),
                        has_access,
                        current_period_start: sub.current_period_start.map(|t| t.to_rfc3339()),
                        current_period_end: sub.current_period_end.map(|t| t.to_rfc3339()),
                        cancel_at_period_end: sub.cancel_at_period_end,
                        // Only the row's own entitlement surfaces, and only
                        // while it actually grants access — payment metadata
                        // and external ids never do.
                        active_entitlements: if has_access {
                            vec![sub.entitlement_key]
                        } else {
                            Vec::new()
                        },
                    }
                })
                .collect();

            Ok(dto::MySubscriptionsPage {
                has_subscription: total > 0,
                has_active_subscription: has_active,
                subscriptions: items,
                page,
                page_size,
                total,
            })
        }
        .await;

        finish_tool(result)
    }
}

#[tool_handler(name = "herald-mcp", router = Self::shared_router())]
impl ServerHandler for HeraldMcpService {}
