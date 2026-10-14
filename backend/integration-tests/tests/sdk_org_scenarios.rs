// =============================================================================
// SDK Organization Scenarios
// =============================================================================
//
// Test the organization authorization ext API through the SDK: lifecycle,
// membership management, instance-level checks, security invariants
// (layer/cross-org isolation, in-org anti-escalation, owner and last-admin
// protection), scope probing, status linkage, cascades, and the audit trail.
//
// The organization face requires a Client-App-bound API key; every test
// provisions keys and users directly in the schema-scoped database.
//
// =============================================================================

use herald_sdk::{
    Client, CreateOrgSdkRequest, Error, OrgCheckRule, OrgPermissionCheckSdkRequest,
    UpsertOrgMemberSdkRequest,
};
use herald_test_support::SchemaTestContext;
use sqlx::query;
use test_context::test_context;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Assert an SDK error carries the expected HTTP status and error code
/// (the code travels inside the JSON error body).
fn expect_error<T>(result: Result<T, Error>, status: u16, code: &str) {
    let err = result
        .err()
        .unwrap_or_else(|| panic!("expected error {code}, got success"));
    let (actual_status, text) = match err {
        Error::Unauthorized(text) => (401, text),
        Error::Forbidden(text) => (403, text),
        Error::NotFound(text) => (404, text),
        Error::ApiError { status, message } => (status, message),
        other => panic!("unexpected error kind for {code}: {other:?}"),
    };
    assert_eq!(
        actual_status, status,
        "expected status {status} for {code}, got {actual_status}: {text}"
    );
    assert!(
        text.contains(code),
        "error body must carry code {code}, got: {text}"
    );
}

async fn start_test_server(ctx: &SchemaTestContext) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    let router = ctx.create_unified_test_router();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    (base_url, handle)
}

