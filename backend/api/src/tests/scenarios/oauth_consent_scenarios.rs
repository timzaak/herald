// =============================================================================
// OAuth Consent Scenario Tests
// =============================================================================
//
// Scenario tests for the third-party OAuth consent layer: authorize redirects
// to the SPA consent route, and the consent probe/decision endpoints carry an
// already-authenticated user (first-party session in the browser profile)
// through a pending authorize transaction without re-entering credentials.
//
// WHY this layer exists: before it, authorize always landed on /auth/login
// and the root route guard bounced authenticated users away from /auth/*
// before the page mounted, dropping oauthClientId/redirectUri/state — the
// authorization could never complete for anyone with a console or
// user-center session (the first third-party client was blocked on exactly
// this in production).
//
// User stories: docs/user-stories/auth/third-party-app.md (US-TP-001
// Authorization Code + PKCE, US-TP-015 SPA SSO initiation),
// docs/user-stories/core/regular-user.md (US-RU-008 access third-party app)
// — the consent decision mints a code with the login entrances' one-time
// state semantics, authorize hands the SPA consent route the continuation
// parameters, and a session-holding user reaches the third-party callback
// with a valid code, silently once granted.
//
// =============================================================================

use crate::tests::helpers::auth_helpers::{
    create_admin_session_with_role, create_admin_session_with_user,
};
use crate::tests::helpers::oauth_pkce_helpers::{
    compute_code_challenge, create_client_app_with_redirect_uris, extract_auth_code_from_redirect,
    generate_code_verifier, generate_state, login_with_oauth, oauth_authorize,
    oauth_token_exchange, redirect_query_param,
};
use crate::tests::helpers::test_setup_helpers::{create_test_user, record_test_user_consent};
use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use test_context::test_context;
use tower::ServiceExt;

/// Helper: set up a first-party session (the "browser profile already logged
/// into Herald" precondition of the original bug) and return (bearer token,
/// user id). Consent to the current effective agreements is recorded too —
/// the helper-created user never passed a signup flow, and the legal consent
/// gate would otherwise (correctly) block every consent approval.
async fn setup_session_user(ctx: &mut SchemaTestContext, email: &str) -> (String, String) {
    let (token, user_id) = create_admin_session_with_user(ctx, email, 1800).await;
    record_test_user_consent(
        &ctx._app_state.pool,
        uuid::Uuid::parse_str(&user_id).expect("valid user id"),
        &ctx._realm_id,
    )
    .await;
    (token, user_id)
}

