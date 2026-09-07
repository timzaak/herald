// =============================================================================
// Scenario Tests: Legal domain repository + service layer
// =============================================================================
//
// Exercises the legal agreement / consent use-cases directly against the
// schema-isolated Postgres database (no HTTP — HTTP is BE-T02). All tables come
// from the BE-D01 migration (`legal_agreement_version` + `user_agreement_consent`
// + the seeded platform-default rows); no second DDL is maintained here.
//
// User stories (docs/user-stories/core/legal-consent-account-deletion.md):
//   - US-RU-011 / US-RU-015  consent record (idempotent upsert)
//   - US-RU-012              version change → reconsent gate
//   - US-RU-013              view current effective agreement
//   - US-RA-019              publish / revert produces a new version
//
// The `LegalService` is taken from `AppState` (wired in BE-D05), which is the
// same concrete `LegalService<PostgresLegalAgreementRepository,
// PostgresUserConsentRepository, PostgresAuditEventRepository>` production uses.
// =============================================================================

use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use herald_core::domain::audit::{ActorType, AuditContext};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::legal::entities::{AgreementSource, AgreementType, ConsentSource};
use sqlx::Row;
use test_context::test_context;
use uuid::Uuid;

// =============================================================================
// Helpers
// =============================================================================

/// Build an `AuditContext` for a normal user. Threaded into `record_consent`
/// / `publish_custom` / `revert_to_default` so each audited call records
/// request-scoped context (ip / ua / trace).
fn make_audit_actor_meta(user_id: Uuid) -> AuditContext {
    AuditContext {
        actor_id: user_id.to_string(),
        actor_type: Some(ActorType::User),
        actor_name: Some(format!("test-user-{user_id}")),
        ip_address: Some("203.0.113.10".to_string()),
        user_agent: Some("legal-scene-test/1.0".to_string()),
        trace_id: Some(format!("trace-{user_id}")),
    }
}

/// Insert a Normal account + profile directly via SQL, bypassing the register
/// HTTP path. Returns the new account id. Used by consent tests so they depend
/// only on the legal tables, not on the auth pipeline.
async fn insert_account_and_profile(ctx: &TestContext, realm_id: &str) -> Uuid {
    let user_id = Uuid::now_v7();
    let email = format!("legal-scene-{user_id}@scene.test");

    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, $4, 1)",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(&email)
    .bind("scene-test-hash")
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to insert account for legal scene test");

    sqlx::query(
        "INSERT INTO profile (id, realm_id, nickname)
         VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(format!("legal-scene-{user_id}"))
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to insert profile for legal scene test");

    user_id
}

/// Read the `consented_version_id` + `consented_at` for a (user, type) pair
/// straight from the table — used to assert the upsert refresh semantics.
async fn read_consent_row(
    ctx: &TestContext,
    user_id: Uuid,
    agreement_type: &str,
) -> (Uuid, chrono::DateTime<chrono::Utc>) {
    let row = sqlx::query(
        "SELECT consented_version_id, consented_at
         FROM user_agreement_consent
         WHERE user_id = $1 AND agreement_type = $2",
    )
    .bind(user_id)
    .bind(agreement_type)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("consent row must exist");
    (
        row.get::<Uuid, _>("consented_version_id"),
        row.get::<chrono::DateTime<chrono::Utc>, _>("consented_at"),
    )
}

/// Count `user_agreement_consent` rows for a (user, type) — the upsert must
/// keep exactly one.
async fn count_consent_rows(ctx: &TestContext, user_id: Uuid, agreement_type: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_agreement_consent
         WHERE user_id = $1 AND agreement_type = $2",
    )
    .bind(user_id)
    .bind(agreement_type)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("Failed to count consent rows")
}

/// Count `agreement.consent` audit rows for a realm — each consented item must
/// produce exactly one audit event.
async fn count_agreement_consent_audit(ctx: &TestContext, realm_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'agreement.consent'",
    )
    .bind(realm_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("Failed to count agreement.consent audit events")
}

