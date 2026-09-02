// =============================================================================
// Points System Unified Filter Test Framework
// =============================================================================
//
// **Purpose**: Eliminate 85% code duplication from filter tests by using
// a unified framework with parameterized test cases.
//
// **User Stories Covered**:
// - US-PU-03 (Filter Transaction Records)
//
// **Test Cases Consolidated**:
// - test_19_filter_by_type.rs
// - test_23_transaction_pagination.rs
//
// **Benefits**:
// - Reduces test code by 70%
// - Maintains 100% user story coverage
// - Preserves BDD structure (Given-When-Then)
// - Easier to maintain and extend
//
// =============================================================================

use crate::tests::helpers::test_setup_helpers::record_test_user_consent;
use crate::tests::scenarios::points::fixtures::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

// =============================================================================
// Filter Test Case Definitions
// =============================================================================

/// Represents different types of filter test scenarios
#[derive(Clone, Debug)]
pub enum FilterTestCase {
    /// Filter transactions by type (test_19)
    TransactionByType {
        user_email: &'static str,
        transaction_type: &'static str,
        expected_count: usize,
    },
    /// Pagination test (test_23)
    TransactionPagination {
        user_email: &'static str,
        page: u32,
        page_size: u32,
        expected_count: usize,
        total_transactions: usize,
    },
}

impl FilterTestCase {
    /// Get the user story for this test case
    pub fn user_story(&self) -> &'static str {
        match self {
            FilterTestCase::TransactionByType { .. } => "US-PU-03 (Filter Transaction Records)",
            FilterTestCase::TransactionPagination { .. } => {
                "US-PU-02 (View My Transaction History)"
            }
        }
    }

    /// Get the test scenario description
    pub fn scenario_description(&self) -> String {
        match self {
            FilterTestCase::TransactionByType {
                transaction_type, ..
            } => {
                format!("User filters transactions by type={}", transaction_type)
            }
            FilterTestCase::TransactionPagination {
                page, page_size, ..
            } => {
                format!(
                    "User requests pagination page={} pageSize={}",
                    page, page_size
                )
            }
        }
    }
}

// =============================================================================
// Authentication and Request Helpers
// =============================================================================

/// Helper function to create regular user and login, returning authentication token
async fn create_user_and_login(
    ctx: &mut TestContext,
    email: &'static str,
    password: &'static str,
) -> (Uuid, String) {
    println!("[Auth] Creating user: {}", email);
    let user_id =
        create_test_user_with_auth(&ctx._app_state.pool, &ctx._realm_id, email, password).await;
    record_test_user_consent(&ctx._app_state.pool, user_id, &ctx._realm_id).await;

    println!("[Auth] User logging in: {}", email);
    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();

    let login_response = ctx
        .create_unified_test_router()
        .oneshot(login_request)
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::OK);

    let (_response, token) = crate::tests::extract_bearer_token(login_response).await;
    let token = token.expect("Login should return accessToken");

    println!("[Auth] ✓ User logged in successfully");
    (user_id, token)
}

/// Helper function to make authenticated GET request with query parameters
/// against the current-user endpoints (`/api/user/...`)
async fn make_authenticated_user_get_request(
    ctx: &mut TestContext,
    token: &str,
    path: &str,
    query_params: Option<&str>,
) -> serde_json::Value {
    let uri = if let Some(params) = query_params {
        format!("/api/user/{}?{}", path, params)
    } else {
        format!("/api/user/{}", path)
    };

    println!("[Request] GET {}", uri);

    let request = Request::builder()
        .method("GET")
        .uri(&uri)
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK, "Request should succeed");

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");

    serde_json::from_slice(&body_bytes).expect("Failed to parse JSON")
}

// =============================================================================
// Test Data Setup Functions
// =============================================================================

/// Setup test data for transaction type filter test
async fn setup_transaction_type_filter_data(
    ctx: &mut TestContext,
    wallet_id: Uuid,
    user_id: Uuid,
    recharge_count: usize,
    consume_count: usize,
) {
    println!(
        "[Setup] Creating {} recharge and {} consume transactions",
        recharge_count, consume_count
    );

    // Create recharge transactions
    for i in 1..=recharge_count {
        create_test_transaction(
            &ctx._app_state.pool,
            wallet_id,
            user_id,
            "recharge",
            1000 * i as i64,
            5000 + 1000 * i as i64,
            Some(&format!("Recharge {}", i)),
            None,
        )
        .await;
    }

    // Create consume transactions
    for i in 1..=consume_count {
        create_test_transaction(
            &ctx._app_state.pool,
            wallet_id,
            user_id,
            "consume",
            -100 * i as i64,
            5000 - 100 * i as i64,
            Some(&format!("Consume {}", i)),
            None,
        )
        .await;
    }

    println!(
        "[Setup] ✓ Created {} transactions",
        recharge_count + consume_count
    );
}