/// GET /consent probe with a bearer session.
async fn consent_probe(
    ctx: &SchemaTestContext,
    realm_id: &str,
    token: &str,
    oauth_client_id: &str,
    redirect_uri: &str,
    state: &str,
) -> axum::response::Response {
    let uri = format!(
        "/api/oauth/{}/consent?oauth_client_id={}&redirect_uri={}&state={}",
        realm_id,
        urlencoding::encode(oauth_client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(state),
    );
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

/// POST /consent decision with a bearer session.
async fn consent_decision(
    ctx: &SchemaTestContext,
    realm_id: &str,
    token: &str,
    oauth_client_id: &str,
    redirect_uri: &str,
    state: &str,
    approved: bool,
) -> axum::response::Response {
    ctx.create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/oauth/{}/consent", realm_id))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(
                    json!({
                        "oauth_client_id": oauth_client_id,
                        "redirect_uri": redirect_uri,
                        "state": state,
                        "approved": approved,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Rows in user_client_grant for (user, client). The grant is what makes the
/// NEXT authorize silent, so tests assert its presence/absence explicitly.
async fn grant_count(ctx: &SchemaTestContext, user_id: &str, client_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_client_grant g
         JOIN client_app c ON c.id = g.client_app_id
         WHERE g.user_id = $1 AND c.realm_id = $2 AND c.client_id = $3",
    )
    .bind(user_id.parse::<uuid::Uuid>().unwrap())
    .bind(&ctx._realm_id)
    .bind(client_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap()
}

// =============================================================================
// Test 1: full consent flow for a session-holding user (the original bug's
// acceptance path: authorize → consent → code → token exchange, then the
// second authorize completing silently on the remembered grant)
// =============================================================================

// WHY: this is the exact production failure — a realm user with an existing
// first-party session starts a third-party authorize. Every step here was
// impossible before the consent layer: the guard ate the params at the login
// redirect, so no code could ever be issued for a session-holder.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_consent_session_user_full_flow(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token = create_admin_session_with_role(ctx, "consent-admin@test.com", 1800).await;

    let redirect_uri = "https://oncewise.example/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "oncewise-flow",
        "OnceWise Flow",
        &[redirect_uri],
    )
    .await;
    assert_eq!(
        create_response.status(),
        201,
        "Client app creation should succeed"
    );

    // The browser profile already holds a Herald session.
    let (session_token, user_id) = setup_session_user(ctx, "consent-user@test.com").await;
    assert_eq!(
        grant_count(ctx, &user_id, "oncewise-flow").await,
        0,
        "no grant exists before the first authorization"
    );

    // Step 1: authorize lands on the consent page with the continuation tuple.
    let verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&verifier);
    let state = generate_state();
    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "oncewise-flow",
        redirect_uri,
        &state,
        &code_challenge,
        "S256",
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);
    let location = authorize_response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("authorize must redirect");
    assert!(
        location.contains(&format!("/{}/auth/consent", realm_id)),
        "authorize must land on the consent page, got: {location}"
    );
    assert!(
        location.contains("oauthClientId=oncewise-flow")
            && location.contains(&format!("state={}", urlencoding::encode(&state))),
        "the consent redirect must carry the OAuth continuation params, got: {location}"
    );

    // Step 2: the probe reports that consent is needed and names the client.
    let probe_response = consent_probe(
        ctx,
        &realm_id,
        &session_token,
        "oncewise-flow",
        redirect_uri,
        &state,
    )
    .await;
    assert_eq!(probe_response.status(), StatusCode::OK);
    let probe_body: Value = response_json(probe_response).await;
    assert_eq!(
        probe_body["consent_required"],
        json!(true),
        "first authorization must surface the consent UI"
    );
    assert_eq!(
        probe_body["client_app_name"],
        json!("OnceWise Flow"),
        "the probe must name the client for the consent UI"
    );

    // Step 3: approval mints a code without any credential re-entry.
    let decision_response = consent_decision(
        ctx,
        &realm_id,
        &session_token,
        "oncewise-flow",
        redirect_uri,
        &state,
        true,
    )
    .await;
    assert_eq!(decision_response.status(), StatusCode::OK);
    let decision_body: Value = response_json(decision_response).await;
    assert_eq!(decision_body["status"], json!("authorized"));
    let redirect_to = decision_body["redirect_to"].as_str().expect("redirect_to");
    let auth_code =
        extract_auth_code_from_redirect(redirect_to).expect("redirect carries the code");
    assert_eq!(
        redirect_query_param(redirect_to, "state").as_deref(),
        Some(state.as_str()),
        "redirect must echo the client state"
    );

    // Step 4: the code exchanges normally (PKCE) — the flow really completed.
    let token_response = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        "oncewise-flow",
        &verifier,
    )
    .await;
    assert_eq!(
        token_response.status(),
        StatusCode::OK,
        "the consent-issued code must exchange for tokens"
    );

    // Step 5: the exchange remembered the grant, so the next authorize is silent.
    assert_eq!(
        grant_count(ctx, &user_id, "oncewise-flow").await,
        1,
        "the token exchange must record the user-client grant"
    );
    let state2 = generate_state();
    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "oncewise-flow",
        redirect_uri,
        &state2,
        &compute_code_challenge(&generate_code_verifier()),
        "S256",
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);
    let probe_response = consent_probe(
        ctx,
        &realm_id,
        &session_token,
        "oncewise-flow",
        redirect_uri,
        &state2,
    )
    .await;
    assert_eq!(probe_response.status(), StatusCode::OK);
    let probe_body: Value = response_json(probe_response).await;
    assert_eq!(
        probe_body["consent_required"],
        json!(false),
        "a remembered grant must make the next authorize silent"
    );
}

// =============================================================================
// Test 2: denial propagates the standard OAuth error and consumes the state
// =============================================================================