// =============================================================================
// Scenario 1: current_effective falls back to platform default
// =============================================================================

/// User Story: US-RU-013 (view current effective agreement)
/// Covers: effective resolution falls back to the seeded
/// platform-default template (`realm_id IS NULL`, `source = default`) when the
/// realm has no custom version.
///
/// WHY this matters: a realm that never published a custom agreement must still
/// be governed by the platform baseline; resolving to `None` here would
/// silently exempt the realm from any consent gate.
#[test_context(TestContext)]
#[tokio::test]
async fn test_current_effective_falls_back_to_default_when_no_custom(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    let effective = svc
        .current_effective(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("current_effective must resolve");

    let version = effective.expect("seeded default ToS row must exist");
    assert_eq!(
        version.source,
        AgreementSource::Default,
        "no custom publish → effective must be the platform default"
    );
    assert!(
        version.realm_id.is_none(),
        "platform default row carries realm_id IS NULL"
    );
    assert_eq!(version.agreement_type, AgreementType::TermsOfService);
}

// =============================================================================
// Scenario 2: current_effective prefers a realm custom version over default
// =============================================================================

/// User Story: US-RA-019 (custom publish overrides default)
/// Covers: once a realm publishes a custom version, effective
/// resolution must prefer it over the platform default.
///
/// WHY this matters: the realm's own published text is the legally binding one
/// for its users; silently falling back to the default would override the
/// realm admin's explicit publish.
#[test_context(TestContext)]
#[tokio::test]
async fn test_current_effective_prefers_custom_over_default(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    svc.publish_custom(
        &realm_id,
        AgreementType::TermsOfService,
        serde_json::json!({ "zh-CN": "realm custom ToS body" }),
        Some("v1".to_string()),
        "admin@scene",
        make_audit_actor_meta(Uuid::now_v7()),
    )
    .await
    .expect("publish_custom must succeed");

    let effective = svc
        .current_effective(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("current_effective must resolve");

    let version = effective.expect("custom ToS must be effective");
    assert_eq!(
        version.source,
        AgreementSource::Custom,
        "custom publish must shadow the platform default"
    );
    assert_eq!(version.realm_id.as_deref(), Some(realm_id.as_str()));
}

// =============================================================================
// Scenario 3: current_effective picks the max version_no
// =============================================================================

/// User Story: US-RA-019 (each publish is a new effective version)
/// Covers: effective resolution
/// selects the row with the greatest `version_no` within the realm's custom
/// rows.
///
/// WHY this matters: with two custom versions published, returning the older
/// one would silently demote the admin's latest edit and re-bind users to
/// stale text.
#[test_context(TestContext)]
#[tokio::test]
async fn test_current_effective_picks_max_version_no(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    let _v1 = svc
        .publish_custom(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "zh-CN": "first custom ToS" }),
            None,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("first publish must succeed");

    let v2 = svc
        .publish_custom(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "zh-CN": "second custom ToS" }),
            None,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("second publish must succeed");

    let effective = svc
        .current_effective(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("current_effective must resolve")
        .expect("custom ToS must be effective");

    assert_eq!(
        effective.version_no, v2.version_no,
        "effective must be the highest version_no"
    );
    assert_eq!(effective.version_no, 2);
    assert_eq!(effective.id, v2.id);
}

// =============================================================================
// Scenario 4: publish_custom produces monotonic version_no + fresh ids
// =============================================================================

/// User Story: US-RA-019
/// Covers: successive publishes must yield strictly increasing
/// `version_no`, distinct `id` (uuid v7, never reused), and non-decreasing
/// `published_at`.
///
/// WHY this matters: the version `id` is the consent token stored on
/// `user_agreement_consent.consented_version_id`; reusing or rewinding an id
/// would let an older consent silently satisfy a newer version, defeating the
/// reconsent gate.
#[test_context(TestContext)]
#[tokio::test]
async fn test_publish_custom_monotonic_version_no_and_new_id(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    let v1 = svc
        .publish_custom(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "zh-CN": "first" }),
            None,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("first publish must succeed");

    let v2 = svc
        .publish_custom(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "zh-CN": "second" }),
            None,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("second publish must succeed");

    assert!(
        v2.version_no > v1.version_no,
        "version_no must be strictly monotonic across publishes"
    );
    assert_ne!(v1.id, v2.id, "each publish must mint a fresh version id");
    assert!(
        v2.published_at >= v1.published_at,
        "published_at must not rewind"
    );
}