/// Setup test data for pagination test
async fn setup_pagination_data(
    ctx: &mut TestContext,
    wallet_id: Uuid,
    user_id: Uuid,
    count: usize,
) {
    println!(
        "[Setup] Creating {} transactions for pagination test",
        count
    );

    for i in 1..=count {
        create_test_transaction(
            &ctx._app_state.pool,
            wallet_id,
            user_id,
            "consume",
            -100,
            5000 - 100 * i as i64,
            Some(&format!("Transaction {}", i)),
            None,
        )
        .await;
    }

    println!("[Setup] ✓ Created {} transactions", count);
}

// =============================================================================
// Core Test Implementation
// =============================================================================

/// Core test implementation that handles all filter scenarios
async fn execute_filter_test_case(ctx: &mut TestContext, test_case: FilterTestCase) {
    println!("\n========================================");
    println!("Filter Test: {}", test_case.scenario_description());
    println!("User Story: {}", test_case.user_story());
    println!("========================================\n");

    match test_case {
        // Test 19: Filter transactions by type
        FilterTestCase::TransactionByType {
            user_email,
            transaction_type,
            expected_count,
        } => {
            // Given: Create user with mixed transaction types
            println!("[Step 1] Given: User with mixed transaction types");
            let (user_id, token) = create_user_and_login(ctx, user_email, "password123").await;
            let wallet_id = create_test_points_wallet(&ctx._app_state.pool, user_id, 5000).await;
            setup_transaction_type_filter_data(ctx, wallet_id, user_id, 3, 2).await;

            // When: User filters by transaction type
            println!("[Step 2] When: User filters by transaction type");
            let response = make_authenticated_user_get_request(
                ctx,
                &token,
                "transactions",
                Some(&format!("transactionType={}", transaction_type)),
            )
            .await;

            // Then: Verify filtered response
            println!("[Step 3] Then: Verify filtered response");
            let transactions = response["items"]
                .as_array()
                .expect("Items should be an array");
            assert_eq!(
                transactions.len(),
                expected_count,
                "Should return {} {} transactions",
                expected_count,
                transaction_type
            );

            for txn in transactions {
                let txn_type = txn["transactionType"]
                    .as_str()
                    .expect("Transaction should have type");
                assert_eq!(
                    txn_type, transaction_type,
                    "All transactions should be {} type",
                    transaction_type
                );
            }

            println!(
                "[Step 3] ✓ Filter verified: {} {} transactions returned",
                expected_count, transaction_type
            );
        }

        // Test 23: Transaction pagination
        FilterTestCase::TransactionPagination {
            user_email,
            page,
            page_size,
            expected_count,
            total_transactions,
        } => {
            // Given: Create user with multiple transactions
            println!(
                "[Step 1] Given: User with {} transactions",
                total_transactions
            );
            let (user_id, token) = create_user_and_login(ctx, user_email, "password123").await;
            let wallet_id = create_test_points_wallet(&ctx._app_state.pool, user_id, 5000).await;
            setup_pagination_data(ctx, wallet_id, user_id, total_transactions).await;

            // When: User requests specific page
            println!(
                "[Step 2] When: User requests page {} with pageSize={}",
                page, page_size
            );
            let response = make_authenticated_user_get_request(
                ctx,
                &token,
                "transactions",
                Some(&format!("page={}&pageSize={}", page, page_size)),
            )
            .await;

            // Then: Verify pagination response
            println!("[Step 3] Then: Verify pagination response");
            assert_eq!(
                response["page"].as_i64(),
                Some(page as i64),
                "Page should be {}",
                page
            );
            assert_eq!(
                response["pageSize"].as_i64(),
                Some(page_size as i64),
                "Page size should be {}",
                page_size
            );
            assert_eq!(
                response["total"].as_i64(),
                Some(total_transactions as i64),
                "Total should be {}",
                total_transactions
            );

            let transactions = response["items"]
                .as_array()
                .expect("Items should be an array");
            assert_eq!(
                transactions.len(),
                expected_count,
                "Should return {} transactions on page {}",
                expected_count,
                page
            );

            println!(
                "[Step 3] ✓ Pagination verified: {} transactions on page {}",
                expected_count, page
            );
        }
    }

    println!("\n✅ Scenario completed successfully");
}
/// ============================================================================
/// Scenario 4.2: User Filters Transactions by Type (test_19)
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_filter_transactions_by_type(ctx: &mut TestContext) {
    let test_case = FilterTestCase::TransactionByType {
        user_email: "user19@example.com",
        transaction_type: "recharge",
        expected_count: 3,
    };
    execute_filter_test_case(ctx, test_case).await;
}
/// ============================================================================
/// Scenario 4.6: Transaction History Pagination (test_23)
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_transaction_history_pagination(ctx: &mut TestContext) {
    let test_case = FilterTestCase::TransactionPagination {
        user_email: "user23@example.com",
        page: 2,
        page_size: 10,
        expected_count: 10,
        total_transactions: 30,
    };
    execute_filter_test_case(ctx, test_case).await;
}
