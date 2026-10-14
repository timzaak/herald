// Organization API for Third-Party Integration
//
// App backends manage Herald-hosted organizations (mapped to their own
// workspace/team groupings via external_id) with a Client-App-bound API key.
// Every operation is scoped to (realm, bound client_app_id); out-of-scope
// organizations answer 404 uniformly so existence is never probed.

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::common::error_helpers::json_error;
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::AuditContext;
use herald_core::domain::authentication::Identity;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::organization::{
    OrgActorOrigin, OrgCallerScope, OrgCheckOutcome, OrgError, OrganizationService,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::authz::{require_principal_permission, require_realm_membership};
use crate::client_app_scope::load_bound_client_app;

const EXTERNAL_ID_MAX: usize = 255;
const NAME_MAX: usize = 255;

// ============================================================================
// DTOs
// ============================================================================

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrgExtRequest {
    /// App-side identifier (workspace id etc.); unique per Client App,
    /// 1-255 chars after trim. Replaying an existing externalId returns the
    /// existing organization with `created = false`.
    pub external_id: String,
    /// Optional display name (<= 255 chars).
    pub name: Option<String>,
    /// Owner account id; must belong to the same realm.
    pub owner_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrgExtResponse {
    pub id: String,
    pub external_id: String,
    pub name: Option<String>,
    pub owner_id: String,
    pub realm_id: String,
    pub client_id: String,
    pub created_at: String,
    pub created: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OrgRoleRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertOrgMemberExtRequest {
    pub user_id: String,
    /// Tier name resolved within (realm, the organization's client_id);
    /// built-ins are `org-owner` / `org-admin` / `org-member`.
    pub role: String,
    /// User the application acts on behalf of; when present the change is
    /// gated by that user's in-org management and self-hold authority.
    pub actor_user_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrgMemberExtResponse {
    pub org_id: String,
    pub user_id: String,
    pub role: OrgRoleRef,
    pub is_owner: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListOrgMembersExtQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrgMemberListItem {
    pub user_id: String,
    pub email: String,
    pub nickname: Option<String>,
    pub role: OrgRoleRef,
    pub is_owner: bool,
    pub joined_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrgMemberListResponse {
    pub items: Vec<OrgMemberListItem>,
    pub page: u64,
    pub page_size: u64,
    pub total: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetOrgMemberResponse {
    pub member: bool,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<OrgRoleRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_owner: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined_at: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema, Clone)]
pub struct OrgCheckRule {
    pub resource: String,
    pub action: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrgPermissionCheckExtRequest {
    pub user_id: String,
    /// All rules must hit for `allowed = true`. Deserialized as `Option` so
    /// a missing or empty array yields a structured 400 `rules_required`
    /// instead of a vacuous allowed=true.
    pub rules: Option<Vec<OrgCheckRule>>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrgPermissionCheckResponse {
    /// false = the user is not a member (a distinct result from a denied
    /// member: `member = true, allowed = false`).
    pub member: bool,
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<OrgRoleRef>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveOrgMemberExtQuery {
    pub actor_user_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListUserOrgsExtQuery {
    pub user_id: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserOrgListItem {
    pub org_id: String,
    pub external_id: String,
    pub name: Option<String>,
    pub role: OrgRoleRef,
    pub is_owner: bool,
    pub joined_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserOrgListResponse {
    pub items: Vec<UserOrgListItem>,
    pub page: u64,
    pub page_size: u64,
    pub total: i64,
}

// ============================================================================
// Caller resolution and error mapping
// ============================================================================

/// Resolve the organization caller scope: realm membership → Client-App
/// bound key (unbound and admin-api-client keys are rejected with 403
/// before any organization existence is probed) → flat policy gate
/// (`orgs.manage` for writes, `orgs.view` for reads).
async fn resolve_org_caller(
    state: &AppState,
    identity: &Identity,
    realm_id: &str,
    write: bool,
) -> Result<OrgCallerScope, Response> {
    if let Err(resp) = require_realm_membership(identity, realm_id, "organization access") {
        return Err(resp.into_response());
    }

    let api_key = identity
        .as_third_party()
        .ok_or_else(|| json_error(StatusCode::FORBIDDEN, ErrorCode::OrgClientAppKeyRequired))?;
    let bound_app_id = api_key
        .client_app_id
        .ok_or_else(|| json_error(StatusCode::FORBIDDEN, ErrorCode::OrgClientAppKeyRequired))?;

    let bound_app = load_bound_client_app(state, bound_app_id)
        .await?
        .filter(|app| app.realm_id == realm_id)
        .ok_or_else(|| json_error(StatusCode::FORBIDDEN, ErrorCode::OrgClientAppKeyRequired))?;

    if bound_app.client_id == herald_core::domain::client_api_keys::constants::ADMIN_API_CLIENT_ID {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            ErrorCode::OrgClientAppKeyRequired,
        ));
    }

    let (resource, action) = if write {
        ("orgs", "manage")
    } else {
        ("orgs", "view")
    };
    if let Err(resp) =
        require_principal_permission(state, identity, realm_id, resource, action).await
    {
        return Err(resp.into_response());
    }

    Ok(OrgCallerScope {
        realm_id: realm_id.to_string(),
        client_app_id: bound_app_id,
        client_id: bound_app.client_id,
        client_app_name: bound_app.name,
        api_key_id: api_key.id.clone(),
        origin: OrgActorOrigin::ClientCredential,
    })
}

fn org_error_response(error: OrgError) -> Response {
    match error {
        OrgError::NotFound => json_error(StatusCode::NOT_FOUND, ErrorCode::OrgNotFound),
        OrgError::OwnerProtected => json_error(StatusCode::CONFLICT, ErrorCode::OrgOwnerProtected),
        OrgError::LastAdminProtected => {
            json_error(StatusCode::CONFLICT, ErrorCode::OrgLastAdminProtected)
        }
        OrgError::RoleNotFound => json_error(StatusCode::BAD_REQUEST, ErrorCode::OrgRoleNotFound),
        OrgError::MemberNotFound => json_error(StatusCode::NOT_FOUND, ErrorCode::OrgMemberNotFound),
        OrgError::AccountScopeMismatch => {
            json_error(StatusCode::BAD_REQUEST, ErrorCode::OrgAccountScopeMismatch)
        }
        OrgError::ActorNotManager => {
            json_error(StatusCode::FORBIDDEN, ErrorCode::OrgActorNotManager)
        }
        OrgError::ActorSelfHoldRequired => {
            json_error(StatusCode::FORBIDDEN, ErrorCode::OrgActorSelfHoldRequired)
        }
        OrgError::OwnershipTransferDenied => {
            json_error(StatusCode::FORBIDDEN, ErrorCode::OrgOwnershipTransferDenied)
        }
        OrgError::Core(CoreError::NotFound) => {
            json_error(StatusCode::NOT_FOUND, ErrorCode::UserNotFound)
        }
        OrgError::Core(other) => ApiError::from(other).into_response(),
    }
}

// `Response` as the Err type keeps the handler early-return sites uniform;
// it is large but only ever short-lived on the error path.
#[allow(clippy::result_large_err)]
fn parse_uuid(value: &str) -> Result<Uuid, Response> {
    Uuid::parse_str(value)
        .map_err(|_| json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError))
}

#[allow(clippy::result_large_err)]
fn parse_page(page: Option<u64>, page_size: Option<u64>) -> Result<(u64, u64), Response> {
    let page = match page {
        None => 1,
        Some(p) if p >= 1 => p,
        Some(_) => {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                ErrorCode::ValidationError,
            ));
        }
    };
    let page_size = match page_size {
        None => 20,
        Some(s) if (1..=100).contains(&s) => s,
        Some(_) => {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                ErrorCode::ValidationError,
            ));
        }
    };
    // The repository binds OFFSET as i64; a page whose offset would overflow
    // the multiplication or the i64 range must fail validation here instead
    // of surfacing later as a database error.
    if (page - 1)
        .checked_mul(page_size)
        .is_none_or(|offset| offset > i64::MAX as u64)
    {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            ErrorCode::ValidationError,
        ));
    }
    Ok((page, page_size))
}

fn trimmed_non_empty(value: &str, max: usize) -> bool {
    let trimmed = value.trim();
    // The contract documents the limit in characters (DTO doc comments);
    // byte length would reject valid multi-byte values.
    !trimmed.is_empty() && trimmed.chars().count() <= max
}

/// Parse an optional `actorUserId` into an id, marking the caller scope as
/// acting on behalf of that user — audit attribution depends on the origin.
/// A present-but-unparseable value (including whitespace) is a 400: silently
/// treating it as absent would reroute the change to the client-credential
/// path and bypass the actor gates.
#[allow(clippy::result_large_err)]
fn resolve_actor(caller: &mut OrgCallerScope, raw: Option<&str>) -> Result<Option<Uuid>, Response> {
    match raw {
        None => Ok(None),
        Some(raw) => match parse_uuid(raw) {
            Ok(id) => {
                caller.origin = OrgActorOrigin::OnBehalfOfUser { user_id: id };
                Ok(Some(id))
            }
            Err(resp) => Err(resp),
        },
    }
}

fn ext_client_ctx(caller: &OrgCallerScope, ip: String, user_agent: Option<String>) -> AuditContext {
    AuditContext::ext_client(
        &caller.client_id,
        Some(caller.client_app_name.clone()),
        ip,
        user_agent,
    )
}

// ============================================================================
// Handlers
// ============================================================================

/// Create an organization
///
/// Creates a Herald organization for an app-side grouping, registering the
/// external id and the owner account. Idempotent: replaying an existing
/// `(clientApp, externalId)` returns the existing organization with
/// `created = false` and does not update it. The first creation for a
/// `(realm, client app)` seeds the three built-in tiers.
#[utoipa::path(
    post,
    path = "/api/ext/realms/{realmId}/orgs",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = CreateOrgExtRequest,
    responses(
        (status = 201, description = "Organization created", body = OrgExtResponse),
        (status = 200, description = "Idempotent replay returning the existing organization", body = OrgExtResponse),
        (status = 400, description = "Validation error / cross-realm owner account", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Unbound/admin API key, cross-realm access, or missing orgs.manage policy", body = ErrorResponse),
        (status = 404, description = "Owner account not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn create_org(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<CreateOrgExtRequest>,
) -> Response {
    let caller = match resolve_org_caller(&state, &identity, &realm_id, true).await {
        Ok(caller) => caller,
        Err(resp) => return resp,
    };

    if !trimmed_non_empty(&req.external_id, EXTERNAL_ID_MAX)
        || req
            .name
            .as_deref()
            .is_some_and(|name| !trimmed_non_empty(name, NAME_MAX))
    {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }
    let owner_id = match parse_uuid(&req.owner_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let ctx = ext_client_ctx(&caller, ip, user_agent_from_headers(&headers));
    let response_client_id = caller.client_id.clone();
    match state
        .org_service
        .create_org(
            caller,
            herald_core::domain::organization::CreateOrgDto {
                external_id: req.external_id.trim().to_string(),
                name: req
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string),
                owner_user_id: owner_id,
            },
            ctx,
        )
        .await
    {
        Ok(outcome) => {
            let status = if outcome.created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            };
            let org = outcome.organization;
            (
                status,
                Json(OrgExtResponse {
                    id: org.id.to_string(),
                    external_id: org.external_id,
                    name: org.name,
                    owner_id: org.owner_user_id.to_string(),
                    realm_id: org.realm_id,
                    client_id: response_client_id,
                    created_at: org.created_at.to_rfc3339(),
                    created: outcome.created,
                }),
            )
                .into_response()
        }
        Err(e) => org_error_response(e),
    }
}

/// Add or change an organization member (idempotent upsert)
///
/// `(orgId, userId)` is unique: adding an existing member with the same tier
/// is a no-op, a different tier updates it. Granting the `org-owner` tier
/// performs an atomic ownership transfer (the previous owner lands on
/// `org-admin`). With `actorUserId` the change additionally passes that
/// user's in-org management gate, self-hold check, and (for transfers) the
/// current-owner requirement.
#[utoipa::path(
    post,
    path = "/api/ext/realms/{realmId}/orgs/{orgId}/members",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("orgId" = String, Path, description = "Organization ID")
    ),
    request_body = UpsertOrgMemberExtRequest,
    responses(
        (status = 200, description = "Membership created or updated", body = OrgMemberExtResponse),
        (status = 400, description = "Validation error / unknown tier / cross-realm account", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Key or policy gate, actor gates (not a manager / self-hold / not the owner)", body = ErrorResponse),
        (status = 404, description = "Organization or target/actor account not found", body = ErrorResponse),
        (status = 409, description = "Owner or last-admin protection", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn upsert_org_member(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, org_id)): Path<(String, String)>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<UpsertOrgMemberExtRequest>,
) -> Response {
    let mut caller = match resolve_org_caller(&state, &identity, &realm_id, true).await {
        Ok(caller) => caller,
        Err(resp) => return resp,
    };

    if !trimmed_non_empty(&req.role, NAME_MAX) {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }
    let org_uuid = match parse_uuid(&org_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let user_id = match parse_uuid(&req.user_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let actor_user_id = match resolve_actor(&mut caller, req.actor_user_id.as_deref()) {
        Ok(actor) => actor,
        Err(resp) => return resp,
    };

    let ctx = ext_client_ctx(&caller, ip, user_agent_from_headers(&headers));
    match state
        .org_service
        .upsert_member(
            caller,
            org_uuid,
            herald_core::domain::organization::UpsertMemberDto {
                user_id,
                role: req.role.trim().to_string(),
                actor_user_id,
            },
            ctx,
        )
        .await
    {
        Ok(view) => Json(OrgMemberExtResponse {
            org_id: view.org_id.to_string(),
            user_id: view.user_id.to_string(),
            role: OrgRoleRef {
                id: view.tier.role_id.to_string(),
                name: view.tier.name,
            },
            is_owner: view.is_owner,
            created_at: view.created_at.to_rfc3339(),
            updated_at: view.updated_at.to_rfc3339(),
        })
        .into_response(),
        Err(e) => org_error_response(e),
    }
}

/// Remove an organization member
///
/// Removal takes effect immediately on the check path (explicit cache
/// invalidation). The owner and the last admin-tier member are protected
/// (409); transfer ownership or grant another admin first.
#[utoipa::path(
    delete,
    path = "/api/ext/realms/{realmId}/orgs/{orgId}/members/{userId}",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("orgId" = String, Path, description = "Organization ID"),
        ("userId" = String, Path, description = "Member account ID"),
        ("actorUserId" = Option<String>, Query, description = "User the app acts on behalf of")
    ),
    responses(
        (status = 204, description = "Member removed"),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Key or policy gate, actor not a manager", body = ErrorResponse),
        (status = 404, description = "Organization or membership not found", body = ErrorResponse),
        (status = 409, description = "Owner or last-admin protection", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn remove_org_member(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, org_id, user_id)): Path<(String, String, String)>,
    Query(query): Query<RemoveOrgMemberExtQuery>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Response {
    let mut caller = match resolve_org_caller(&state, &identity, &realm_id, true).await {
        Ok(caller) => caller,
        Err(resp) => return resp,
    };

    let org_uuid = match parse_uuid(&org_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let user_uuid = match parse_uuid(&user_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let actor_user_id = match resolve_actor(&mut caller, query.actor_user_id.as_deref()) {
        Ok(actor) => actor,
        Err(resp) => return resp,
    };

    let ctx = ext_client_ctx(&caller, ip, user_agent_from_headers(&headers));
    match state
        .org_service
        .remove_member(caller, org_uuid, user_uuid, actor_user_id, ctx)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => org_error_response(e),
    }
}

/// Check a user's capabilities in an organization
///
/// Boolean judgment for the per-request pattern: every rule must hit for
/// `allowed = true`. Non-members get an explicit `member = false`; a
/// disabled member keeps `member = true` with `allowed = false`. Rejections
/// are structured 200 results, matching the ext permission-check contract.
#[utoipa::path(
    post,
    path = "/api/ext/realms/{realmId}/orgs/{orgId}/permission-check",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("orgId" = String, Path, description = "Organization ID")
    ),
    request_body = OrgPermissionCheckExtRequest,
    responses(
        (status = 200, description = "Check result (denials are structured 200 bodies)", body = OrgPermissionCheckResponse),
        (status = 400, description = "Missing/empty rules or validation error", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Key or policy gate", body = ErrorResponse),
        (status = 404, description = "Organization not found in caller scope", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn check_org_permission(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, org_id)): Path<(String, String)>,
    Json(req): Json<OrgPermissionCheckExtRequest>,
) -> Response {
    let caller = match resolve_org_caller(&state, &identity, &realm_id, false).await {
        Ok(caller) => caller,
        Err(resp) => return resp,
    };

    // A missing or empty rules array is a request-contract error, never a
    // vacuous allowed=true (same contract as POST /api/ext/permission/check).
    let Some(rules) = req.rules.filter(|rules| !rules.is_empty()) else {
        return ApiError::bad_request_json(crate::permission::RulesRequiredBody {
            code: "rules_required",
            message: "Organization permission check requires a non-empty rules array",
        })
        .into_response();
    };

    if rules.iter().any(|rule| {
        !trimmed_non_empty(&rule.resource, NAME_MAX) || !trimmed_non_empty(&rule.action, NAME_MAX)
    }) {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }

    let user_uuid = match parse_uuid(&req.user_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let org_uuid = match parse_uuid(&org_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let org = match state.org_service.find_org(&caller, org_uuid).await {
        Ok(org) => org,
        Err(e) => return org_error_response(e),
    };

    let policies: Vec<(String, String)> = rules
        .into_iter()
        .map(|rule| {
            (
                rule.resource.trim().to_string(),
                rule.action.trim().to_string(),
            )
        })
        .collect();

    match state.org_service.check(&org, user_uuid, &policies).await {
        Ok(outcome) => {
            let response = match outcome {
                OrgCheckOutcome::NonMember => OrgPermissionCheckResponse {
                    member: false,
                    allowed: false,
                    role: None,
                },
                OrgCheckOutcome::MemberDenied => OrgPermissionCheckResponse {
                    member: true,
                    allowed: false,
                    role: None,
                },
                OrgCheckOutcome::Member { allowed, tier } => OrgPermissionCheckResponse {
                    member: true,
                    allowed,
                    role: Some(OrgRoleRef {
                        id: tier.role_id.to_string(),
                        name: tier.name,
                    }),
                },
            };
            Json(response).into_response()
        }
        Err(e) => org_error_response(e),
    }
}

/// Get a member's tier in an organization
///
/// Factual read (no rule evaluation, no account-status gating): a member
/// gets the tier label and joined-at timestamp; anyone else gets
/// `member = false`.
#[utoipa::path(
    get,
    path = "/api/ext/realms/{realmId}/orgs/{orgId}/members/{userId}",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("orgId" = String, Path, description = "Organization ID"),
        ("userId" = String, Path, description = "Member account ID")
    ),
    responses(
        (status = 200, description = "Tier label or explicit non-member result", body = GetOrgMemberResponse),
        (status = 400, description = "Invalid UUID format", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Key or policy gate", body = ErrorResponse),
        (status = 404, description = "Organization not found in caller scope", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn get_org_member(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, org_id, user_id)): Path<(String, String, String)>,
) -> Response {
    let caller = match resolve_org_caller(&state, &identity, &realm_id, false).await {
        Ok(caller) => caller,
        Err(resp) => return resp,
    };

    let org_uuid = match parse_uuid(&org_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let user_uuid = match parse_uuid(&user_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let org = match state.org_service.find_org(&caller, org_uuid).await {
        Ok(org) => org,
        Err(e) => return org_error_response(e),
    };

    match state.org_service.get_member(org.id, user_uuid).await {
        Ok(Some(detail)) => Json(GetOrgMemberResponse {
            member: true,
            user_id: detail.user_id.to_string(),
            role: Some(OrgRoleRef {
                id: detail.role_id.to_string(),
                name: detail.role_name,
            }),
            is_owner: Some(org.owner_user_id == user_uuid),
            joined_at: Some(detail.created_at.to_rfc3339()),
        })
        .into_response(),
        Ok(None) => Json(GetOrgMemberResponse {
            member: false,
            user_id: user_uuid.to_string(),
            role: None,
            is_owner: None,
            joined_at: None,
        })
        .into_response(),
        Err(e) => org_error_response(e),
    }
}

/// List an organization's members
///
/// Paginated (page >= 1 default 1, pageSize 1-100 default 20; out-of-range
/// values are rejected with 400). Rows carry account display fields so the
/// app can render a member list without per-user lookups.
#[utoipa::path(
    get,
    path = "/api/ext/realms/{realmId}/orgs/{orgId}/members",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("orgId" = String, Path, description = "Organization ID"),
        ("page" = Option<u64>, Query, description = "Page number (1-based, default 1)"),
        ("pageSize" = Option<u64>, Query, description = "Page size (1-100, default 20)")
    ),
    responses(
        (status = 200, description = "Member list", body = OrgMemberListResponse),
        (status = 400, description = "Invalid pagination parameters", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Key or policy gate", body = ErrorResponse),
        (status = 404, description = "Organization not found in caller scope", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn list_org_members(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, org_id)): Path<(String, String)>,
    Query(query): Query<ListOrgMembersExtQuery>,
) -> Response {
    let caller = match resolve_org_caller(&state, &identity, &realm_id, false).await {
        Ok(caller) => caller,
        Err(resp) => return resp,
    };

    let (page, page_size) = match parse_page(query.page, query.page_size) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let org_uuid = match parse_uuid(&org_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let org = match state.org_service.find_org(&caller, org_uuid).await {
        Ok(org) => org,
        Err(e) => return org_error_response(e),
    };

    match state
        .org_service
        .list_members(org.id, page, page_size)
        .await
    {
        Ok((items, total)) => Json(OrgMemberListResponse {
            items: items
                .into_iter()
                .map(|detail| OrgMemberListItem {
                    user_id: detail.user_id.to_string(),
                    email: detail.email,
                    nickname: detail.nickname,
                    role: OrgRoleRef {
                        id: detail.role_id.to_string(),
                        name: detail.role_name,
                    },
                    is_owner: org.owner_user_id == detail.user_id,
                    joined_at: detail.created_at.to_rfc3339(),
                })
                .collect(),
            page,
            page_size,
            total,
        })
        .into_response(),
        Err(e) => org_error_response(e),
    }
}

/// List a user's organizations in the caller's Client App
///
/// Every organization of the user under the calling app, with the user's
/// tier in each. Deleted organizations drop out (membership cascade).
#[utoipa::path(
    get,
    path = "/api/ext/realms/{realmId}/orgs",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("userId" = String, Query, description = "User account ID"),
        ("page" = Option<u64>, Query, description = "Page number (1-based, default 1)"),
        ("pageSize" = Option<u64>, Query, description = "Page size (1-100, default 20)")
    ),
    responses(
        (status = 200, description = "User's organization list", body = UserOrgListResponse),
        (status = 400, description = "Missing/invalid userId or pagination", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Key or policy gate", body = ErrorResponse),
        (status = 404, description = "User not found in the realm", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn list_user_orgs(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Query(query): Query<ListUserOrgsExtQuery>,
) -> Response {
    let caller = match resolve_org_caller(&state, &identity, &realm_id, false).await {
        Ok(caller) => caller,
        Err(resp) => return resp,
    };

    let (page, page_size) = match parse_page(query.page, query.page_size) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let user_uuid = match query.user_id.as_deref() {
        Some(raw) if !raw.trim().is_empty() => match parse_uuid(raw) {
            Ok(id) => id,
            Err(resp) => return resp,
        },
        _ => return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError),
    };

    match state
        .org_service
        .list_user_orgs(&caller, user_uuid, page, page_size)
        .await
    {
        Ok((items, total)) => Json(UserOrgListResponse {
            items: items
                .into_iter()
                .map(|detail| UserOrgListItem {
                    org_id: detail.org_id.to_string(),
                    external_id: detail.external_id,
                    name: detail.name,
                    role: OrgRoleRef {
                        id: detail.role_id.to_string(),
                        name: detail.role_name,
                    },
                    is_owner: detail.is_owner,
                    joined_at: detail.created_at.to_rfc3339(),
                })
                .collect(),
            page,
            page_size,
            total,
        })
        .into_response(),
        Err(e) => org_error_response(e),
    }
}

/// Delete an organization
///
/// Deletes the organization; every membership cascades away and the check
/// path answers 404 immediately after. Repeating the delete answers 404.
#[utoipa::path(
    delete,
    path = "/api/ext/realms/{realmId}/orgs/{orgId}",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("orgId" = String, Path, description = "Organization ID")
    ),
    responses(
        (status = 204, description = "Organization deleted"),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Key or policy gate", body = ErrorResponse),
        (status = 404, description = "Organization not found in caller scope", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn delete_org(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, org_id)): Path<(String, String)>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Response {
    let caller = match resolve_org_caller(&state, &identity, &realm_id, true).await {
        Ok(caller) => caller,
        Err(resp) => return resp,
    };

    let org_uuid = match parse_uuid(&org_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let ctx = ext_client_ctx(&caller, ip, user_agent_from_headers(&headers));
    match state.org_service.delete_org(caller, org_uuid, ctx).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => org_error_response(e),
    }
}
