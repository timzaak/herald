// =============================================================================
// Email Config Test Helpers
// =============================================================================
//
// Shared helpers for email configuration scenario tests.
// Provides functions for inserting, deleting, and querying email config
// via direct SQL and the email status API.
//
// =============================================================================

#![allow(dead_code)]

use axum::{
    body::Body,
    http::{Request, header},
};
use sqlx::PgPool;
use tower::ServiceExt;

/// Insert a complete Resend email configuration for a realm via direct SQL.
///
/// Inserts: provider="resend", from_address, resend_api_key (secret).
pub async fn insert_resend_email_config_direct(pool: &PgPool, realm_id: &str) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, 'email', $2, $3, $4, true, NULL, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, updated_at = NOW()",
    )
    .bind(realm_id)
    .bind("provider")
    .bind("resend")
    .bind(false)
    .execute(pool)
    .await
    .expect("Failed to insert email provider config");

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, 'email', $2, $3, $4, true, NULL, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, updated_at = NOW()",
    )
    .bind(realm_id)
    .bind("from_address")
    .bind("noreply@example.com")
    .bind(false)
    .execute(pool)
    .await
    .expect("Failed to insert email from_address config");

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, 'email', $2, $3, $4, true, NULL, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, updated_at = NOW()",
    )
    .bind(realm_id)
    .bind("resend_api_key")
    .bind("re_test_api_key_12345")
    .bind(true)
    .execute(pool)
    .await
    .expect("Failed to insert email resend_api_key config");
}

/// Insert a complete SMTP email configuration for a realm via direct SQL.
///
/// Inserts: provider="smtp", from_address, smtp_host, smtp_port, smtp_username,
/// smtp_password (secret), smtp_encryption.
pub async fn insert_smtp_email_config_direct(pool: &PgPool, realm_id: &str) {
    let smtp_keys: &[(&str, &str, bool)] = &[
        ("provider", "smtp", false),
        ("from_address", "notify@company.com", false),
        ("smtp_host", "smtp.example.com", false),
        ("smtp_port", "587", false),
        ("smtp_username", "notify@company.com", false),
        ("smtp_password", "smtp-auth-code", true),
        ("smtp_encryption", "starttls", false),
    ];

    for (key, value, is_secret) in smtp_keys {
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
             VALUES ($1, 'email', $2, $3, $4, true, NULL, NOW(), NOW())
             ON CONFLICT (realm_id, config_type, config_key)
             DO UPDATE SET config_value = EXCLUDED.config_value, updated_at = NOW()",
        )
        .bind(realm_id)
        .bind(*key)
        .bind(*value)
        .bind(*is_secret)
        .execute(pool)
        .await
        .expect("Failed to insert SMTP email config row");
    }
}

/// Delete all email configuration rows for a realm via direct SQL.
pub async fn delete_email_config_direct(pool: &PgPool, realm_id: &str) {
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'email'")
        .bind(realm_id)
        .execute(pool)
        .await
        .expect("Failed to delete email config");
}

/// GET /api/configs/email/status via the test router.
///
/// Returns the parsed JSON response body.
pub async fn get_email_status(
    app: &axum::Router,
    _realm_id: &str,
    token: &str,
) -> serde_json::Value {
    let req = Request::builder()
        .method("GET")
        .uri("/api/configs/email/status".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::OK,
        "email status endpoint should return 200"
    );
    crate::tests::response_json(resp).await
}

/// POST /api/configs/email/test with a recipient address.
///
/// Returns the raw response for the caller to assert status and body.
pub async fn send_test_email(
    app: &axum::Router,
    _realm_id: &str,
    token: &str,
    recipient: &str,
) -> axum::response::Response {
    let payload = serde_json::json!({ "recipient": recipient });
    let req = Request::builder()
        .method("POST")
        .uri("/api/configs/email/test".to_string())
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    app.clone().oneshot(req).await.unwrap()
}

/// Insert a partial SMTP email config (missing username and password)
/// to test missing-fields detection.
pub async fn insert_partial_smtp_email_config_direct(pool: &PgPool, realm_id: &str) {
    let partial_keys: &[(&str, &str, bool)] = &[
        ("provider", "smtp", false),
        ("from_address", "partial@company.com", false),
        ("smtp_host", "smtp.partial.com", false),
        ("smtp_port", "587", false),
        // Deliberately omitted: smtp_username, smtp_password
    ];

    for (key, value, is_secret) in partial_keys {
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
             VALUES ($1, 'email', $2, $3, $4, true, NULL, NOW(), NOW())
             ON CONFLICT (realm_id, config_type, config_key)
             DO UPDATE SET config_value = EXCLUDED.config_value, updated_at = NOW()",
        )
        .bind(realm_id)
        .bind(*key)
        .bind(*value)
        .bind(*is_secret)
        .execute(pool)
        .await
        .expect("Failed to insert partial SMTP config row");
    }
}
