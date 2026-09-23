// =============================================================================
// Paywall M1 — Role-Grant Config Dimension Scenario Tests
// =============================================================================
//
// Proves `grantedRoleIds` is a fully wired, validated, persisted, exposed
// cross-cutting dimension on `provider_entitlement_mappings`:
//   * PUT batch sets / clears / leaves-unchanged the column (three-state)
//   * GET list + GET detail surface the field (always present, never null)
//   * Cross-realm role id → structured HTTP 400 `role_not_in_realm`,
//     whole batch rolled back (no partial write)
//   * UUID[] array holds multiple values end-to-end
//
// Mirrors the existing `entitlement_mapping_crud_scenarios.rs` patterns
// (auth_request helper, oneshot, setup_billing_admin_session,
// setup_test_entitlement_mapping_full, create_role).
//
// User Story: US-PW-001 (entitlement→role mapping configuration)
// Covers: design §1.3/§1.4, §4.2.2 (PUT/GET grantedRoleIds + 400 guard),
//         §4.3.2 (UUID[] column), §5.2 (three-state + RoleNotInRealm),
//         §6.1 (M1), §6.3 (non-regression)
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::*;
    use crate::tests::helpers::rbac_helpers::create_role;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use serde_json::{Value, json};
    use sqlx::Row;
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as PaywallM1Context;

    // Helper: build request with admin auth cookie + JSON content-type.
    // Copied verbatim from `entitlement_mapping_crud_scenarios.rs`.
    fn auth_request(method: &str, uri: String, token: &str, body: Option<Body>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", token));
        if let Some(b) = body {
            builder = builder.header("Content-Type", "application/json");
            builder.body(b).unwrap()
        } else {
            builder.body(Body::empty()).unwrap()
        }
    }

    /// Read the persisted `granted_role_ids` column directly from
    /// `provider_entitlement_mappings` for `mapping_id`. Asserts the DB
    /// column (not just the API echo). The column is `UUID[] NOT NULL DEFAULT
    /// '{}'` so sqlx decodes it straight to `Vec<Uuid>` (same encoding path the
    /// infra repo uses for `Vec<Uuid>` → `uuid[]`).
    async fn fetch_mapping_granted_role_ids(ctx: &PaywallM1Context, mapping_id: Uuid) -> Vec<Uuid> {
        // The column is `UUID[] NOT NULL DEFAULT '{}'`; sqlx decodes a Postgres
        // uuid[] straight to `Vec<Uuid>` via `PgRow::get` (same encoding path
        // the infra repo uses). Using `query` + `row.get` mirrors the proven
        // `provider_ids` read in `account_self_delete_scenarios.rs`.
        let row =
            sqlx::query("SELECT granted_role_ids FROM provider_entitlement_mappings WHERE id = $1")
                .bind(mapping_id)
                .fetch_one(&ctx.app_state.pool)
                .await
                .expect("mapping row must exist");
        row.get::<Vec<Uuid>, _>("granted_role_ids")
    }

    /// Read the persisted fixed-grant points for a mapping from its owned
    /// distribution rule. `provider_entitlement_mappings.points_per_period` was
    /// removed by the distribution-rules refactor; the grant amount now lives on
    /// `points_distribution_rules.points_amount`. Returns the first fixed rule's
    /// amount (the migrated equivalent of the old single mapping-level value).
    async fn fetch_mapping_points(ctx: &PaywallM1Context, mapping_id: Uuid) -> Option<i32> {
        sqlx::query_scalar::<_, Option<i64>>(
            "SELECT points_amount FROM points_distribution_rules
             WHERE entitlement_mapping_id = $1 AND grant_mode = 'fixed'
             ORDER BY display_order LIMIT 1",
        )
        .bind(mapping_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("mapping rule query must execute")
        .map(|v| v as i32)
    }

    /// PUT batch with a single update carrying `grantedRoleIds` = `granted`.
    /// Returns `(status, body_json)`. `None` → field omitted entirely;
    /// `Some(vec)` → serialized (clear when empty, set when non-empty).
    async fn put_batch_granted_role_ids(
        app: axum::Router,
        _realm_id: &str,
        token: &str,
        provider: &str,
        external_product_id: &str,
        mapping_id: Uuid,
        granted: Option<Vec<Uuid>>,
    ) -> (StatusCode, Value) {
        // Build the per-row update object. `grantedRoleIds` is only added
        // when `granted.is_some()` so that `None` ⟺ field absent ⟺ unchanged.
        let mut update = json!({
            "mappingId": mapping_id,
        });
        if let Some(ids) = granted {
            let arr: Vec<String> = ids.into_iter().map(|u| u.to_string()).collect();
            update["grantedRoleIds"] = json!(arr);
        }

        let payload = json!({
            "paymentProvider": provider,
            "externalProductId": external_product_id,
            "updates": [update],
        });

        let response = app
            .oneshot(auth_request(
                "PUT",
                "/api/bill/entitlement-mappings/batch".to_string(),
                token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, json)
    }

    /// PUT batch helper that lets a test also carry extra fields alongside
    /// `grantedRoleIds`. Used by the "unchanged when omitted" test which
    /// changes `pointsPerPeriod` while omitting `grantedRoleIds`.
    async fn put_batch_update(
        app: axum::Router,
        _realm_id: &str,
        token: &str,
        provider: &str,
        external_product_id: &str,
        update_row: Value,
    ) -> (StatusCode, Value) {
        let payload = json!({
            "paymentProvider": provider,
            "externalProductId": external_product_id,
            "updates": [update_row],
        });
        let response = app
            .oneshot(auth_request(
                "PUT",
                "/api/bill/entitlement-mappings/batch".to_string(),
                token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, json)
    }

    /// Insert a role row directly with a foreign `realm_id` so it is NOT a
    /// member of `home_realm_id`. `roles.realm_id` has no FK constraint and
    /// `validate_granted_role_ids` does a string comparison on `roles.realm_id`,
    /// so a hand-inserted foreign role is a faithful cross-realm stand-in
    /// (avoids bootstrapping a second realm + second admin session).
    async fn insert_foreign_role(
        ctx: &PaywallM1Context,
        foreign_realm_id: &str,
        client_id: &str,
    ) -> Uuid {
        let role_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
             VALUES ($1, $2, $3, $4, $5, false)",
        )
        .bind(role_id)
        .bind(format!("foreign-role-{}", role_id))
        .bind("cross-realm stand-in for granted_role_ids validation test")
        .bind(foreign_realm_id)
        .bind(client_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("insert foreign role");
        role_id
    }

    // =========================================================================
    // 1. PUT batch sets grantedRoleIds
    // =========================================================================

    /// User Story: US-PW-001 (configure role-grant dimension)
    /// Covers: design §4.2.2 (PUT batch grantedRoleIds set), §5.2 (persisted),
    ///         §6.1 M1
    #[test_context(PaywallM1Context)]
    #[tokio::test]
    async fn test_batch_update_sets_granted_role_ids(ctx: &mut PaywallM1Context) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "pw1-set@test.com").await;
        let realm_id = ctx._realm_id.clone();

        let mapping_id = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_pw1_set",
            None,
            "pw-set-key",
            Some("one_time"),
            None,
            Some(100),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await;

        let role_a = create_role(ctx, &realm_id, &token, "pw1-role-a", "role a").await;
        let role_b = create_role(ctx, &realm_id, &token, "pw1-role-b", "role b").await;

        let (status, json) = put_batch_granted_role_ids(
            app.clone(),
            &realm_id,
            &token,
            "stripe",
            "prod_pw1_set",
            mapping_id,
            Some(vec![role_a, role_b]),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        // Response prices[0].grantedRoleIds — order-insensitive compare.
        let resp_ids = json["prices"][0]["grantedRoleIds"]
            .as_array()
            .expect("prices[0].grantedRoleIds must be an array");
        let mut got: Vec<Uuid> = resp_ids
            .iter()
            .map(|v| Uuid::parse_str(v.as_str().unwrap()).unwrap())
            .collect();
        let mut want = vec![role_a, role_b];
        got.sort();
        want.sort();
        assert_eq!(got, want);

        // DB column assertion.
        let db_ids = fetch_mapping_granted_role_ids(ctx, mapping_id).await;
        let mut db_sorted = db_ids.clone();
        db_sorted.sort();
        assert_eq!(db_sorted, want);
    }

    // =========================================================================
    // 2. PUT batch clears grantedRoleIds (Some([]))
    // =========================================================================

    /// User Story: US-PW-001 (two dimensions can each be empty)
    /// Covers: design §4.2.2 (Some([]) = clear), §5.2 three-state semantics
    #[test_context(PaywallM1Context)]
    #[tokio::test]
    async fn test_batch_update_clears_granted_role_ids(ctx: &mut PaywallM1Context) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "pw1-clear@test.com").await;
        let realm_id = ctx._realm_id.clone();

        let mapping_id = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_pw1_clear",
            None,
            "pw-clear-key",
            Some("one_time"),
            None,
            Some(100),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await;

        let role_a = create_role(ctx, &realm_id, &token, "pw1-clear-role", "clear role").await;

        // Seed a non-empty array first so the DB has something to clear.
        let (status, _json) = put_batch_granted_role_ids(
            app.clone(),
            &realm_id,
            &token,
            "stripe",
            "prod_pw1_clear",
            mapping_id,
            Some(vec![role_a]),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            fetch_mapping_granted_role_ids(ctx, mapping_id).await,
            vec![role_a]
        );

        // Now clear with Some(vec![]).
        let (status, json) = put_batch_granted_role_ids(
            app.clone(),
            &realm_id,
            &token,
            "stripe",
            "prod_pw1_clear",
            mapping_id,
            Some(vec![]),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        // DB column now empty.
        assert!(
            fetch_mapping_granted_role_ids(ctx, mapping_id)
                .await
                .is_empty()
        );

        // GET single must surface `[]` (not null — always present).
        let resp = app
            .clone()
            .oneshot(auth_request(
                "GET",
                format!("/api/bill/entitlement-mappings/{}", mapping_id),
                &token,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let detail: Value = serde_json::from_slice(&body).unwrap();
        assert!(
            detail["grantedRoleIds"].is_array(),
            "grantedRoleIds must be present as an array, got: {}",
            detail["grantedRoleIds"]
        );
        assert!(detail["grantedRoleIds"].as_array().unwrap().is_empty());
        // The response prices[0] after the clear also reflects empty.
        assert!(
            json["prices"][0]["grantedRoleIds"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(false),
            "cleared grantedRoleIds must serialize to [] in prices, got: {}",
            json["prices"][0]["grantedRoleIds"]
        );
    }

    // =========================================================================
    // 3. PUT batch leaves grantedRoleIds unchanged when omitted
    // =========================================================================

    /// User Story: US-PW-001 (None = unchanged, orthogonal to other fields)
    /// Covers: design §4.2.2 (None = unchanged), §5.2 three-state, §6.3 regression
    #[test_context(PaywallM1Context)]
    #[tokio::test]
    async fn test_batch_update_leaves_granted_role_ids_unchanged_when_omitted(
        ctx: &mut PaywallM1Context,
    ) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "pw1-omit@test.com").await;
        let realm_id = ctx._realm_id.clone();

        let mapping_id = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_pw1_omit",
            None,
            "pw-omit-key",
            Some("one_time"),
            None,
            Some(100),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await;

        let role_a = create_role(ctx, &realm_id, &token, "pw1-omit-role", "omit role").await;

        // Set grantedRoleIds = [role_a] first.
        let (status, _) = put_batch_granted_role_ids(
            app.clone(),
            &realm_id,
            &token,
            "stripe",
            "prod_pw1_omit",
            mapping_id,
            Some(vec![role_a]),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            fetch_mapping_granted_role_ids(ctx, mapping_id).await,
            vec![role_a]
        );

        // Now PUT batch changing a DIFFERENT field (pointRules) and OMITTING
        // grantedRoleIds. (`pointsPerPeriod` was removed; points config is now
        // `pointRules`.) Carry the existing rule's id so the upsert UPDATES the
        // seeded 100-amount rule rather than creating a second rule.
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            &realm_id,
        )
        .await;
        let existing_rule_id: String = sqlx::query_scalar(
            "SELECT id::text FROM points_distribution_rules \
             WHERE entitlement_mapping_id = $1 AND grant_mode = 'fixed' \
             ORDER BY display_order LIMIT 1",
        )
        .bind(mapping_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("seeded mapping must have one fixed rule to upsert");
        let update_row = json!({
            "mappingId": mapping_id,
            "pointRules": [{
                "id": existing_rule_id,
                "bucketId": bucket_id,
                "triggerSources": ["topup"],
                "grantMode": "fixed",
                "pointsAmount": 999
            }],
        });
        let (status, json) = put_batch_update(
            app.clone(),
            &realm_id,
            &token,
            "stripe",
            "prod_pw1_omit",
            update_row,
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        // grantedRoleIds STILL [role_a] — unchanged (COALESCE semantics).
        let db_ids = fetch_mapping_granted_role_ids(ctx, mapping_id).await;
        assert_eq!(db_ids, vec![role_a]);

        // The topup rule amount DID change to 999 — proves the other field updated.
        assert_eq!(fetch_mapping_points(ctx, mapping_id).await, Some(999));
        // Response reflects the new points too (pointRules[0].pointsAmount).
        assert_eq!(json["prices"][0]["pointRules"][0]["pointsAmount"], 999);
    }

    // =========================================================================
    // 4. GET detail returns grantedRoleIds
    // =========================================================================

    /// User Story: US-PW-001 (GET returns grantedRoleIds)
    /// Covers: design §4.2.2 (GET single response field), §6.1 M1
    #[test_context(PaywallM1Context)]
    #[tokio::test]
    async fn test_get_mapping_detail_returns_granted_role_ids(ctx: &mut PaywallM1Context) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "pw1-detail@test.com").await;
        let realm_id = ctx._realm_id.clone();

        let mapping_id = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_pw1_detail",
            None,
            "pw-detail-key",
            Some("one_time"),
            None,
            Some(100),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await;

        let role_a = create_role(ctx, &realm_id, &token, "pw1-detail-a", "detail role a").await;
        let role_b = create_role(ctx, &realm_id, &token, "pw1-detail-b", "detail role b").await;

        let (status, _) = put_batch_granted_role_ids(
            app.clone(),
            &realm_id,
            &token,
            "stripe",
            "prod_pw1_detail",
            mapping_id,
            Some(vec![role_a, role_b]),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                format!("/api/bill/entitlement-mappings/{}", mapping_id),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        // Field is always serialized (present, an array).
        assert!(
            json.get("grantedRoleIds").is_some(),
            "grantedRoleIds must be present (always serialized)"
        );
        assert!(
            json["grantedRoleIds"].is_array(),
            "grantedRoleIds must be a JSON array"
        );

        let mut got: Vec<Uuid> = json["grantedRoleIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| Uuid::parse_str(v.as_str().unwrap()).unwrap())
            .collect();
        let mut want = vec![role_a, role_b];
        got.sort();
        want.sort();
        assert_eq!(got, want);
    }

    // =========================================================================
    // 5. GET list returns grantedRoleIds
    // =========================================================================

    /// User Story: US-PW-001 (GET list returns grantedRoleIds)
    /// Covers: design §4.2.2 (GET list response field), §6.1 M1
    #[test_context(PaywallM1Context)]
    #[tokio::test]
    async fn test_list_mappings_returns_granted_role_ids(ctx: &mut PaywallM1Context) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "pw1-list@test.com").await;
        let realm_id = ctx._realm_id.clone();

        // Mapping A: grantedRoleIds = [role_a].
        let mapping_a = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_pw1_list_a",
            None,
            "pw-list-key-a",
            Some("one_time"),
            None,
            Some(100),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await;
        // Mapping B: empty grantedRoleIds (left as default '{}').
        let mapping_b = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_pw1_list_b",
            None,
            "pw-list-key-b",
            Some("one_time"),
            None,
            Some(50),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await;

        let role_a = create_role(ctx, &realm_id, &token, "pw1-list-role", "list role").await;

        let (status, _) = put_batch_granted_role_ids(
            app.clone(),
            &realm_id,
            &token,
            "stripe",
            "prod_pw1_list_a",
            mapping_a,
            Some(vec![role_a]),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                "/api/bill/entitlement-mappings".to_string(),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let items = json["items"].as_array().expect("items must be an array");
        assert_eq!(items.len(), 2);

        // Index by mapping id for stable assertions.
        let by_id: std::collections::HashMap<String, &Value> = items
            .iter()
            .map(|it| {
                let id = it["id"].as_str().unwrap().to_string();
                (id, it)
            })
            .collect();

        let a = by_id
            .get(&mapping_a.to_string())
            .expect("mapping A must be in the list");
        let b = by_id
            .get(&mapping_b.to_string())
            .expect("mapping B must be in the list");

        assert!(a["grantedRoleIds"].is_array());
        let a_ids: Vec<Uuid> = a["grantedRoleIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| Uuid::parse_str(v.as_str().unwrap()).unwrap())
            .collect();
        assert_eq!(a_ids, vec![role_a]);

        assert!(b["grantedRoleIds"].is_array());
        assert!(b["grantedRoleIds"].as_array().unwrap().is_empty());
    }

    // =========================================================================
    // 6. PUT batch rejects cross-realm role id → 400 role_not_in_realm
    // =========================================================================

    /// User Story: US-PW-001 (realm-membership validation)
    /// Covers: design §4.2.2 (400 role_not_in_realm), §5.2 (RoleNotInRealm),
    ///         §6.1 M1, §6.3
    #[test_context(PaywallM1Context)]
    #[tokio::test]
    async fn test_batch_update_rejects_cross_realm_role_id(ctx: &mut PaywallM1Context) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "pw1-crossrealm@test.com").await;
        let realm_id = ctx._realm_id.clone();
        let client_id = ctx._client_id.clone();

        let mapping_id = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_pw1_cross",
            None,
            "pw-cross-key",
            Some("one_time"),
            None,
            Some(100),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await;

        // A role in a DIFFERENT realm (distinct realm_id string). `roles.realm_id`
        // has no FK; `validate_granted_role_ids` does a string compare, so a
        // hand-inserted foreign role is a faithful cross-realm stand-in.
        let foreign_realm_id = format!("realm_foreign_{}", Uuid::now_v7());
        let foreign_role = insert_foreign_role(ctx, &foreign_realm_id, &client_id).await;

        let (status, json) = put_batch_granted_role_ids(
            app.clone(),
            &realm_id,
            &token,
            "stripe",
            "prod_pw1_cross",
            mapping_id,
            Some(vec![foreign_role]),
        )
        .await;

        // EXACT 400 BAD_REQUEST — no is_client_error() weakening.
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Structured body.
        assert_eq!(json["code"], "role_not_in_realm");
        assert_eq!(json["roleId"], foreign_role.to_string());
        assert_eq!(json["realmId"], realm_id);

        // No partial write — the whole batch was rejected before persisting.
        assert!(
            fetch_mapping_granted_role_ids(ctx, mapping_id)
                .await
                .is_empty(),
            "cross-realm batch must not persist anything"
        );
    }

    // =========================================================================
    // 7. PUT batch accepts empty grantedRoleIds on a fresh mapping (pure-points)
    // =========================================================================

    /// User Story: US-PW-001 (pure-points package round-trips; dimension not required)
    /// Covers: design §1.3 (orthogonal: empty role + points = pure points), §6.1 M1
    #[test_context(PaywallM1Context)]
    #[tokio::test]
    async fn test_batch_update_accepts_empty_granted_role_ids_on_fresh_mapping(
        ctx: &mut PaywallM1Context,
    ) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "pw1-purepoints@test.com").await;
        let realm_id = ctx._realm_id.clone();

        // Fresh mapping; granted_role_ids defaults to '{}' on the column.
        let mapping_id = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_pw1_pure",
            None,
            "pw-pure-key",
            Some("one_time"),
            None,
            None,
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await;
        assert!(
            fetch_mapping_granted_role_ids(ctx, mapping_id)
                .await
                .is_empty()
        );

        // PUT batch with grantedRoleIds: [] and a topup pointRules grant.
        // (`pointsPerPeriod` was removed; points config is now `pointRules`.)
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            &realm_id,
        )
        .await;
        let update_row = json!({
            "mappingId": mapping_id,
            "grantedRoleIds": [],
            "pointRules": [{
                "bucketId": bucket_id,
                "triggerSources": ["topup"],
                "grantMode": "fixed",
                "pointsAmount": 500
            }],
        });
        let (status, _json) = put_batch_update(
            app.clone(),
            &realm_id,
            &token,
            "stripe",
            "prod_pw1_pure",
            update_row,
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        // DB: empty role grant, points persisted.
        assert!(
            fetch_mapping_granted_role_ids(ctx, mapping_id)
                .await
                .is_empty()
        );
        assert_eq!(fetch_mapping_points(ctx, mapping_id).await, Some(500));
    }

    // =========================================================================
    // 8. PUT batch with multiple roles grants all (UUID[] array column)
    // =========================================================================

    /// User Story: US-PW-001 (multi-role binding, one-to-many)
    /// Covers: design §1.4 (multi role), §4.3.2 (UUID[]), §6.1 M1
    #[test_context(PaywallM1Context)]
    #[tokio::test]
    async fn test_batch_update_with_multiple_roles_grants_all(ctx: &mut PaywallM1Context) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "pw1-multi@test.com").await;
        let realm_id = ctx._realm_id.clone();

        let mapping_id = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_pw1_multi",
            None,
            "pw-multi-key",
            Some("one_time"),
            None,
            Some(100),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await;

        let role_a = create_role(ctx, &realm_id, &token, "pw1-multi-a", "multi role a").await;
        let role_b = create_role(ctx, &realm_id, &token, "pw1-multi-b", "multi role b").await;
        let role_c = create_role(ctx, &realm_id, &token, "pw1-multi-c", "multi role c").await;

        let (status, json) = put_batch_granted_role_ids(
            app.clone(),
            &realm_id,
            &token,
            "stripe",
            "prod_pw1_multi",
            mapping_id,
            Some(vec![role_a, role_b, role_c]),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        // DB column holds exactly 3 UUIDs (order-insensitive).
        let mut db_ids = fetch_mapping_granted_role_ids(ctx, mapping_id).await;
        let mut want = vec![role_a, role_b, role_c];
        db_ids.sort();
        want.sort();
        assert_eq!(db_ids, want);
        assert_eq!(db_ids.len(), 3);

        // Response also surfaces 3.
        let resp_count = json["prices"][0]["grantedRoleIds"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        assert_eq!(resp_count, 3);
    }
}