/// The schema's default test client app id (orgs bind to it).
async fn test_client_app_id(ctx: &SchemaTestContext) -> Uuid {
    let (id,): (Uuid,) =
        sqlx::query_as("SELECT id FROM client_app WHERE realm_id = $1 AND client_id = $2")
            .bind(&ctx._realm_id)
            .bind(&ctx._client_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("default test client app must exist");
    id
}

/// A Client-App-bound API key with the given flat policies granted to its
/// api_key principal. `orgs.manage` alone also covers reads through the
/// manage→view action hierarchy.
async fn setup_bound_key(
    ctx: &SchemaTestContext,
    permissions: &[(&str, &str)],
) -> (String, String, Uuid) {
    let app_id = test_client_app_id(ctx).await;
    let (plaintext, entity) = herald_test_support::helpers::create_test_client_app_api_key(
        ctx,
        "org-test-bound-key",
        true,
        None,
        app_id,
    )
    .await;
    herald_test_support::helpers::grant_api_key_permissions(
        &ctx.app_state.pool,
        &ctx._realm_id,
        &ctx._client_id,
        &entity.id,
        permissions,
    )
    .await;
    (plaintext, entity.id, app_id)
}

// A plain realm account with no roles (an organization member candidate)
// is provisioned by `herald_test_support::helpers::create_test_user`.

/// An account in a foreign realm (cross-realm negative cases).
async fn create_foreign_realm_user(ctx: &SchemaTestContext, email: &str) -> Uuid {
    let id = Uuid::now_v7();
    query("INSERT INTO account (id, realm_id, email, password, status) VALUES ($1, $2, $3, $4, 1)")
        .bind(id)
        .bind("foreign-realm-for-org-tests")
        .bind(email)
        .bind("$2a$12$dummy_password_hash")
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create foreign realm user");
    id
}

/// Resolve a tier's role id within (realm, default test client_id).
async fn tier_role_id(ctx: &SchemaTestContext, tier_name: &str) -> Uuid {
    let (id,): (Uuid,) =
        sqlx::query_as("SELECT id FROM roles WHERE name = $1 AND realm_id = $2 AND client_id = $3")
            .bind(tier_name)
            .bind(&ctx._realm_id)
            .bind(&ctx._client_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap_or_else(|e| panic!("tier {tier_name} must exist after org creation: {e}"));
    id
}

/// Add policies to a tier's vocabulary. Direct SQL mirrors what a realm
/// admin would configure through the existing role-definition surface; call
/// before the first check on the tier so the short policy cache starts cold.
async fn grant_tier_policies(ctx: &SchemaTestContext, tier_name: &str, policies: &[(&str, &str)]) {
    let role_id = tier_role_id(ctx, tier_name).await;
    for (resource, action) in policies {
        query(
            "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(role_id)
        .bind(&ctx._realm_id)
        .bind(resource)
        .bind(action)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to grant tier policy");
    }
}

/// Create a custom (non-builtin) tier, as an app/realm admin would through
/// the existing role-definition surface. Returns the new role id.
async fn create_custom_tier(
    ctx: &SchemaTestContext,
    name: &str,
    policies: &[(&str, &str)],
) -> Uuid {
    let role_id = Uuid::now_v7();
    query(
        "INSERT INTO roles (id, name, realm_id, client_id, is_builtin)
         VALUES ($1, $2, $3, $4, false)",
    )
    .bind(role_id)
    .bind(name)
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create custom tier");
    for (resource, action) in policies {
        query(
            "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(role_id)
        .bind(&ctx._realm_id)
        .bind(resource)
        .bind(action)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to grant custom tier policy");
    }
    role_id
}

fn check_rules(pairs: &[(&str, &str)]) -> OrgPermissionCheckSdkRequest {
    OrgPermissionCheckSdkRequest {
        user_id: String::new(),
        rules: pairs
            .iter()
            .map(|(resource, action)| OrgCheckRule {
                resource: resource.to_string(),
                action: action.to_string(),
            })
            .collect(),
    }
}

/// Create an org through the SDK and return (org_id, owner_id).
async fn create_org_with_owner(
    client: &Client,
    ctx: &SchemaTestContext,
    external_id: &str,
    owner: Uuid,
) -> String {
    let org = client
        .create_org(
            &ctx._realm_id,
            CreateOrgSdkRequest {
                external_id: external_id.to_string(),
                name: Some(format!("org-{external_id}")),
                owner_id: owner.to_string(),
            },
        )
        .await
        .expect("org creation should succeed");
    org.id
}

// ---------------------------------------------------------------------------
// 1. Full lifecycle closed loop (create → member → check → change → remove)
// ---------------------------------------------------------------------------

// Covers the full app-backend loop with a single bound key + orgs.manage
// (manage covers view through the action hierarchy): create → add member →
// check → change tier → remove, with no admin console involvement; a
// non-member is an explicit result; removal is effective on the very next
// check (explicit cache invalidation).

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_full_lifecycle_closed_loop(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-lifecycle@test.com").await;
    let member =
        herald_test_support::helpers::create_test_user(ctx, "org-member-lifecycle@test.com").await;

    // Non-member before the org even exists → explicit member:false.
    let mut req = check_rules(&[("article", "view")]);
    req.user_id = member.to_string();
    let org_id = create_org_with_owner(&client, ctx, "ws-lifecycle", owner).await;

    let result = client
        .check_org_permission(&ctx._realm_id, &org_id, req)
        .await
        .expect("non-member check must be a structured 200");
    assert!(!result.member && !result.allowed && result.role.is_none());

    grant_tier_policies(ctx, "org-member", &[("article", "view")]).await;
    grant_tier_policies(ctx, "org-admin", &[("article", "manage")]).await;

    // Add member and verify tier + rule-resolved judgment.
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            UpsertOrgMemberSdkRequest {
                user_id: member.to_string(),
                role: "org-member".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .expect("member upsert should succeed");

    let lookup = client
        .get_org_member(&ctx._realm_id, &org_id, &member.to_string())
        .await
        .expect("member lookup should succeed");
    assert!(lookup.member);
    assert_eq!(
        lookup.role.as_ref().map(|r| r.name.as_str()),
        Some("org-member")
    );
    assert!(!lookup.is_owner.unwrap());

    let mut allowed_req = check_rules(&[("article", "view")]);
    allowed_req.user_id = member.to_string();
    let result = client
        .check_org_permission(&ctx._realm_id, &org_id, allowed_req)
        .await
        .unwrap();
    assert!(
        result.member && result.allowed,
        "member with article.view must pass: {result:?}"
    );

    let mut denied_req = check_rules(&[("article", "manage")]);
    denied_req.user_id = member.to_string();
    let result = client
        .check_org_permission(&ctx._realm_id, &org_id, denied_req)
        .await
        .unwrap();
    assert!(
        result.member && !result.allowed,
        "org-member must not pass article.manage"
    );

    // Change the tier and verify the judgment follows.
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            UpsertOrgMemberSdkRequest {
                user_id: member.to_string(),
                role: "org-admin".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .expect("tier change should succeed");

    let mut admin_req = check_rules(&[("article", "manage")]);
    admin_req.user_id = member.to_string();
    let result = client
        .check_org_permission(&ctx._realm_id, &org_id, admin_req)
        .await
        .unwrap();
    assert!(
        result.member && result.allowed,
        "org-admin must pass article.manage"
    );

    // Remove → the very next check reads non-member (no perceivable window).
    client
        .remove_org_member(&ctx._realm_id, &org_id, &member.to_string(), None)
        .await
        .expect("member removal should succeed");

    let mut req = check_rules(&[("article", "view")]);
    req.user_id = member.to_string();
    let result = client
        .check_org_permission(&ctx._realm_id, &org_id, req)
        .await
        .unwrap();
    assert!(
        !result.member,
        "removed member must read as non-member immediately: {result:?}"
    );

    let lookup = client
        .get_org_member(&ctx._realm_id, &org_id, &member.to_string())
        .await
        .unwrap();
    assert!(!lookup.member);

    handle.abort();
}

// ---------------------------------------------------------------------------
// 2. Idempotency (org create replay + member upsert replay)
// ---------------------------------------------------------------------------

// Covers US-OA-001 scenario 2 and US-OA-002 scenario 2: replaying the same
// externalId returns the existing org (created:false, no second row, replay
// payload ignored); member upsert is idempotent per role and updates on
// role change without duplicating rows.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_create_and_member_upsert_idempotent(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-idem@test.com").await;
    let member =
        herald_test_support::helpers::create_test_user(ctx, "org-member-idem@test.com").await;

    let first = client
        .create_org(
            &ctx._realm_id,
            CreateOrgSdkRequest {
                external_id: "ws-idem".to_string(),
                name: Some("original-name".to_string()),
                owner_id: owner.to_string(),
            },
        )
        .await
        .unwrap();
    assert!(first.created);

    let replay = client
        .create_org(
            &ctx._realm_id,
            CreateOrgSdkRequest {
                external_id: "ws-idem".to_string(),
                name: Some("replayed-name-ignored".to_string()),
                owner_id: owner.to_string(),
            },
        )
        .await
        .unwrap();
    assert!(!replay.created, "replay must report created:false");
    assert_eq!(first.id, replay.id);
    assert_eq!(
        replay.name.as_deref(),
        Some("original-name"),
        "replay must not update the org"
    );

    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM organizations WHERE external_id = 'ws-idem'")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        count, 1,
        "no second organization row for the same externalId"
    );

    let upsert = |role: &str| UpsertOrgMemberSdkRequest {
        user_id: member.to_string(),
        role: role.to_string(),
        actor_user_id: None,
    };
    client
        .upsert_org_member(&ctx._realm_id, &first.id, upsert("org-member"))
        .await
        .unwrap();
    client
        .upsert_org_member(&ctx._realm_id, &first.id, upsert("org-member"))
        .await
        .unwrap();
    let (rows,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM org_memberships WHERE org_id = $1 AND user_id = $2")
            .bind(Uuid::parse_str(&first.id).unwrap())
            .bind(member)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        rows, 1,
        "same-role replay must not duplicate the membership"
    );

    client
        .upsert_org_member(&ctx._realm_id, &first.id, upsert("org-admin"))
        .await
        .unwrap();
    let lookup = client
        .get_org_member(&ctx._realm_id, &first.id, &member.to_string())
        .await
        .unwrap();
    assert_eq!(
        lookup.role.as_ref().map(|r| r.name.as_str()),
        Some("org-admin"),
        "role difference must not be silently swallowed"
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// 3. Instance-level tiers (same user, different orgs)
// ---------------------------------------------------------------------------

// Covers US-OA-003 scenario 1: the same user holds different tiers in two
// organizations and each judgment follows that organization's tier only.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_instance_level_tiers(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let user =
        herald_test_support::helpers::create_test_user(ctx, "org-user-instance@test.com").await;
    let other_owner =
        herald_test_support::helpers::create_test_user(ctx, "org-other-owner-instance@test.com")
            .await;

    // The first org creation seeds the built-in tiers; configure the tier
    // vocabularies after that (and before any check, so the policy cache
    // starts cold).
    let org1 = create_org_with_owner(&client, ctx, "ws-instance-1", user).await;
    grant_tier_policies(ctx, "org-owner", &[("article", "manage")]).await;
    grant_tier_policies(ctx, "org-member", &[("article", "view")]).await;
    let org2 = create_org_with_owner(&client, ctx, "ws-instance-2", other_owner).await;
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org2,
            UpsertOrgMemberSdkRequest {
                user_id: user.to_string(),
                role: "org-member".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .unwrap();

    let mut manage_req = check_rules(&[("article", "manage")]);
    manage_req.user_id = user.to_string();
    let in_org1 = client
        .check_org_permission(&ctx._realm_id, &org1, manage_req.clone())
        .await
        .unwrap();
    let in_org2 = client
        .check_org_permission(&ctx._realm_id, &org2, manage_req)
        .await
        .unwrap();
    assert!(in_org1.allowed, "owner tier in org1 grants article.manage");
    assert!(
        !in_org2.allowed,
        "member tier in org2 denies article.manage"
    );
    assert_eq!(
        in_org1.role.as_ref().map(|r| r.name.as_str()),
        Some("org-owner")
    );
    assert_eq!(
        in_org2.role.as_ref().map(|r| r.name.as_str()),
        Some("org-member")
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// 4. Layer isolation (platform identity does not pierce org checks)
// ---------------------------------------------------------------------------

// Covers US-OA-006 scenario 1: a user holding a flat platform-side role is
// a non-member of the organization — flat user_roles never join org checks.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_layer_isolation(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-layer@test.com").await;
    // Flat platform identity (e.g. a realm-admin-style grant), no org membership.
    let (platform_user, _token) = herald_test_support::helpers::create_test_user_with_permissions(
        ctx,
        "org-platform-identity@test.com",
        &[("users", "manage"), ("roles", "manage")],
    )
    .await;

    let org_id = create_org_with_owner(&client, ctx, "ws-layer", owner).await;

    let mut req = check_rules(&[("users", "manage")]);
    req.user_id = platform_user;
    let result = client
        .check_org_permission(&ctx._realm_id, &org_id, req)
        .await
        .unwrap();
    assert!(
        !result.member && !result.allowed,
        "platform identity must read as non-member: {result:?}"
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// 5. Cross-org isolation
// ---------------------------------------------------------------------------

// Covers US-OA-006 scenario 2: an admin of organization A is a non-member
// of organization B.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_cross_org_isolation(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let owner_a =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-a-isolation@test.com").await;
    let owner_b =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-b-isolation@test.com").await;

    let org_a = create_org_with_owner(&client, ctx, "ws-iso-a", owner_a).await;
    let org_b = create_org_with_owner(&client, ctx, "ws-iso-b", owner_b).await;

    let mut req = check_rules(&[("article", "view")]);
    req.user_id = owner_a.to_string();
    let result = client
        .check_org_permission(&ctx._realm_id, &org_b, req)
        .await
        .unwrap();
    assert!(
        !result.member,
        "org A's owner must be a non-member of org B: {result:?}"
    );

    // Sanity: the same user is the owner in org A.
    let lookup = client
        .get_org_member(&ctx._realm_id, &org_a, &owner_a.to_string())
        .await
        .unwrap();
    assert!(lookup.member && lookup.is_owner.unwrap());

    handle.abort();
}

// ---------------------------------------------------------------------------
// 6. Actor gates (US-OA-005: collaboration + in-org anti-escalation)
// ---------------------------------------------------------------------------

// Covers US-OA-005 scenarios 1+2 and the in-org anti-escalation rules:
// a non-manager actor is rejected; a manager must self-hold the granted
// tier's vocabulary; only the current owner may transfer ownership; an
// owner-actor promotion succeeds.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_actor_gates_and_anti_escalation(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-gates@test.com").await;
    let admin =
        herald_test_support::helpers::create_test_user(ctx, "org-admin-gates@test.com").await;
    let member =
        herald_test_support::helpers::create_test_user(ctx, "org-plain-gates@test.com").await;
    let target =
        herald_test_support::helpers::create_test_user(ctx, "org-target-gates@test.com").await;

    let org_id = create_org_with_owner(&client, ctx, "ws-gates", owner).await;

    let upsert = |user: Uuid, role: &str, actor: Option<Uuid>| UpsertOrgMemberSdkRequest {
        user_id: user.to_string(),
        role: role.to_string(),
        actor_user_id: actor.map(|id| id.to_string()),
    };

    // Non-manager actor: the plain member is rejected outright.
    expect_error(
        client
            .upsert_org_member(
                &ctx._realm_id,
                &org_id,
                upsert(target, "org-member", Some(member)),
            )
            .await
            .map(|_| ()),
        403,
        "org_actor_not_manager",
    );

    // Owner-actor promotion succeeds while vocabularies are empty
    // (vacuous self-hold): US-OA-005 scenario 1.
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            upsert(member, "org-admin", Some(owner)),
        )
        .await
        .expect("owner acting on behalf must be able to promote a member");

    // Now give the tiers vocabularies and exercise the self-hold gate.
    grant_tier_policies(ctx, "org-admin", &[("article", "manage")]).await;
    create_custom_tier(
        ctx,
        "editor",
        &[("article", "manage"), ("article", "delete")],
    )
    .await;

    client
        .upsert_org_member(&ctx._realm_id, &org_id, upsert(admin, "org-admin", None))
        .await
        .unwrap();

    // org-admin holds article.manage but not article.delete → self-hold fails.
    expect_error(
        client
            .upsert_org_member(
                &ctx._realm_id,
                &org_id,
                upsert(target, "editor", Some(admin)),
            )
            .await
            .map(|_| ()),
        403,
        "org_actor_self_hold_required",
    );

    // Complete the admin's vocabulary → the same grant passes.
    grant_tier_policies(ctx, "org-admin", &[("article", "delete")]).await;
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            upsert(target, "editor", Some(admin)),
        )
        .await
        .expect("admin holding the full vocabulary must be able to grant it");
    let lookup = client
        .get_org_member(&ctx._realm_id, &org_id, &target.to_string())
        .await
        .unwrap();
    assert_eq!(
        lookup.role.as_ref().map(|r| r.name.as_str()),
        Some("editor")
    );

    // Ownership transfer gate: a non-owner admin cannot confer org-owner.
    expect_error(
        client
            .upsert_org_member(
                &ctx._realm_id,
                &org_id,
                upsert(target, "org-owner", Some(admin)),
            )
            .await
            .map(|_| ()),
        403,
        "org_ownership_transfer_denied",
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// 7. Owner and last-admin protection
// ---------------------------------------------------------------------------

// Covers US-OA-004 scenarios 2+3: the owner cannot be removed or demoted
// (409, transfer is the only path); removing an admin while the owner
// remains succeeds; the last-admin guard rejects changes that would leave
// no admin-tier member at all.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_owner_and_last_admin_protection(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-protect@test.com").await;
    let admin =
        herald_test_support::helpers::create_test_user(ctx, "org-admin-protect@test.com").await;

    let org_id = create_org_with_owner(&client, ctx, "ws-protect", owner).await;
    let org_uuid = Uuid::parse_str(&org_id).unwrap();

    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            UpsertOrgMemberSdkRequest {
                user_id: admin.to_string(),
                role: "org-admin".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .unwrap();

    // Owner protection: removal and demotion are both rejected.
    expect_error(
        client
            .remove_org_member(&ctx._realm_id, &org_id, &owner.to_string(), None)
            .await,
        409,
        "org_owner_protected",
    );
    expect_error(
        client
            .upsert_org_member(
                &ctx._realm_id,
                &org_id,
                UpsertOrgMemberSdkRequest {
                    user_id: owner.to_string(),
                    role: "org-member".to_string(),
                    actor_user_id: None,
                },
            )
            .await
            .map(|_| ()),
        409,
        "org_owner_protected",
    );

    // Removing an admin while the owner (an admin-tier member) remains is fine.
    client
        .remove_org_member(&ctx._realm_id, &org_id, &admin.to_string(), None)
        .await
        .expect("admin removal with owner present must succeed");

    // Last-admin guard: with the owner's membership anomalously absent (the
    // only state where no other admin-tier member remains), removing or
    // downgrading the last admin must be rejected with a transfer hint.
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            UpsertOrgMemberSdkRequest {
                user_id: admin.to_string(),
                role: "org-admin".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .unwrap();
    query("DELETE FROM org_memberships WHERE org_id = $1 AND user_id = $2")
        .bind(org_uuid)
        .bind(owner)
        .execute(&ctx.app_state.pool)
        .await
        .expect("anomalous-state setup must run");

    expect_error(
        client
            .remove_org_member(&ctx._realm_id, &org_id, &admin.to_string(), None)
            .await,
        409,
        "org_last_admin_protected",
    );
    expect_error(
        client
            .upsert_org_member(
                &ctx._realm_id,
                &org_id,
                UpsertOrgMemberSdkRequest {
                    user_id: admin.to_string(),
                    role: "org-member".to_string(),
                    actor_user_id: None,
                },
            )
            .await
            .map(|_| ()),
        409,
        "org_last_admin_protected",
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// 8. Atomic ownership transfer
// ---------------------------------------------------------------------------

// Covers US-OA-004: granting org-owner transfers ownership atomically — the
// new owner holds the owner tier, the previous owner lands on org-admin,
// and the owner pointer flips. The new owner (as actor) can transfer on.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_ownership_transfer(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-transfer@test.com").await;
    let successor =
        herald_test_support::helpers::create_test_user(ctx, "org-successor-transfer@test.com")
            .await;
    let third =
        herald_test_support::helpers::create_test_user(ctx, "org-third-transfer@test.com").await;

    let org_id = create_org_with_owner(&client, ctx, "ws-transfer", owner).await;
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            UpsertOrgMemberSdkRequest {
                user_id: successor.to_string(),
                role: "org-member".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .unwrap();

    let view = client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            UpsertOrgMemberSdkRequest {
                user_id: successor.to_string(),
                role: "org-owner".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .expect("ownership transfer should succeed");
    assert!(view.is_owner);

    let old_owner_lookup = client
        .get_org_member(&ctx._realm_id, &org_id, &owner.to_string())
        .await
        .unwrap();
    assert_eq!(
        old_owner_lookup.role.as_ref().map(|r| r.name.as_str()),
        Some("org-admin"),
        "previous owner lands on org-admin"
    );
    assert!(!old_owner_lookup.is_owner.unwrap());

    let new_owner_lookup = client
        .get_org_member(&ctx._realm_id, &org_id, &successor.to_string())
        .await
        .unwrap();
    assert_eq!(
        new_owner_lookup.role.as_ref().map(|r| r.name.as_str()),
        Some("org-owner")
    );
    assert!(new_owner_lookup.is_owner.unwrap());

    let (owner_row,): (Uuid,) =
        sqlx::query_as("SELECT owner_user_id FROM organizations WHERE id = $1")
            .bind(Uuid::parse_str(&org_id).unwrap())
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
    assert_eq!(owner_row, successor);

    // The new owner can transfer onward acting as themselves.
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            UpsertOrgMemberSdkRequest {
                user_id: third.to_string(),
                role: "org-owner".to_string(),
                actor_user_id: Some(successor.to_string()),
            },
        )
        .await
        .expect("current owner as actor must be able to transfer ownership");

    handle.abort();
}

// ---------------------------------------------------------------------------
// 9. Scope, probing and credential gates
// ---------------------------------------------------------------------------

// Covers PRD §6: another app's bound key gets the same 404 whether the org
// exists or not (no existence probing); an unbound key is rejected before
// any org lookup; a key from another realm is walled off.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_scope_and_probing(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url.clone(), api_key, None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-scope@test.com").await;
    let org_id = create_org_with_owner(&client, ctx, "ws-scope", owner).await;

    // A second app in the same realm, with its own bound key.
    let second_app = Uuid::now_v7();
    query(
        r#"INSERT INTO client_app (id, realm_id, client_id, name, redirect_uris, allowed_origins,
             browser_refresh_absolute_ttl_seconds, is_first_party, enabled,
             device_code_grant_enabled, turnstile_enabled, mcp_token_generation)
           VALUES ($1, $2, $3, 'Second App', '[]'::jsonb, '[]'::jsonb, 2592000, false, true, false, false, 0)"#,
    )
    .bind(second_app)
    .bind(&ctx._realm_id)
    .bind(format!("second-app-{}", Uuid::now_v7()))
    .execute(&ctx.app_state.pool)
    .await
    .expect("second client app must be insertable");

    let (foreign_key, foreign_entity) =
        herald_test_support::helpers::create_test_client_app_api_key(
            ctx,
            "org-second-app-key",
            true,
            None,
            second_app,
        )
        .await;
    herald_test_support::helpers::grant_api_key_permissions(
        &ctx.app_state.pool,
        &ctx._realm_id,
        &ctx._client_id,
        &foreign_entity.id,
        &[("orgs", "view")],
    )
    .await;
    let foreign_client = Client::new(base_url.clone(), foreign_key, None);

    // Existing org and nonexistent org answer identically: 404 org_not_found.
    expect_error(
        foreign_client
            .list_org_members(&ctx._realm_id, &org_id)
            .await
            .map(|_| ()),
        404,
        "org_not_found",
    );
    let random_org = Uuid::now_v7().to_string();
    expect_error(
        foreign_client
            .list_org_members(&ctx._realm_id, &random_org)
            .await
            .map(|_| ()),
        404,
        "org_not_found",
    );

    // Unbound key: rejected with the dedicated code before any org lookup.
    let (unbound_key, unbound_entity) =
        herald_test_support::helpers::create_test_api_key(ctx, "org-unbound-key", true, None).await;
    herald_test_support::helpers::grant_api_key_permissions(
        &ctx.app_state.pool,
        &ctx._realm_id,
        &ctx._client_id,
        &unbound_entity.id,
        &[("orgs", "manage")],
    )
    .await;
    let unbound_client = Client::new(base_url.clone(), unbound_key, None);
    expect_error(
        unbound_client
            .create_org(
                &ctx._realm_id,
                CreateOrgSdkRequest {
                    external_id: "ws-unbound".to_string(),
                    name: None,
                    owner_id: owner.to_string(),
                },
            )
            .await
            .map(|_| ()),
        403,
        "org_client_app_key_required",
    );

    // Key from another realm: cross-realm wall.
    let cross_realm_key =
        herald_core::domain::client_api_keys::services::ClientApiKeyService::generate_api_key();
    let cross_realm_hash =
        herald_core::domain::client_api_keys::services::ClientApiKeyService::hash_api_key(
            &cross_realm_key,
        );
    query(
        "INSERT INTO client_api_keys (id, name, api_key_hash, realm_id, client_app_id, enabled)
         VALUES ($1, 'cross-realm-key', $2, 'foreign-realm-for-org-tests', NULL, true)",
    )
    .bind(Uuid::now_v7())
    .bind(&cross_realm_hash)
    .execute(&ctx.app_state.pool)
    .await
    .unwrap();
    let cross_client = Client::new(base_url, cross_realm_key, None);
    expect_error(
        cross_client
            .create_org(
                &ctx._realm_id,
                CreateOrgSdkRequest {
                    external_id: "ws-cross-realm".to_string(),
                    name: None,
                    owner_id: owner.to_string(),
                },
            )
            .await
            .map(|_| ()),
        403,
        "cross_realm_access_forbidden",
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// 10. Account and Client App status linkage
// ---------------------------------------------------------------------------

// Covers PRD §4.2: a disabled member keeps member:true with allowed:false
// (distinct from non-member); a disabled Client App takes its keys' whole
// org face down with 401.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_account_and_client_app_status(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url.clone(), api_key.clone(), None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-status@test.com").await;
    let member =
        herald_test_support::helpers::create_test_user(ctx, "org-member-status@test.com").await;
    let org_id = create_org_with_owner(&client, ctx, "ws-status", owner).await;

    grant_tier_policies(ctx, "org-member", &[("article", "view")]).await;
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            UpsertOrgMemberSdkRequest {
                user_id: member.to_string(),
                role: "org-member".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .unwrap();

    let mut req = check_rules(&[("article", "view")]);
    req.user_id = member.to_string();
    let result = client
        .check_org_permission(&ctx._realm_id, &org_id, req)
        .await
        .unwrap();
    assert!(result.member && result.allowed);

    // Disabled member: member stays true, allowed flips to false.
    query("UPDATE account SET status = 2 WHERE id = $1")
        .bind(member)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    let mut req = check_rules(&[("article", "view")]);
    req.user_id = member.to_string();
    let result = client
        .check_org_permission(&ctx._realm_id, &org_id, req)
        .await
        .unwrap();
    assert!(
        result.member,
        "disabled member is still a member: {result:?}"
    );
    assert!(!result.allowed, "disabled member's checks read as denied");

    // Disabled Client App: the bound key's org face answers 401.
    query("UPDATE client_app SET enabled = false WHERE id = $1")
        .bind(app_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    let client2 = Client::new(base_url, api_key, None);
    expect_error(
        client2
            .list_org_members(&ctx._realm_id, &org_id)
            .await
            .map(|_| ()),
        401,
        "client_app_disabled",
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// 11. Cascades (org delete, member account hard delete)
// ---------------------------------------------------------------------------

// Covers US-OA-007 scenario 2 and PRD §4.1 deletion cascades: deleting an
// org drops it from the user's list and 404s the check path; hard-deleting
// a member account clears the membership row via the FK cascade.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_delete_and_hard_delete_cascades(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-cascade@test.com").await;
    let member =
        herald_test_support::helpers::create_test_user(ctx, "org-member-cascade@test.com").await;
    let org1 = create_org_with_owner(&client, ctx, "ws-cascade-1", owner).await;
    let org2 = create_org_with_owner(&client, ctx, "ws-cascade-2", owner).await;

    client
        .upsert_org_member(
            &ctx._realm_id,
            &org1,
            UpsertOrgMemberSdkRequest {
                user_id: member.to_string(),
                role: "org-member".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .unwrap();
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org2,
            UpsertOrgMemberSdkRequest {
                user_id: member.to_string(),
                role: "org-member".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .unwrap();

    // Org delete cascades memberships; repeat delete answers 404.
    client.delete_org(&ctx._realm_id, &org1).await.unwrap();
    let mut req = check_rules(&[("article", "view")]);
    req.user_id = member.to_string();
    expect_error(
        client
            .check_org_permission(&ctx._realm_id, &org1, req)
            .await
            .map(|_| ()),
        404,
        "org_not_found",
    );
    expect_error(
        client.delete_org(&ctx._realm_id, &org1).await,
        404,
        "org_not_found",
    );

    let list = client
        .list_user_orgs(&ctx._realm_id, &member.to_string())
        .await
        .unwrap();
    assert_eq!(
        list.total, 1,
        "deleted org must drop out of the user's list"
    );
    assert_eq!(list.items[0].org_id, org2);

    // The deletion cascades themselves are FK contracts, and schemas cloned
    // from the template (table LIKE copy) never carry foreign keys — so the
    // ON DELETE rules are asserted against the template schema, where the
    // real migrations ran. Production databases get these rules from the
    // org tables in 0001_core.sql.
    let template_schema = format!("template_test_schema_{}", std::process::id());
    let cascade_rules: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT a.attname AS column_name, con.confdeltype::text AS delete_rule
        FROM pg_constraint con
        JOIN pg_class rel ON rel.oid = con.conrelid
        JOIN pg_namespace ns ON ns.oid = rel.relnamespace
        JOIN unnest(con.conkey) WITH ORDINALITY AS k(attnum, ord) ON true
        JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum
        WHERE ns.nspname = $1 AND rel.relname = 'org_memberships' AND con.contype = 'f'
        ORDER BY a.attname
        "#,
    )
    .bind(&template_schema)
    .fetch_all(&ctx.app_state.pool)
    .await
    .expect("org_memberships FKs must be inspectable in the template schema");
    let rules: std::collections::HashMap<String, String> = cascade_rules.into_iter().collect();
    assert_eq!(
        rules.get("org_id").map(String::as_str),
        Some("c"),
        "org deletion must cascade memberships (confdeltype c)"
    );
    assert_eq!(
        rules.get("user_id").map(String::as_str),
        Some("c"),
        "account hard delete must cascade memberships (confdeltype c)"
    );
    assert_eq!(
        rules.get("role_id").map(String::as_str),
        Some("a"),
        "tier references protect in-use roles from deletion"
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// 12. User org list correctness + pagination
// ---------------------------------------------------------------------------

// Covers US-OA-007 scenario 1: three orgs, three tiers, owner flag exactly
// once; pagination slices server-side.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_list_user_orgs(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let user = herald_test_support::helpers::create_test_user(ctx, "org-user-list@test.com").await;
    let other =
        herald_test_support::helpers::create_test_user(ctx, "org-other-list@test.com").await;

    let org1 = create_org_with_owner(&client, ctx, "ws-list-1", user).await;
    let org2 = create_org_with_owner(&client, ctx, "ws-list-2", other).await;
    let org3 = create_org_with_owner(&client, ctx, "ws-list-3", other).await;
    for (org, role) in [(&org2, "org-admin"), (&org3, "editor-custom-list")] {
        if role == "editor-custom-list" {
            create_custom_tier(ctx, "editor-custom-list", &[("article", "view")]).await;
        }
        client
            .upsert_org_member(
                &ctx._realm_id,
                org,
                UpsertOrgMemberSdkRequest {
                    user_id: user.to_string(),
                    role: role.to_string(),
                    actor_user_id: None,
                },
            )
            .await
            .unwrap();
    }

    let page = client
        .list_user_orgs(&ctx._realm_id, &user.to_string())
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    let by_org: std::collections::HashMap<String, _> = page
        .items
        .iter()
        .map(|item| (item.org_id.clone(), item))
        .collect();
    assert_eq!(
        by_org[&org1].role.name, "org-owner",
        "list must reflect the owner tier"
    );
    assert!(by_org[&org1].is_owner);
    assert_eq!(by_org[&org2].role.name, "org-admin");
    assert!(!by_org[&org2].is_owner);
    assert_eq!(
        by_org[&org3].role.name, "editor-custom-list",
        "custom tiers list equally"
    );

    let first_two = client
        .list_user_orgs_page(&ctx._realm_id, &user.to_string(), Some(1), Some(2))
        .await
        .unwrap();
    assert_eq!(first_two.items.len(), 2);
    assert_eq!(first_two.total, 3);
    assert_eq!(first_two.page, 1);
    assert_eq!(first_two.page_size, 2);

    handle.abort();
}

// ---------------------------------------------------------------------------
// 13. Validation and error shapes
// ---------------------------------------------------------------------------

// Covers the contract's error table: rules_required mirrors the ext
// permission check; unknown tier, bad UUIDs, out-of-range pagination and
// cross-realm accounts answer 400; missing membership answers 404.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_validation_and_error_shapes(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-validation@test.com").await;
    let member =
        herald_test_support::helpers::create_test_user(ctx, "org-member-validation@test.com").await;
    let foreign = create_foreign_realm_user(ctx, "org-foreign-validation@test.com").await;
    let org_id = create_org_with_owner(&client, ctx, "ws-validation", owner).await;

    // Empty rules → structured rules_required.
    expect_error(
        client
            .check_org_permission(
                &ctx._realm_id,
                &org_id,
                OrgPermissionCheckSdkRequest {
                    user_id: member.to_string(),
                    rules: vec![],
                },
            )
            .await
            .map(|_| ()),
        400,
        "rules_required",
    );

    // Unknown tier name.
    expect_error(
        client
            .upsert_org_member(
                &ctx._realm_id,
                &org_id,
                UpsertOrgMemberSdkRequest {
                    user_id: member.to_string(),
                    role: "no-such-tier".to_string(),
                    actor_user_id: None,
                },
            )
            .await
            .map(|_| ()),
        400,
        "org_role_not_found",
    );

    // Malformed UUIDs.
    expect_error(
        client
            .get_org_member(&ctx._realm_id, "not-a-uuid", &member.to_string())
            .await
            .map(|_| ()),
        400,
        "validation_error",
    );
    expect_error(
        client
            .upsert_org_member(
                &ctx._realm_id,
                &org_id,
                UpsertOrgMemberSdkRequest {
                    user_id: "not-a-uuid".to_string(),
                    role: "org-member".to_string(),
                    actor_user_id: None,
                },
            )
            .await
            .map(|_| ()),
        400,
        "validation_error",
    );

    // Out-of-range pagination.
    expect_error(
        client
            .list_org_members_page(&ctx._realm_id, &org_id, Some(0), None)
            .await
            .map(|_| ()),
        400,
        "validation_error",
    );
    expect_error(
        client
            .list_org_members_page(&ctx._realm_id, &org_id, None, Some(101))
            .await
            .map(|_| ()),
        400,
        "validation_error",
    );

    // Removing a non-member.
    expect_error(
        client
            .remove_org_member(&ctx._realm_id, &org_id, &member.to_string(), None)
            .await,
        404,
        "org_member_not_found",
    );

    // Cross-realm member account and cross-realm owner.
    expect_error(
        client
            .upsert_org_member(
                &ctx._realm_id,
                &org_id,
                UpsertOrgMemberSdkRequest {
                    user_id: foreign.to_string(),
                    role: "org-member".to_string(),
                    actor_user_id: None,
                },
            )
            .await
            .map(|_| ()),
        400,
        "org_account_scope_mismatch",
    );
    expect_error(
        client
            .create_org(
                &ctx._realm_id,
                CreateOrgSdkRequest {
                    external_id: "ws-validation-foreign-owner".to_string(),
                    name: None,
                    owner_id: foreign.to_string(),
                },
            )
            .await
            .map(|_| ()),
        400,
        "org_account_scope_mismatch",
    );

    // Nonexistent owner account.
    expect_error(
        client
            .create_org(
                &ctx._realm_id,
                CreateOrgSdkRequest {
                    external_id: "ws-validation-missing-owner".to_string(),
                    name: None,
                    owner_id: Uuid::now_v7().to_string(),
                },
            )
            .await
            .map(|_| ()),
        404,
        "user_not_found",
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// 14. Audit trail
// ---------------------------------------------------------------------------

// Covers PRD §5.2-8: org events land in the audit log with the initiating
// side distinguishable — the client credential (ActorType::Client with the
// bound client_id) versus the represented user — and rejected changes are
// recorded as failures with a reason.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_audit_trail(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-audit@test.com").await;
    let member =
        herald_test_support::helpers::create_test_user(ctx, "org-member-audit@test.com").await;
    let org_id = create_org_with_owner(&client, ctx, "ws-audit", owner).await;

    // On-behalf-of change: audit must attribute the acting user, with the
    // credential recorded in details.
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            UpsertOrgMemberSdkRequest {
                user_id: member.to_string(),
                role: "org-admin".to_string(),
                actor_user_id: Some(owner.to_string()),
            },
        )
        .await
        .unwrap();

    // A rejected change (owner removal) must land as a failure with reason.
    let _ = client
        .remove_org_member(&ctx._realm_id, &org_id, &owner.to_string(), None)
        .await;

    let (client_id,): (String,) = sqlx::query_as("SELECT client_id FROM client_app WHERE id = $1")
        .bind(app_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();

    // org.create: client-credential actor.
    let create_row: (String, String, String) = sqlx::query_as(
        "SELECT actor_type, actor_id, result FROM audit_events \
         WHERE realm_id = $1 AND action = 'org.create' AND target_id = $2",
    )
    .bind(&ctx._realm_id)
    .bind(&org_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("org.create audit event must exist");
    assert_eq!(
        create_row,
        (
            "client".to_string(),
            client_id.clone(),
            "success".to_string()
        )
    );

    // org.member.upsert performed on behalf of the owner: user actor.
    let upsert_row: (String, String) = sqlx::query_as(
        "SELECT actor_type, actor_id FROM audit_events \
         WHERE realm_id = $1 AND action = 'org.member.upsert' \
           AND details->>'viaClientId' IS NOT NULL \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("on-behalf upsert audit event must exist");
    assert_eq!(
        upsert_row,
        ("user".to_string(), owner.to_string()),
        "on-behalf changes attribute the acting user, not the credential"
    );

    // Rejected owner removal: failure with the protection reason.
    let failure_row: (String,) = sqlx::query_as(
        "SELECT result FROM audit_events \
         WHERE realm_id = $1 AND action = 'org.member.remove' AND result = 'failure' \
           AND details->>'reason' LIKE '%owner%'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("rejected change must be audited as a failure with the reason");
    assert_eq!(failure_row.0, "failure");

    handle.abort();
}

// ---------------------------------------------------------------------------
// 15. Console bootstrap path (granting the orgs gate)
// ---------------------------------------------------------------------------

// Covers the deployment path PRD §5.2-1 presupposes: a realm admin grants
// orgs.manage to an app key's role through the admin console
// (POST /api/permission/roles/{roleId}/policies), and the key then reaches
// the organization face. The realm-admin seed must include the orgs pair —
// grantor self-hold otherwise makes the gate permanently ungrantable.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_console_bootstrap_path(ctx: &mut SchemaTestContext) {
    let (base_url, handle) = start_test_server(ctx).await;

    // The rbac seed includes the orgs pair for the realm-admin role (the
    // admin realm is initialized in every test schema, exercising the same
    // init_default_rbac code path production uses for new realms).
    let (seeded,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM role_policies rp \
         JOIN roles r ON r.id = rp.role_id \
         WHERE r.realm_id = 'admin' AND r.name = 'realm-admin' AND rp.resource = 'orgs'",
    )
    .fetch_one(&ctx.app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        seeded, 2,
        "realm-admin seed must carry orgs.view and orgs.manage"
    );

    // A realm-admin-style actor: holds policies.manage plus the orgs pair.
    let (_admin_id, admin_token) = herald_test_support::helpers::create_test_user_with_permissions(
        ctx,
        "org-console-admin@test.com",
        &[("policies", "manage"), ("orgs", "manage"), ("orgs", "view")],
    )
    .await;

    // An app key role with no policies yet; the gate arrives via the console.
    let (api_key, key_id, _app_id) = setup_bound_key(ctx, &[]).await;
    let (role_id,): (Uuid,) = sqlx::query_as(
        "SELECT role_id FROM user_roles WHERE principal_type = 'api_key' AND principal_id = $1",
    )
    .bind(&key_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("bound key role binding must exist");

    let http = reqwest::Client::new();
    let grant = http
        .post(format!(
            "{base_url}/api/permission/roles/{role_id}/policies"
        ))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({"resource": "orgs", "action": "manage"}))
        .send()
        .await
        .expect("console grant request must send");
    assert!(
        grant.status().is_success(),
        "realm admin must be able to grant orgs.manage to an app key role: {}",
        grant.status()
    );

    // End to end: the granted key drives the organization face.
    let client = Client::new(base_url.clone(), api_key, None);
    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-bootstrap@test.com").await;
    let org = create_org_with_owner(&client, ctx, "ws-console-bootstrap", owner).await;
    assert!(!org.is_empty());

    // Boundary: a delegated policies.manage holder WITHOUT orgs.manage
    // cannot grant it to a regular (non-exempt) role — self-hold stands.
    let (_, limited_token) = herald_test_support::helpers::create_test_user_with_permissions(
        ctx,
        "org-console-limited@test.com",
        &[("policies", "manage")],
    )
    .await;
    let custom_role = create_custom_tier(ctx, "plain-flat-role", &[]).await;

    let denied = http
        .post(format!(
            "{base_url}/api/permission/roles/{custom_role}/policies"
        ))
        .bearer_auth(&limited_token)
        .json(&serde_json::json!({"resource": "orgs", "action": "manage"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        denied.status().as_u16(),
        403,
        "self-hold must still bind for non-exempt roles"
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// 16. Tier vocabulary via the admin console
// ---------------------------------------------------------------------------

// Covers the org-authorization PRD §4.1 tier-configuration path: realm admins
// attach app resource policies to the builtin org tiers through the console.
// The grantor self-hold is exempted for those tiers (an admin cannot hold app
// resource names), stays enforced for regular roles, and the configured
// vocabulary drives organization checks end to end.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_org_tier_vocabulary_via_console(ctx: &mut SchemaTestContext) {
    let (api_key, _key_id, _app_id) = setup_bound_key(ctx, &[("orgs", "manage")]).await;
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url.clone(), api_key, None);

    let (_, admin_token) = herald_test_support::helpers::create_test_user_with_permissions(
        ctx,
        "org-tier-console-admin@test.com",
        &[("policies", "manage")],
    )
    .await;

    let owner =
        herald_test_support::helpers::create_test_user(ctx, "org-owner-vocab@test.com").await;
    let member =
        herald_test_support::helpers::create_test_user(ctx, "org-member-vocab@test.com").await;
    let org_id = create_org_with_owner(&client, ctx, "ws-vocab", owner).await;

    // The builtin org-member tier is exempt from grantor self-hold: an
    // admin holding no app-resource permission can still configure it.
    let tier_role_id = tier_role_id(ctx, "org-member").await;
    let http = reqwest::Client::new();
    let attach = http
        .post(format!(
            "{base_url}/api/permission/roles/{tier_role_id}/policies"
        ))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({"resource": "article", "action": "view"}))
        .send()
        .await
        .expect("tier vocabulary attach must send");
    assert!(
        attach.status().is_success(),
        "builtin org tiers must be configurable without self-hold: {}",
        attach.status()
    );

    // The same grant on a regular role stays blocked (self-hold boundary).
    let custom_role = create_custom_tier(ctx, "plain-flat-role-vocab", &[]).await;
    let denied = http
        .post(format!(
            "{base_url}/api/permission/roles/{custom_role}/policies"
        ))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({"resource": "article", "action": "view"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        denied.status().as_u16(),
        403,
        "self-hold must still bind for non-exempt roles"
    );

    // The console-configured vocabulary drives the organization check.
    client
        .upsert_org_member(
            &ctx._realm_id,
            &org_id,
            UpsertOrgMemberSdkRequest {
                user_id: member.to_string(),
                role: "org-member".to_string(),
                actor_user_id: None,
            },
        )
        .await
        .unwrap();
    let mut req = check_rules(&[("article", "view")]);
    req.user_id = member.to_string();
    let result = client
        .check_org_permission(&ctx._realm_id, &org_id, req)
        .await
        .unwrap();
    assert!(
        result.member && result.allowed,
        "console-configured tier vocabulary must drive org checks: {result:?}"
    );

    handle.abort();
}
