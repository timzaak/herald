// The five read-only Herald MCP tools.
//
// Common structure (the three-check contract: authenticate → authorize → read):
// 1. The protocol middleware authenticated the caller (Identity in Parts).
// 2. `ensure_permission` is the FIRST business statement of every tool —
//    the user and points services do not gate ThirdParty identities, so the
//    tool layer is the only RBAC defense on this surface.
// 3. realm always comes from the credential; no tool accepts a realmId
//    argument, so cross-realm reads are structurally inexpressible (a user
//    of another realm simply reads as not_found).
//
// Field minimization is part of the contract, not style: agents may carry
// tool output into third-party models, so audit details (ip, user agent,
// trace id, details), config values, and ledger attribution fields are
// deliberately absent from the DTOs.

use http::request::Parts;
use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use sea_orm::EntityTrait;
use serde::Serialize;

use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{AuditEventFilters, AuditEventRepository};
use herald_core::domain::authentication::Identity;
use herald_core::domain::client_api_keys::constants::ADMIN_API_CLIENT_ID;
use herald_core::domain::points::ports::TransactionFilters;
use herald_core::domain::realm_config::RealmConfigRepository;
use herald_core::domain::user::User;
use herald_core::domain::user::ports::UserService;
use herald_core::entity::client_app;
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

