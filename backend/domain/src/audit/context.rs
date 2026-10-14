//! Request-scoped audit context threaded from the HTTP layer into domain
//! services.
//!
//! Every audit-writing domain service takes an `&AuditContext` (or owned copy)
//! so that `ip_address` / `user_agent` / actor identity are recorded
//! consistently without each service re-deriving them from `Identity` (which
//! carries no HTTP request data). This is the single, canonical carrier for
//! the actor + request half of a [`NewAuditEvent`](super::NewAuditEvent);
//! target / category / action / details remain the caller's responsibility.

use crate::audit::ActorType;
use crate::authentication::Identity;

/// Audit-write metadata derived from the current HTTP request.
///
/// Field-for-field mirrors the actor-related columns of `NewAuditEvent`
/// (`actor_id`, `actor_type`, `actor_name`, `ip_address`, `user_agent`,
/// `trace_id`). Keeping these bundled lets domain services stay HTTP-agnostic
/// while still recording request-scoped audit context.
#[derive(Debug, Clone)]
pub struct AuditContext {
    /// Authenticated actor's identifier (user id, client id, etc.). For
    /// pre-authentication failure paths where the actor is unknown, use the
    /// best-available identifier (e.g. the attempted email) or `"unknown"`.
    pub actor_id: String,
    /// `None` when the actor type cannot be determined (e.g. failed login
    /// before the principal was identified).
    pub actor_type: Option<ActorType>,
    pub actor_name: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    /// Currently always `None` — no trace-id middleware exists yet. The field
    /// is carried so a future extractor can populate it without re-threading
    /// signatures.
    pub trace_id: Option<String>,
}

impl AuditContext {
    /// Build a context for an authenticated admin operation.
    ///
    /// `ip` is the value extracted by the `ClientIp` axum extractor (always
    /// `Some`, possibly of an empty string); `user_agent` comes from
    /// `user_agent_from_headers`.
    pub fn admin(identity: &Identity, ip: String, user_agent: Option<String>) -> Self {
        Self {
            actor_id: identity.user_id(),
            actor_type: Some(ActorType::Admin),
            actor_name: identity.as_user().map(|u| u.email.clone()),
            ip_address: Some(ip),
            user_agent,
            trace_id: None,
        }
    }

    /// Build a context for a self-service (end-user) operation.
    pub fn user(identity: &Identity, ip: String, user_agent: Option<String>) -> Self {
        Self {
            actor_id: identity.user_id(),
            actor_type: Some(ActorType::User),
            actor_name: identity.as_user().map(|u| u.email.clone()),
            ip_address: Some(ip),
            user_agent,
            trace_id: None,
        }
    }

    /// Build a context for an ext-face operation initiated by an application
    /// backend through a Client-App-bound API key. Unlike the admin/user
    /// constructors there is no `Identity::User` behind the call, so the
    /// actor is the application itself: `actor_id` is the bound Client App's
    /// `client_id` and `actor_name` its display name. The API key id belongs
    /// in the event's `details` (via `viaApiKeyId`), never in `actor_id`.
    pub fn ext_client(
        client_id: &str,
        app_name: Option<String>,
        ip: String,
        user_agent: Option<String>,
    ) -> Self {
        Self {
            actor_id: client_id.to_string(),
            actor_type: Some(ActorType::Client),
            actor_name: app_name,
            ip_address: Some(ip),
            user_agent,
            trace_id: None,
        }
    }
}