// =============================================================================
// Scenario 5: revert_to_default appends a live-default follow marker
// =============================================================================

/// User Story: US-RA-019 (revert is a version change, not a deletion)
/// Covers: revert is a live-default follow marker — reverting a
/// realm to the default does NOT delete prior custom rows and does NOT rewind
/// the version token id. Instead a realm-scoped default-follow marker is
/// appended (new id, monotonic version_no, source = default, body = current
/// platform default), so effective resolution follows the live platform
/// default after the marker instead of a permanently detached snapshot.
///
/// WHY this matters: rewinding the id / deleting rows would let an old consent
/// row silently match the "reverted" version and bypass reconsent; the marker
/// semantic is what makes revert observable to users as a real version change
/// while keeping later platform-default updates visible to the realm.
#[test_context(TestContext)]
#[tokio::test]
async fn test_revert_to_default_snapshots_into_new_custom_version(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    // Establish a realm custom version (version_no = 1 for this realm).
    let custom = svc
        .publish_custom(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "zh-CN": "realm-specific custom body" }),
            None,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("publish_custom must succeed");
    let prior_version_no = custom.version_no;
    let prior_id = custom.id;

    // Resolve the platform default body to compare the snapshot against.
    let default = svc
        .list_history(&realm_id, AgreementType::TermsOfService, 50)
        .await
        .expect("list_history must succeed")
        .into_iter()
        .find(|v| v.source == AgreementSource::Default)
        .expect("a seeded platform default row must exist");

    let reverted = svc
        .revert_to_default(
            &realm_id,
            AgreementType::TermsOfService,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("revert_to_default must succeed");

    // Live-default follow marker: new id, monotonic version_no,
    // source = default (follows the platform default, not a custom snapshot),
    // body == current default template.
    assert_eq!(
        reverted.version_no,
        prior_version_no + 1,
        "revert must advance version_no, not rewind it"
    );
    assert_ne!(
        reverted.id, prior_id,
        "revert must mint a fresh version id, never reuse a prior one"
    );
    assert_ne!(
        reverted.id, default.id,
        "revert must not surface the platform default row directly"
    );
    assert_eq!(
        reverted.source,
        AgreementSource::Default,
        "revert appends a default-follow marker, not a custom snapshot"
    );
    assert_eq!(
        reverted.content, default.content,
        "the marker body must equal the current platform default template"
    );

    // Append-only: the prior custom row is still in the table.
    let still_present: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM legal_agreement_version WHERE id = $1")
            .bind(prior_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("must query legal_agreement_version");
    assert_eq!(
        still_present, 1,
        "append-only: prior custom row must not be deleted on revert"
    );
}

// =============================================================================
// Scenario 6: record_consent upsert is idempotent and refreshes the timestamp
// =============================================================================

/// User Story: US-RU-011 / US-RU-015 (consent record idempotent upsert)
/// Covers: record_consent — repeated consent for the same
/// (user, type) at the current version is an upsert: exactly one row remains,
/// `consented_version_id` is the latest consent target, and `consented_at` is
/// refreshed so re-consent is observable.
///
/// WHY this matters: re-consenting must not accumulate duplicate rows (would
/// corrupt the gate read) nor silently keep an old timestamp (would hide that
/// the user re-confirmed).
#[test_context(TestContext)]
#[tokio::test]
async fn test_record_consent_upsert_refreshes_version(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    let user_id = insert_account_and_profile(ctx, &realm_id).await;
    let current = svc
        .current_effective(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("current_effective must resolve")
        .expect("seeded default ToS must exist");

    svc.record_consent(
        user_id,
        &realm_id,
        vec![(AgreementType::TermsOfService, current.id)],
        ConsentSource::Explicit,
        make_audit_actor_meta(user_id),
    )
    .await
    .expect("first record_consent must succeed");

    let (version_after_first, ts_after_first) =
        read_consent_row(ctx, user_id, "terms_of_service").await;

    // Sleep so the refreshed timestamp is observably later (DB now() has
    // microsecond resolution but we want a deterministic gap).
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    svc.record_consent(
        user_id,
        &realm_id,
        vec![(AgreementType::TermsOfService, current.id)],
        ConsentSource::Explicit,
        make_audit_actor_meta(user_id),
    )
    .await
    .expect("second record_consent must succeed");

    let row_count = count_consent_rows(ctx, user_id, "terms_of_service").await;
    assert_eq!(row_count, 1, "upsert must keep exactly one consent row");

    let (version_after_second, ts_after_second) =
        read_consent_row(ctx, user_id, "terms_of_service").await;
    assert_eq!(
        version_after_first, version_after_second,
        "consented_version_id stays the current version on re-consent"
    );
    assert_eq!(version_after_second, current.id);
    assert!(
        ts_after_second >= ts_after_first,
        "consented_at must be refreshed on re-consent"
    );
}

// =============================================================================
// Scenario 7: record_consent rejects a stale version id
// =============================================================================

/// User Story: US-RU-012 (version change → reconsent) — gate side
/// Covers: StaleVersion → CoreError::Conflict —
/// `record_consent` must refuse a `version_id` that is not the current
/// effective one. Otherwise a user could "consent" to an obsolete version and
/// bypass the reconsent gate after an admin published a newer one.
///
/// WHY this matters: the version id is the consent token; accepting a stale
/// token is exactly the attack the StaleVersion gate exists to prevent.
#[test_context(TestContext)]
#[tokio::test]
async fn test_record_consent_rejects_stale_version(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    // Publish V_old, then V_current, so V_old is provably stale.
    let v_old = svc
        .publish_custom(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "zh-CN": "old version body" }),
            None,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("first publish must succeed");

    let _v_current = svc
        .publish_custom(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "zh-CN": "newer version body" }),
            None,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("second publish must succeed");

    let user_id = insert_account_and_profile(ctx, &realm_id).await;

    let err = svc
        .record_consent(
            user_id,
            &realm_id,
            vec![(AgreementType::TermsOfService, v_old.id)],
            ConsentSource::Reconsent,
            make_audit_actor_meta(user_id),
        )
        .await
        .expect_err("stale version_id must be rejected");
    assert!(
        matches!(err, CoreError::Conflict(_)),
        "stale consent must surface as a Conflict-class error, got: {err:?}"
    );

    // The rejected write must not have persisted a consent row.
    let row_count = count_consent_rows(ctx, user_id, "terms_of_service").await;
    assert_eq!(
        row_count, 0,
        "rejected consent must not write a user_agreement_consent row"
    );
}

