// Tool-level error semantics for the MCP surface.
//
// MCP distinguishes two failure modes: protocol errors (`Err(McpError)`,
// rendered opaquely by clients) and tool-level errors (`Ok(CallToolResult)`
// with `isError: true`, whose content the agent reads). Every business
// failure a Herald tool can produce is agent-facing by design: the error
// text must tell the agent what to do next (which permission to request,
// which argument shape is wrong) without leaking internals (SQL, stack,
// raw domain error strings — those go to tracing only).

use http::request::Parts;
use rmcp::model::{CallToolResult, ContentBlock};
use tracing::error;

use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::common::entities::app_errors::CoreError;

#[derive(Debug, Clone, Copy)]
pub enum ToolErrorCode {
    PermissionDenied,
    NotFound,
    InvalidArgument,
    InternalError,
}

impl ToolErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolErrorCode::PermissionDenied => "permission_denied",
            ToolErrorCode::NotFound => "not_found",
            ToolErrorCode::InvalidArgument => "invalid_argument",
            ToolErrorCode::InternalError => "internal_error",
        }
    }
}

impl std::fmt::Display for ToolErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct ToolError {
    code: ToolErrorCode,
    message: String,
}

impl ToolError {
    pub fn permission_denied(permission: &str) -> Self {
        ToolError {
            code: ToolErrorCode::PermissionDenied,
            message: format!(
                "This API key does not have the '{permission}' permission. \
Ask your realm administrator to grant it to a role bound to this key."
            ),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        ToolError {
            code: ToolErrorCode::NotFound,
            message: message.into(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        ToolError {
            code: ToolErrorCode::InvalidArgument,
            message: message.into(),
        }
    }

    /// Generic internal failure. The original error never reaches the agent;
    /// callers log it via tracing before calling this.
    pub fn internal() -> Self {
        ToolError {
            code: ToolErrorCode::InternalError,
            message: "The request could not be completed. Please retry later.".to_string(),
        }
    }

    pub fn code_as_str(&self) -> &'static str {
        self.code.as_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// Tool-level error exit: HTTP stays 200, the agent reads
    /// `"<code>: <agent-readable message>"` as isError content.
    pub fn to_call_tool_result(self) -> CallToolResult {
        CallToolResult::error(vec![ContentBlock::text(format!(
            "{}: {}",
            self.code, self.message
        ))])
    }
}

/// Tool-layer permission gate — the ONLY RBAC defense on this surface.
/// The user and points services apply no permission checks to ThirdParty
/// identities, so skipping this call means unauthenticated-by-RBAC data
/// access; every tool must run it as its first business statement.
/// The realm is taken from the credential, never from tool arguments.
///
/// Mirrors api-ext `authz::require_principal_permission` (checker errors are
/// treated as denial, never as allowance) but returns the agent-readable
/// ToolError instead of an axum response, and avoids a feature-crate
/// dependency on herald-api-ext.
pub async fn ensure_permission(
    state: &AppState,
    identity: &Identity,
    resource: &str,
    action: &str,
) -> Result<(), ToolError> {
    let realm_id = identity.realm_id();
    let permission = format!("{resource}.{action}");
    let principal = identity.principal_ref();
    let allowed = state
        .permission_checker
        .check_principal_permission(
            &realm_id,
            principal.principal_type,
            &principal.principal_id,
            resource,
            action,
        )
        .await
        .unwrap_or(false);

    if allowed {
        Ok(())
    } else {
        tracing::warn!(
            realm_id = %realm_id,
            principal_id = %principal.principal_id,
            permission = %permission,
            "MCP tool call denied: missing permission"
        );
        Err(ToolError::permission_denied(&permission))
    }
}

/// Recover the `Identity` that the protocol-level auth middleware inserted
/// into the request extensions. Missing identity means the middleware
/// contract was violated (endpoint mounted without auth) — a protocol-level
/// internal error, never a client error.
pub fn identity_from_parts(parts: &Parts) -> Result<Identity, rmcp::ErrorData> {
    parts.extensions.get::<Identity>().cloned().ok_or_else(|| {
        error!("MCP tool reached without an authenticated identity in request parts");
        rmcp::ErrorData::internal_error(
            "The request could not be completed. Please retry later.",
            None,
        )
    })
}

/// Map a user-lookup failure. `get_user` returns Forbidden for users of
/// another realm; since the realm is always taken from the credential and
/// cannot be requested cross-realm, a Forbidden here is indistinguishable
/// from "does not exist" — both surface as not_found with zero data.
pub fn map_user_lookup_error(e: CoreError, user_id: &str) -> ToolError {
    match e {
        CoreError::NotFound | CoreError::Forbidden(_) => {
            ToolError::not_found(format!("User {user_id} was not found in this realm."))
        }
        other => {
            error!(error = %other, "User lookup failed");
            ToolError::internal()
        }
    }
}

/// Map a domain error from a read-path service call to a tool error.
/// `permission` is echoed back on Forbidden so the agent learns which grant
/// is missing.
pub fn map_core_error(e: CoreError, permission: &str) -> ToolError {
    match e {
        CoreError::NotFound => {
            ToolError::not_found("The requested resource was not found in this realm.")
        }
        CoreError::Forbidden(_) | CoreError::Unauthorized => {
            ToolError::permission_denied(permission)
        }
        CoreError::BadRequest(msg) => ToolError::invalid_argument(msg),
        other => {
            error!(error = %other, "MCP tool domain call failed");
            ToolError::internal()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_denied_message_names_the_permission_and_next_step() {
        let e = ToolError::permission_denied("users.view");
        assert_eq!(e.code_as_str(), "permission_denied");
        assert!(e.message().contains("'users.view'"));
        assert!(e.message().contains("administrator"));
    }

    #[test]
    fn to_call_tool_result_renders_code_prefix_and_is_error() {
        let r = ToolError::not_found("User x was not found in this realm.").to_call_tool_result();
        assert_eq!(r.is_error, Some(true));
        let text = match r.content.first() {
            Some(rmcp::model::ContentBlock::Text(t)) => t.text.clone(),
            other => panic!("expected text content, got {other:?}"),
        };
        assert!(text.starts_with("not_found: User x was not found in this realm."));
    }

    #[test]
    fn internal_error_never_contains_debug_of_domain_error() {
        // The internal mapping must drop the original error text entirely —
        // map_core_error logs it, the agent sees only the generic message.
        let e = map_core_error(
            CoreError::DatabaseError("secret table pg_user query failed".into()),
            "points.view",
        );
        assert_eq!(e.code_as_str(), "internal_error");
        assert!(!e.message().contains("pg_user"));
    }

    #[test]
    fn map_core_error_routes_domain_variants() {
        assert_eq!(
            map_core_error(CoreError::NotFound, "x.view").code_as_str(),
            "not_found"
        );
        assert_eq!(
            map_core_error(CoreError::Forbidden("no".into()), "x.view").code_as_str(),
            "permission_denied"
        );
        assert_eq!(
            map_core_error(CoreError::BadRequest("bad input".into()), "x.view").code_as_str(),
            "invalid_argument"
        );
    }

    #[test]
    fn map_user_lookup_error_treats_cross_realm_forbidden_as_not_found() {
        assert_eq!(
            map_user_lookup_error(CoreError::NotFound, "u1").code_as_str(),
            "not_found"
        );
        // get_user yields Forbidden only for another realm's user; from this
        // credential's vantage point that user simply does not exist.
        assert_eq!(
            map_user_lookup_error(CoreError::Forbidden("different realm".into()), "u1")
                .code_as_str(),
            "not_found"
        );
    }
}