// WHY: a denial is a protocol outcome the third-party client must see (RFC
// 6749 §4.1.2.1 error=access_denied), and the one-time state must not
// survive a decision — otherwise a denied transaction could be replayed into
// an approval.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_consent_deny_redirects_and_consumes_state(
    ctx: &mut SchemaTestContext,
) {
    let realm_id = ctx._realm_id.clone();
    let admin_token =
        create_admin_session_with_role(ctx, "consent-deny-admin@test.com", 1800).await;
    let redirect_uri = "https://denied-app.example/cb";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "denied-app",
        "Denied App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    let (session_token, user_id) = setup_session_user(ctx, "consent-deny-user@test.com").await;

    let state = generate_state();
    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "denied-app",
        redirect_uri,
        &state,
        &compute_code_challenge(&generate_code_verifier()),
        "S256",
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);

    let decision_response = consent_decision(
        ctx,
        &realm_id,
        &session_token,
        "denied-app",
        redirect_uri,
        &state,
        false,
    )
    .await;
    assert_eq!(decision_response.status(), StatusCode::OK);
    let decision_body: Value = response_json(decision_response).await;
    assert_eq!(decision_body["status"], json!("denied"));
    let redirect_to = decision_body["redirect_to"].as_str().expect("redirect_to");
    assert!(
        redirect_to.starts_with(redirect_uri),
        "the denial must return to the client callback, got: {redirect_to}"
    );
    assert_eq!(
        redirect_query_param(redirect_to, "error").as_deref(),
        Some("access_denied")
    );
    assert_eq!(
        redirect_query_param(redirect_to, "state").as_deref(),
        Some(state.as_str())
    );

    // The decision consumed the one-time state: a replayed approval on the
    // same transaction is rejected.
    let replay_response = consent_decision(
        ctx,
        &realm_id,
        &session_token,
        "denied-app",
        redirect_uri,
        &state,
        true,
    )
    .await;
    assert_eq!(
        replay_response.status(),
        StatusCode::BAD_REQUEST,
        "a decided state must never authorize anything again"
    );

    assert_eq!(
        grant_count(ctx, &user_id, "denied-app").await,
        0,
        "a denial must not record a grant"
    );
}

// =============================================================================
// Test 3: the decision validates the request against the stored state
// =============================================================================

// WHY: the probe and the decision must agree on WHICH transaction is being
// approved — a mismatched client_id on the decision must be rejected exactly
// like the login entrances reject it, or an attacker could swap the client
// mid-flow (state fixation-adjacent parameter mixing).
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_consent_decision_rejects_client_mismatch(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token =
        create_admin_session_with_role(ctx, "consent-mismatch-admin@test.com", 1800).await;
    let redirect_uri = "https://mismatch-app.example/cb";
    for (client_id, name) in [("mismatch-app", "Mismatch App"), ("other-app", "Other App")] {
        let response = create_client_app_with_redirect_uris(
            ctx,
            &realm_id,
            &admin_token,
            client_id,
            name,
            &[redirect_uri],
        )
        .await;
        assert_eq!(response.status(), 201);
    }

    let (session_token, _) = setup_session_user(ctx, "consent-mismatch-user@test.com").await;

    let state = generate_state();
    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "mismatch-app",
        redirect_uri,
        &state,
        &compute_code_challenge(&generate_code_verifier()),
        "S256",
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);

    // Same state, but the decision claims a different client.
    let decision_response = consent_decision(
        ctx,
        &realm_id,
        &session_token,
        "other-app",
        redirect_uri,
        &state,
        true,
    )
    .await;
    assert_eq!(
        decision_response.status(),
        StatusCode::BAD_REQUEST,
        "a decision for a different client than the stored state must be rejected"
    );
}

// =============================================================================
// Test 4: the probe requires a session; a missing/unknown state is a 400
// =============================================================================

// WHY: the consent endpoints exist ONLY for authenticated users (anonymous
// visitors are forwarded to login by the SPA). An unauthenticated probe must
// never leak whether a state is pending, and an unknown state must not be
// distinguishable from an expired one beyond "restart the flow".
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_consent_probe_requires_session_and_known_state(
    ctx: &mut SchemaTestContext,
) {
    let realm_id = ctx._realm_id.clone();

    // Anonymous probe → 401 before any state lookup.
    let anonymous_response = consent_probe(
        ctx,
        &realm_id,
        "not-a-real-token",
        "any-app",
        "https://any.example/cb",
        "some-state",
    )
    .await;
    assert_eq!(anonymous_response.status(), StatusCode::UNAUTHORIZED);

    // Authenticated probe against a state that was never seeded.
    let (session_token, _) = setup_session_user(ctx, "consent-anon-user@test.com").await;
    let unknown_state = consent_probe(
        ctx,
        &realm_id,
        &session_token,
        "any-app",
        "https://any.example/cb",
        "never-seeded-state",
    )
    .await;
    assert_eq!(
        unknown_state.status(),
        StatusCode::BAD_REQUEST,
        "an unknown/expired state must read as a restartable failure"
    );
}

// =============================================================================
// Test 5: a password login completion records the grant (fresh-user parity)
// =============================================================================

