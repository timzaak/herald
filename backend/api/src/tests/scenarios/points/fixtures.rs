// =============================================================================
// Points System Test Fixtures
// =============================================================================
//
// Helper functions for creating test data in the Points system.
//
// **Usage**:
// ```rust
// use crate::tests::scenarios::points::fixtures::*;
//
// let user_id = create_test_user(&pool, &realm_id, "test@example.com").await;
// let wallet_id = create_test_points_wallet(&pool, user_id, 1000).await;
// ```
//
// =============================================================================

use herald_core::domain::client_api_keys::services::ClientApiKeyService;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Create a test user with authentication
///
/// Returns the user ID
pub async fn create_test_user_with_auth(
    pool: &PgPool,
    realm_id: &str,
    email: &str,
    password: &str,
) -> Uuid {
    let user_uuid = Uuid::now_v7();
    let password_hash =
        bcrypt::hash(password, bcrypt::DEFAULT_COST).expect("Failed to hash password");

    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, $4, 1)",
    )
    .bind(user_uuid)
    .bind(realm_id)
    .bind(email)
    .bind(&password_hash)
    .execute(pool)
    .await
    .expect("Failed to create test user");

    // Assign user role to grant points.view permission
    // Get the user role
    let user_role_id: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM roles WHERE name = 'user' AND realm_id = $1 LIMIT 1")
            .bind(realm_id)
            .fetch_optional(pool)
            .await
            .expect("Failed to find user role");

    if let Some(role_id) = user_role_id {
        // Get the admin-web-console client_id for user role assignment
        let client_id: Option<String> =
            sqlx::query_scalar("SELECT client_id FROM client_app WHERE client_id = 'admin-web-console' AND realm_id = $1 LIMIT 1")
                .bind(realm_id)
                .fetch_optional(pool)
                .await
                .expect("Failed to find admin-web-console client_app");

        if let Some(client_id) = client_id {
            sqlx::query(
                "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
                 VALUES ($1, $2, $3, $4, $5, $6, $2::text)
                 ON CONFLICT DO NOTHING",
            )
            .bind(Uuid::now_v7())
            .bind(user_uuid)
            .bind(role_id)
            .bind(realm_id)
            .bind(&client_id)
            .bind(herald_core::domain::authorization::principal_types::USER)
            .execute(pool)
            .await
            .expect("Failed to assign user role");
        }
    }

    user_uuid
}

/// Create a test user
///
/// Returns the user ID
pub async fn create_test_user(pool: &PgPool, realm_id: &str, email: &str) -> Uuid {
    create_test_user_with_auth(pool, realm_id, email, "password123").await
}

/// Create a test points account
///
/// Returns the account ID
///
/// NOTE: This creates both the points_wallets entry and the corresponding
/// points_credit_ledger entries to support the new ledger-based points system.
pub async fn create_test_points_wallet(pool: &PgPool, user_id: Uuid, balance: i64) -> Uuid {
    // Get user's realm_id
    let realm_id: String = sqlx::query_scalar("SELECT realm_id FROM account WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("Failed to get user realm_id");

    // Ensure a legacy credit bucket exists for this realm so the NOT NULL
    // points_wallets.bucket_id / points_credit_ledger.bucket_id constraints hold.
    let bucket_id =
        crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(pool, &realm_id).await;

    let wallet_id = Uuid::now_v7();

    // Create the points_wallets entry. Under point-time the wallet
    // has no Stored balance columns; available balance is derived from
    // `points_credit_ledger` (seeded below). `total_subscription_granted`
    // is a retained lifetime-analytics column.
    sqlx::query(
        "INSERT INTO points_wallets (id, user_id, realm_id, bucket_id, total_subscription_granted, total_topup_granted, total_recharged, total_consumed, status)
         VALUES ($1, $2, $3, $4, $5, 0, $5, 0, 'active')
         ON CONFLICT (realm_id, user_id, bucket_id) DO NOTHING",
    )
    .bind(wallet_id)
    .bind(user_id)
    .bind(&realm_id)
    .bind(bucket_id)
    .bind(balance)
    .execute(pool)
    .await
    .expect("Failed to create points account");

    // Re-read in case a wallet already existed for this (realm, user, bucket) pool
    // and our minted id collided with the unique row.
    let wallet_id: Uuid = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM points_wallets WHERE realm_id = $1 AND user_id = $2 AND bucket_id = $3",
    )
    .bind(&realm_id)
    .bind(user_id)
    .bind(bucket_id)
    .fetch_one(pool)
    .await
    .expect("Failed to fetch points wallet after ensure");

    // Create corresponding credit ledger entry for subscription credits
    // This is required for the new ledger-based points system
    let ledger_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_credit_ledger (id, user_id, realm_id, bucket_id, credit_type, source_type, source_id, granted_amount, used_amount, revoked_amount, status)
         VALUES ($1, $2, $3, $4, 'subscription_credit', 'system_grant', $5, $6, 0, 0, 'active')",
    )
    .bind(ledger_id)
    .bind(user_id)
    .bind(&realm_id)
    .bind(bucket_id)
    .bind(format!("test-grant-{}", wallet_id))
    .bind(balance)
    .execute(pool)
    .await
    .expect("Failed to create credit ledger entry");

    wallet_id
}