fn user_item(user: User) -> dto::UserItem {
    dto::UserItem {
        id: user.id.to_string(),
        email: user.email,
        nickname: user.nickname,
        status: i16::from(user.status) as i32,
        created_at: user.created_at.to_rfc3339(),
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

    /// Verify the target user exists in the credential's realm. Must run
    /// before any balance/transaction query: `get_balance` synthesizes a
    /// zero balance for users without a wallet, which would misreport a
    /// non-existent user as "0 points".
    async fn ensure_user_exists(
        &self,
        identity: &Identity,
        user_id: uuid::Uuid,
    ) -> Result<(), ToolError> {
        self.state
            .service
            .user_service()
            .get_user(identity.clone(), user_id)
            .await
            .map(|_| ())
            .map_err(|e| map_user_lookup_error(e, &user_id.to_string()))
    }

    /// Resolve the balance scope exactly like the ext API: a client-app-
    /// bound key reads its app's covered buckets unless the bound app is the
    /// admin-api client; unbound keys read realm-wide.
    async fn balance_scope(
        &self,
        identity: &Identity,
    ) -> Result<(&'static str, Option<uuid::Uuid>), ToolError> {
        let bound_app_id = identity.as_third_party().and_then(|key| key.client_app_id);
        match bound_app_id {
            None => Ok(("realm", None)),
            Some(app_id) => {
                let bound_app = client_app::Entity::find_by_id(app_id)
                    .one(self.state.db.as_ref())
                    .await
                    .map_err(|e| {
                        tracing::error!("Failed to load API key bound Client App: {e}");
                        ToolError::internal()
                    })?;
                if bound_app
                    .as_ref()
                    .is_some_and(|app| app.client_id == ADMIN_API_CLIENT_ID)
                {
                    Ok(("realm", None))
                } else {
                    Ok(("client_app", Some(app_id)))
                }
            }
        }
    }
}

#[tool_router]
impl HeraldMcpService {
    #[tool(
        description = "List or look up users in the Herald realm this API key \
        belongs to. Omit 'userId' to page through all users (optionally filtered by \
        exact 'email'); provide 'userId' (UUID) to fetch a single user's detail. \
        Requires the users.view permission. User status codes: 1=normal, \
        0=disabled/waiting verification."
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

    #[tool(description = "Get a user's points balance in this API key's realm. \
        Requires the points.view permission. The 'scope' field reports what the \
        balance covers: \"realm\" (all buckets) or \"client_app\" (only the \
        buckets this key's bound client app covers).")]
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
            // Existence first: get_balance synthesizes zero balances for
            // wallet-less users and would misreport "no such user" as 0.
            // The existence probe and the scope resolution are independent
            // reads, so they run concurrently; existence errors still
            // surface first.
            let (user_exists, scope_result) = tokio::join!(
                self.ensure_user_exists(&identity, user_id),
                self.balance_scope(&identity),
            );
            user_exists?;
            let (scope, scoped_app_id) = scope_result?;

            let balance = match scoped_app_id {
                Some(app_id) => {
                    self.state
                        .points_service
                        .get_balance_for_client_app(identity.clone(), &realm_id, user_id, app_id)
                        .await
                }
                None => {
                    self.state
                        .points_service
                        .get_balance(identity.clone(), &realm_id, user_id)
                        .await
                }
            }
            .map_err(|e| map_core_error(e, "points.view"))?;

            Ok(dto::PointsBalanceView {
                user_id: balance.user_id.to_string(),
                scope: scope.to_string(),
                balance: balance.balance,
                topup_balance: balance.topup_balance,
                subscription_balance: balance.subscription_balance,
                granted_balance: balance.granted_balance,
                registration_balance: balance.registration_balance,
                free_periodic_balance: balance.free_periodic_balance,
                updated_at: balance.updated_at.to_rfc3339(),
            })
        }
        .await;

        finish_tool(result)
    }

    #[tool(description = "List a user's points transactions in this API key's \
        realm, newest first, with optional transactionType and time-range \
        filters. Requires the points.view permission. 'amount' is signed: \
        consumption is negative, grants are positive.")]
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
            self.ensure_user_exists(&identity, user_id).await?;

            let (page, page_size) = dto::normalize_page(input.page, input.page_size)?;
            let filters = TransactionFilters {
                user_id: Some(user_id),
                transaction_type: input
                    .transaction_type
                    .as_deref()
                    .map(|v| dto::parse_transaction_type("transactionType", v))
                    .transpose()?,
                start_time: input
                    .start_time
                    .as_deref()
                    .map(|v| dto::parse_time_rfc3339("startTime", v))
                    .transpose()?,
                end_time: input
                    .end_time
                    .as_deref()
                    .map(|v| dto::parse_time_rfc3339("endTime", v))
                    .transpose()?,
                page: Some(page),
                page_size: Some(page_size),
                ..Default::default()
            };

            let paginated = self
                .state
                .points_service
                .list_transactions(identity.clone(), &realm_id, filters)
                .await
                .map_err(|e| map_core_error(e, "points.view"))?;

            Ok(dto::TransactionsPage {
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
            })
        }
        .await;

        finish_tool(result)
    }

    #[tool(description = "List audit log events for this API key's realm with \
        optional category, action, actorId and time-range filters. Requires the \
        audit.view permission. Categories: user_management, rbac, \
        realm_management, auth, billing, oauth, compliance.")]
    async fn list_audit_logs(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<dto::ListAuditLogsInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let identity = identity_from_parts(&parts)?;
        let realm_id = identity.realm_id();

        if let Err(e) = ensure_permission(&self.state, &identity, "audit", "view").await {
            return Ok(e.to_call_tool_result());
        }

        let result: Result<dto::AuditEventsPage, ToolError> = async {
            let (page, page_size) = dto::normalize_page(input.page, input.page_size)?;
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
                start_time: input
                    .start_time
                    .as_deref()
                    .map(|v| dto::parse_query_time("startTime", v))
                    .transpose()?,
                end_time: input
                    .end_time
                    .as_deref()
                    .map(|v| dto::parse_query_time("endTime", v))
                    .transpose()?,
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

    #[tool(
        description = "Get a configuration status overview for this API key's \
        realm: which settings exist and whether they are enabled. Values are \
        never returned. Requires the settings.view permission. This is the \
        lightest tool and doubles as an end-to-end connectivity self-check."
    )]
    async fn get_realm_config_status(
        &self,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let identity = identity_from_parts(&parts)?;
        let realm_id = identity.realm_id();

        if let Err(e) = ensure_permission(&self.state, &identity, "settings", "view").await {
            return Ok(e.to_call_tool_result());
        }

        // Direct repository read: realm_config_service's policy check keys on
        // identity.user_id(), which is empty for API keys and would always
        // deny — the permission gate above is the real check.
        let result: Result<dto::RealmConfigStatus, ToolError> = match self
            .state
            .realm_config_repository
            .get_all(realm_id.clone())
            .await
        {
            Ok(configs) => Ok(dto::RealmConfigStatus {
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
            }),
            Err(e) => {
                tracing::error!("MCP realm config listing failed: {e}");
                Err(ToolError::internal())
            }
        };

        finish_tool(result)
    }
}

#[tool_handler(name = "herald-mcp", router = Self::shared_router())]
impl ServerHandler for HeraldMcpService {}