// WHY: the pre-consent semantics treat completing login on the continuation
// page as the authorization itself (the code is minted immediately). If the
// grant were only written by consent-page approvals, a user who first
// authorized via a fresh login would be shown a consent page on their SECOND
// visit — a question they already answered. The /token exchange is the single
// choke point that makes both entrances count.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_login_completion_records_grant(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token =
        create_admin_session_with_role(ctx, "consent-login-admin@test.com", 1800).await;
    let redirect_uri = "https://login-path-app.example/cb";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "login-path-app",
        "Login Path App",
        &[redirect_uri],
    )
    .await;
    assert_eq!(create_response.status(), 201);

    // Anonymous user completes the flow the classic way: credentials on the
    // login page with the OAuth continuation params.
    let email = "consent-fresh-user@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;

    let verifier = generate_code_verifier();
    let state = generate_state();
    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "login-path-app",
        redirect_uri,
        &state,
        &compute_code_challenge(&verifier),
        "S256",
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);

    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "login-path-app",
        redirect_uri,
        &state,
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let login_body: Value = response_json(login_response).await;
    let redirect_to = login_body["redirectTo"]
        .as_str()
        .expect("login must return the OAuth redirect")
        .to_string();
    let auth_code = extract_auth_code_from_redirect(&redirect_to).expect("code in redirect");

    assert_eq!(
        grant_count(ctx, &user_id.to_string(), "login-path-app").await,
        0,
        "no grant before the code is exchanged"
    );

    let token_response = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        "login-path-app",
        &verifier,
    )
    .await;
    assert_eq!(token_response.status(), StatusCode::OK);

    assert_eq!(
        grant_count(ctx, &user_id.to_string(), "login-path-app").await,
        1,
        "the token exchange must remember the authorization for the next visit"
    );
}

// =============================================================================
// Login-continuation client resolution
// =============================================================================

/// The consent redirect's `clientId` names the first-party client the SPA
/// login page binds the continuation session to. It is resolved per realm,
/// not hardcoded to the seed id: a realm whose `admin-web-console` seed is
/// disabled still has `user-account-center` (provisioning seeds both), and
/// the authorization must complete through it — end to end, down to the
/// token exchange.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_oauth_consent_login_client_resolved_per_realm(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let admin_token =
        create_admin_session_with_role(ctx, "consent-resolve-admin@test.com", 1800).await;

    let redirect_uri = "https://resolve.example/callback";
    let create_response = create_client_app_with_redirect_uris(
        ctx,
        &realm_id,
        &admin_token,
        "resolve-flow",
        "Resolve Flow",
        &[redirect_uri],
    )
    .await;
    assert_eq!(
        create_response.status(),
        201,
        "Client app creation should succeed"
    );

    // Disable the seeded console client: the realm's remaining enabled
    // first-party client must take over the login continuation.
    sqlx::query("UPDATE client_app SET enabled = false WHERE realm_id = $1 AND client_id = $2")
        .bind(&realm_id)
        .bind("admin-web-console")
        .execute(&ctx._app_state.pool)
        .await
        .expect("disable the seeded console client");

    let verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&verifier);
    let state = generate_state();
    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "resolve-flow",
        redirect_uri,
        &state,
        &code_challenge,
        "S256",
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);
    let location = authorize_response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("authorize must redirect")
        .to_string();
    assert!(
        location.contains("clientId=user-account-center"),
        "the login continuation must bind to an enabled first-party client, got: {location}"
    );
    assert!(
        !location.contains("clientId=admin-web-console"),
        "a disabled first-party client must not be handed to the login page, got: {location}"
    );

    // End-to-end: the resolved client actually serves as the login entrance.
    let email = "consent-resolve-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    let login_payload = json!({
        "clientId": "user-account-center",
        "email": email,
        "password": password,
        "turnstileToken": "dummy",
        "oauthClientId": "resolve-flow",
        "redirectUri": redirect_uri,
        "state": state,
    });
    let login_response = ctx
        .create_unified_test_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/auth/{}/login", realm_id))
                .header("content-type", "application/json")
                .header("x-forwarded-for", "3.3.3.3")
                .body(Body::from(login_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        login_response.status(),
        StatusCode::OK,
        "login through the resolved client must succeed"
    );
    let login_body: Value = response_json(login_response).await;
    let redirect_to = login_body["redirectTo"]
        .as_str()
        .expect("OAuth login must complete with a redirect carrying the code");
    let auth_code =
        extract_auth_code_from_redirect(redirect_to).expect("code in the login redirect");

    let token_response = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &auth_code,
        redirect_uri,
        "resolve-flow",
        &verifier,
    )
    .await;
    assert_eq!(
        token_response.status(),
        StatusCode::OK,
        "the code minted through the resolved login client must exchange"
    );
}