/// Create a test transaction
///
/// Returns the transaction ID
#[allow(clippy::too_many_arguments)]
pub async fn create_test_transaction(
    pool: &PgPool,
    wallet_id: Uuid,
    user_id: Uuid,
    transaction_type: &str,
    amount: i64,
    balance_after: i64,
    description: Option<&str>,
    client_app_id: Option<Uuid>,
) -> Uuid {
    let row = sqlx::query("SELECT realm_id, bucket_id FROM points_wallets WHERE id = $1")
        .bind(wallet_id)
        .fetch_one(pool)
        .await
        .expect("Failed to get account realm_id / bucket_id");
    let realm_id: String = row.get("realm_id");
    let bucket_id: Uuid = row.get("bucket_id");

    let transaction_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO points_transactions (id, wallet_id, user_id, realm_id, bucket_id, type, amount, balance_after, description, client_app_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(transaction_id)
    .bind(wallet_id)
    .bind(user_id)
    .bind(&realm_id)
    .bind(bucket_id)
    .bind(transaction_type)
    .bind(amount)
    .bind(balance_after)
    .bind(description)
    .bind(client_app_id)
    .execute(pool)
    .await
    .expect("Failed to create transaction");

    transaction_id
}

/// Create a test entitlement mapping.
///
/// Distribution-rules model: grant config lives in `points_distribution_rules`,
/// so the mapping row carries no grant columns (mirrors the bare-row
/// `setup_test_entitlement_mapping`). The Points-v1 scenario callers either
/// recharge points explicitly via `points_service` (test_01) or drive grants
/// through the admin API / inline mapping INSERTs (test_62); none relies
/// on a mapping-owned rule being seeded here, so no rule is created.
/// `points_per_period` is retained in the signature for call-site stability.
///
/// Returns the mapping ID.
pub async fn create_test_entitlement_mapping(
    pool: &PgPool,
    realm_id: &str,
    entitlement_name: &str,
    _points_per_period: i64,
) -> Uuid {
    let mapping_id = Uuid::now_v7();
    let external_product_id = format!("prod_test_{}", entitlement_name);

    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key, enabled)
         VALUES ($1, $2, 'creem', $3, $4, true)",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .bind(&external_product_id)
    .bind(mapping_id.to_string())
    .execute(pool)
    .await
    .expect("Failed to create entitlement mapping");

    mapping_id
}