// =============================================================================
// Scenario 8: record_consent writes one audit event per item
// =============================================================================

/// User Story: US-RU-011 (consent is auditable per agreement)
/// Covers: per-item audit — each consented agreement type
/// produces its own `agreement.consent` audit row under category `compliance`,
/// carrying `agreement_type`, `version_id`, and `source` (matching the
/// ConsentSource) in `details`.
///
/// WHY this matters: compliance evidence is per-agreement; collapsing both
/// types into one audit row would lose which text the user actually agreed to,
/// and a missing `source` would erase why the consent was collected.
#[test_context(TestContext)]
#[tokio::test]
async fn test_record_consent_writes_per_item_audit(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    let user_id = insert_account_and_profile(ctx, &realm_id).await;

    let tos = svc
        .current_effective(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("current_effective ToS must resolve")
        .expect("seeded default ToS must exist");
    let pp = svc
        .current_effective(&realm_id, AgreementType::PrivacyPolicy)
        .await
        .expect("current_effective PrivacyPolicy must resolve")
        .expect("seeded default PrivacyPolicy must exist");

    let baseline = count_agreement_consent_audit(ctx, &realm_id).await;

    svc.record_consent(
        user_id,
        &realm_id,
        vec![
            (AgreementType::TermsOfService, tos.id),
            (AgreementType::PrivacyPolicy, pp.id),
        ],
        ConsentSource::Reconsent,
        make_audit_actor_meta(user_id),
    )
    .await
    .expect("record_consent for both types must succeed");

    let after = count_agreement_consent_audit(ctx, &realm_id).await;
    assert_eq!(
        after - baseline,
        2,
        "exactly one agreement.consent audit row per consented item"
    );

    // Verify the details payload carries the per-item facts we rely on for
    // compliance evidence.
    let rows = sqlx::query(
        "SELECT details FROM audit_events
         WHERE realm_id = $1 AND action = 'agreement.consent'
         ORDER BY created_at DESC LIMIT 2",
    )
    .bind(&realm_id)
    .fetch_all(&ctx.app_state.pool)
    .await
    .expect("must query audit_events");

    let mut sources = Vec::new();
    let mut types = Vec::new();
    for row in &rows {
        let details: serde_json::Value = row.get("details");
        sources.push(
            details
                .get("source")
                .and_then(|v| v.as_str())
                .expect("details.source must be present")
                .to_string(),
        );
        types.push(
            details
                .get("agreement_type")
                .and_then(|v| v.as_str())
                .expect("details.agreement_type must be present")
                .to_string(),
        );
        assert!(
            details.get("version_id").is_some(),
            "details.version_id must be present"
        );
    }
    assert!(
        sources.iter().all(|s| s == "reconsent"),
        "details.source must reflect the ConsentSource passed in"
    );
    assert!(types.contains(&"terms_of_service".to_string()));
    assert!(types.contains(&"privacy_policy".to_string()));
}

// =============================================================================
// Scenario 9: consent_status flips to needs_reconsent after a publish
// =============================================================================

/// User Story: US-RU-012 / US-RA-019 (publish triggers user reconsent)
/// Covers: consent_status needsReconsent — after the user
/// consented to the current version, an admin publishing a newer version must
/// flip `needs_reconsent` to true, with `current_version_id` advanced to the
/// new version and `consented_version_id` still pointing at the old one.
///
/// WHY this matters: this is the reconsent gate's whole purpose — if a publish
/// did not flip this flag, users would never be prompted to accept a changed
/// agreement, defeating the legal "explicit re-consent on material change"
/// requirement.
#[test_context(TestContext)]
#[tokio::test]
async fn test_consent_status_needs_reconsent_after_publish(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    let user_id = insert_account_and_profile(ctx, &realm_id).await;

    // Consent to the current (default) version V1.
    let v1 = svc
        .current_effective(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("current_effective must resolve")
        .expect("seeded default ToS must exist");

    svc.record_consent(
        user_id,
        &realm_id,
        vec![(AgreementType::TermsOfService, v1.id)],
        ConsentSource::Explicit,
        make_audit_actor_meta(user_id),
    )
    .await
    .expect("record_consent must succeed");

    // Admin publishes V2.
    let v2 = svc
        .publish_custom(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "zh-CN": "amended ToS body" }),
            None,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("publish must succeed");

    let status = svc
        .consent_status(user_id, &realm_id)
        .await
        .expect("consent_status must resolve");
    let tos = status
        .iter()
        .find(|s| s.agreement_type == AgreementType::TermsOfService)
        .expect("ToS status item must be present");

    assert!(
        tos.needs_reconsent,
        "publishing a new version must flip needs_reconsent to true"
    );
    assert_eq!(tos.current_version_id, v2.id);
    assert_eq!(tos.consented_version_id, Some(v1.id));
}

// =============================================================================
// Scenario 10: consent_status is false when the user is up to date
// =============================================================================

/// User Story: US-RU-012 (do not re-prompt up-to-date users)
/// Covers: regression — when the user has consented to the
/// current effective version and nothing newer has been published,
/// `needs_reconsent` must be false.
///
/// WHY this matters: a false positive here would trap users in an infinite
/// reconsent loop on every request; the gate must be stable when state hasn't
/// changed.
#[test_context(TestContext)]
#[tokio::test]
async fn test_consent_status_false_when_up_to_date(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    let user_id = insert_account_and_profile(ctx, &realm_id).await;
    let current = svc
        .current_effective(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("current_effective must resolve")
        .expect("seeded default ToS must exist");

    svc.record_consent(
        user_id,
        &realm_id,
        vec![(AgreementType::TermsOfService, current.id)],
        ConsentSource::Explicit,
        make_audit_actor_meta(user_id),
    )
    .await
    .expect("record_consent must succeed");

    let status = svc
        .consent_status(user_id, &realm_id)
        .await
        .expect("consent_status must resolve");
    let tos = status
        .iter()
        .find(|s| s.agreement_type == AgreementType::TermsOfService)
        .expect("ToS status item must be present");

    assert!(
        !tos.needs_reconsent,
        "user consenting to the current version must not be re-prompted"
    );
    assert_eq!(tos.current_version_id, current.id);
    assert_eq!(tos.consented_version_id, Some(current.id));
}

// =============================================================================
// Scenario: draft lifecycle (save / get / publish-from-draft / isolation)
// =============================================================================
//
// WHY this matters: drafts are staged in a separate table and must NEVER affect
// `current_effective`, `has_custom`, the version_no sequence, or consent. Only
// `publish_from_draft` flips those. These tests encode that invariant at the
// service/repository layer.

#[test_context(TestContext)]
#[tokio::test]
async fn test_save_draft_upsert_is_idempotent_and_isolated_from_versions(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    // Effective version before any draft exists.
    let pre_effective = svc
        .current_effective(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("current_effective must resolve")
        .expect("seeded default ToS must exist");

    // No draft initially.
    assert!(
        svc.get_draft(&realm_id, AgreementType::TermsOfService)
            .await
            .expect("get_draft must resolve")
            .is_none(),
        "no draft should exist initially"
    );

    // Save once.
    let first = svc
        .save_draft(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "en": "first draft" }),
            Some("label one".to_string()),
            "admin@scene",
        )
        .await
        .expect("first save_draft must succeed");

    // Save again — overwrites (idempotent upsert), same row scope.
    let second = svc
        .save_draft(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "en": "second draft" }),
            None,
            "admin@scene",
        )
        .await
        .expect("second save_draft must succeed");

    assert_eq!(first.id, second.id, "upsert keeps the same draft id");
    assert_eq!(
        second.content,
        serde_json::json!({ "en": "second draft" }),
        "second save must overwrite content"
    );
    assert!(
        second.version_label.is_none(),
        "second save must overwrite (clear) version_label"
    );

    // CRITICAL INVARIANT: saving a draft must not move the effective version,
    // flip has_custom, or touch the version table at all.
    let post_effective = svc
        .current_effective(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("current_effective must resolve")
        .expect("effective ToS must still exist");
    assert_eq!(
        pre_effective.id, post_effective.id,
        "saving a draft must not change the effective version"
    );
    let has_custom = svc
        .has_custom(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("has_custom must resolve");
    assert!(
        !has_custom,
        "saving a draft must not flip has_custom (drafts are not published versions)"
    );
}

#[test_context(TestContext)]
#[tokio::test]
async fn test_publish_from_draft_creates_version_and_clears_draft(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    let pre_custom_max = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(version_no) FROM legal_agreement_version
         WHERE realm_id = $1 AND agreement_type = 'terms_of_service'",
    )
    .bind(&realm_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("read pre-custom max must resolve")
    .unwrap_or(0);

    // Publish with no draft → DraftNotFound (CoreError::NotFound).
    let err = svc
        .publish_from_draft(
            &realm_id,
            AgreementType::TermsOfService,
            None,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect_err("publish with no draft must fail");
    assert!(
        matches!(err, CoreError::NotFound),
        "publish with no draft must surface NotFound, got {err:?}"
    );

    // Stage a draft, then publish from it.
    svc.save_draft(
        &realm_id,
        AgreementType::TermsOfService,
        serde_json::json!({ "en": "ready to publish" }),
        Some("draft label".to_string()),
        "admin@scene",
    )
    .await
    .expect("save_draft must succeed");

    let published = svc
        .publish_from_draft(
            &realm_id,
            AgreementType::TermsOfService,
            None,
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("publish_from_draft must succeed");

    assert!(
        published.version_no as i64 > pre_custom_max,
        "published version_no ({}) must exceed prior custom max ({})",
        published.version_no,
        pre_custom_max
    );
    assert_eq!(
        published.content,
        serde_json::json!({ "en": "ready to publish" }),
        "published content must match the staged draft"
    );
    assert_eq!(
        published.version_label.as_deref(),
        Some("draft label"),
        "publish must reuse the draft's version_label when no override is given"
    );
    assert_eq!(published.source, AgreementSource::Custom);

    // Draft must be cleared after publish.
    assert!(
        svc.get_draft(&realm_id, AgreementType::TermsOfService)
            .await
            .expect("get_draft must resolve")
            .is_none(),
        "draft must be cleared after publish_from_draft"
    );
}

#[test_context(TestContext)]
#[tokio::test]
async fn test_discard_draft_is_idempotent(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    // Discarding a missing draft is a no-op.
    svc.discard_draft(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("discard of missing draft must succeed (idempotent)");

    // Save then discard.
    svc.save_draft(
        &realm_id,
        AgreementType::TermsOfService,
        serde_json::json!({ "en": "transient" }),
        None,
        "admin@scene",
    )
    .await
    .expect("save_draft must succeed");
    svc.discard_draft(&realm_id, AgreementType::TermsOfService)
        .await
        .expect("discard must succeed");
    assert!(
        svc.get_draft(&realm_id, AgreementType::TermsOfService)
            .await
            .expect("get_draft must resolve")
            .is_none(),
        "draft must be gone after discard"
    );
}

// =============================================================================
// Scenario: get_version_by_id resolves a published version with full body
// =============================================================================

/// User Story: US-RA-019 (admin views past version body)
/// Covers: the admin history "view past version" path. `list_history` only
/// returns summaries (no content body, to keep the list payload small); the
/// "view" action fetches a single version by id with its full localized
/// `content`.
///
/// WHY this matters: returning the wrong id's body (or None for a real id)
/// would either show an admin content from an unrelated version or silently
/// fail to render the body they expected to audit. The lookup is by primary
/// key so it must hit exactly one row.
#[test_context(TestContext)]
#[tokio::test]
async fn test_get_version_by_id_returns_full_body(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();

    let published = svc
        .publish_custom(
            &realm_id,
            AgreementType::TermsOfService,
            serde_json::json!({ "en": "viewable body" }),
            Some("history label".to_string()),
            "admin@scene",
            make_audit_actor_meta(Uuid::now_v7()),
        )
        .await
        .expect("publish_custom must succeed");

    let fetched = svc
        .get_version_by_id(published.id)
        .await
        .expect("get_version_by_id must resolve")
        .expect("a just-published id must resolve");
    assert_eq!(fetched.id, published.id);
    assert_eq!(fetched.version_no, published.version_no);
    assert_eq!(fetched.version_label.as_deref(), Some("history label"));
    assert_eq!(
        fetched.content,
        serde_json::json!({ "en": "viewable body" }),
        "get_version_by_id must return the full localized content body"
    );

    // An unknown id must resolve to None (handler maps this to 404), never error.
    let missing = svc
        .get_version_by_id(Uuid::now_v7())
        .await
        .expect("get_version_by_id for unknown id must not error");
    assert!(missing.is_none(), "unknown version id must resolve to None");
}