/// Configure a test entitlement mapping points policy.
///
/// In the distribution-rules model the grant policy lives on
/// `points_distribution_rules`, not on `provider_entitlement_mappings`; the
/// removed mapping columns (`points_per_period` / `validity_days` /
/// `quota_windows`) no longer exist. The Points-v1 callers either recharge
/// explicitly or assert the admin API response, so they do not depend on a
/// rule being configured here. The function is retained (signature unchanged)
/// for call-site stability and now only refreshes `updated_at`.
///
/// Returns the mapping ID.
pub async fn configure_test_entitlement_points(
    pool: &PgPool,
    realm_id: &str,
    mapping_id: Uuid,
    _points_per_period: i64,
    _validity_days: i64,
) -> Uuid {
    sqlx::query(
        "UPDATE provider_entitlement_mappings
         SET updated_at = NOW()
         WHERE id = $1 AND realm_id = $2",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .execute(pool)
    .await
    .expect("Failed to touch entitlement mapping");

    mapping_id
}

/// Create a test subscription
///
/// Returns the subscription ID
pub async fn create_test_subscription(
    pool: &PgPool,
    _user_id: Uuid,
    mapping_id: Uuid,
    realm_id: &str,
) -> Uuid {
    let subscription_id = Uuid::now_v7();
    let client_app_id = Uuid::now_v7();
    let client_id = format!("client-{}", Uuid::now_v7());
    let user_id = create_test_user(
        pool,
        realm_id,
        &format!("subscription-fixture-owner-{}@test.com", subscription_id),
    )
    .await;

    sqlx::query(
        "INSERT INTO client_app (id, realm_id, client_id, name, enabled)
         VALUES ($1, $2, $3, $4, true)",
    )
    .bind(client_app_id)
    .bind(realm_id)
    .bind(&client_id)
    .bind("Test Subscription Client App")
    .execute(pool)
    .await
    .expect("Failed to create client app for subscription");

    // `subscription.bucket_id` was removed by the distribution-rules refactor
    // (grant routing is configured via distribution rules), so no bucket is
    // bound here. `billing_type` is NOT NULL (0011_pay_model).
    sqlx::query(
        "INSERT INTO subscription
            (id, realm_id, user_id, external_subscription_id, external_product_id, payment_provider,
             status, entitlement_key, current_period_start, current_period_end,
             cancel_at_period_end, client_app_id, created_at, updated_at, billing_type)
         VALUES ($1, $2, $3, $4, $5, 'creem', 'active', $6, NOW(), NOW() + INTERVAL '30 days',
                 false, $7, NOW(), NOW(), 'recurring')",
    )
    .bind(subscription_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(format!("ext-sub-{}", subscription_id))
    .bind("test_product_id")
    .bind(mapping_id.to_string())
    .bind(client_app_id)
    .execute(pool)
    .await
    .expect("Failed to create subscription");

    subscription_id
}

/// Create a test API Key for a client app
///
/// Returns the API Key string
pub async fn create_test_api_key(pool: &PgPool, realm_id: &str, client_app_id: Uuid) -> String {
    // 1. Generate API Key (UUID v7) using Argon2-based service
    let api_key_plaintext = ClientApiKeyService::generate_api_key();

    // 2. First create a client app if it doesn't exist
    sqlx::query(
        "INSERT INTO client_app (id, realm_id, client_id, name, enabled)
         VALUES ($1, $2, $3, $4, true)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(client_app_id)
    .bind(realm_id)
    .bind(format!("client-{}", client_app_id))
    .bind("Test Client App")
    .execute(pool)
    .await
    .expect("Failed to create client app");

    // 2b. Attach this client app to the realm's legacy credit bucket so the
    // consume-path coverage resolution includes it.
    crate::tests::helpers::points_helpers::attach_client_app_to_legacy_bucket(
        pool,
        realm_id,
        client_app_id,
    )
    .await;

    // 3. Hash API Key using Argon2 (same as production)
    let api_key_hash = ClientApiKeyService::hash_api_key(&api_key_plaintext);

    // 4. Store API key with all required fields including client_app_id
    let api_key_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO client_api_keys (id, realm_id, api_key_hash, client_app_id, name, enabled, created_at, last_used_at)
         VALUES ($1, $2, $3, $4, $5, true, NOW(), NULL)",
    )
    .bind(api_key_id)
    .bind(realm_id)
    .bind(&api_key_hash)
    .bind(client_app_id)
    .bind("Test API Key")
    .execute(pool)
    .await
    .expect("Failed to create API key");

    // 5. Grant ext endpoint permissions to the API key
    let client_id = format!("client-{}", client_app_id);
    herald_test_support::helpers::grant_api_key_permissions(
        pool,
        realm_id,
        &client_id,
        &api_key_id.to_string(),
        &[
            ("billing", "view"),
            ("points", "view"),
            ("points", "manage"),
        ],
    )
    .await;

    api_key_plaintext
}

/// Helper function to create a test client app
///
/// Returns the client app ID
pub async fn create_test_client_app(pool: &PgPool, realm_id: &str) -> Uuid {
    let client_app_id = Uuid::now_v7();
    let client_id = format!("client-{}", Uuid::now_v7());

    sqlx::query(
        "INSERT INTO client_app (id, realm_id, client_id, name, enabled)
         VALUES ($1, $2, $3, $4, true)",
    )
    .bind(client_app_id)
    .bind(realm_id)
    .bind(&client_id)
    .bind("Test Client App")
    .execute(pool)
    .await
    .expect("Failed to create client app");

    // Attach this client app to the realm's legacy credit bucket so the
    // consume-path coverage resolution includes it.
    crate::tests::helpers::points_helpers::attach_client_app_to_legacy_bucket(
        pool,
        realm_id,
        client_app_id,
    )
    .await;

    client_app_id
}
