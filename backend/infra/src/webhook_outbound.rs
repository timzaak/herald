// Postgres repositories for outbound webhooks: subscription management +
// fan-out persistence. Raw sqlx over the shared pool (flat-module style,
// same as the payment-event lease repository).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::Row;
use uuid::Uuid;

use herald_domain::webhook_outbound::{
    OutboundEventCategory, OutboundEventRepository, OutboundEventType, WebhookOutboundError,
    WebhookSubscription, WebhookSubscriptionRepository, WebhookSubscriptionStatus,
};

fn db_err(e: sqlx::Error) -> WebhookOutboundError {
    WebhookOutboundError::Database(e.to_string())
}

fn parse_status(raw: &str) -> Result<WebhookSubscriptionStatus, WebhookOutboundError> {
    match raw {
        "active" => Ok(WebhookSubscriptionStatus::Active),
        "paused" => Ok(WebhookSubscriptionStatus::Paused),
        "disabled" => Ok(WebhookSubscriptionStatus::Disabled),
        other => Err(WebhookOutboundError::Database(format!(
            "unknown webhook_subscription status: {other}"
        ))),
    }
}

fn parse_categories(raw: Vec<String>) -> Result<Vec<OutboundEventCategory>, WebhookOutboundError> {
    raw.into_iter()
        .map(|value| {
            OutboundEventCategory::try_from_str(&value).ok_or_else(|| {
                WebhookOutboundError::Database(format!("unknown event category: {value}"))
            })
        })
        .collect()
}

fn categories_to_strings(categories: &[OutboundEventCategory]) -> Vec<String> {
    categories.iter().map(|c| c.as_str().to_string()).collect()
}

fn row_to_subscription(
    row: &sqlx::postgres::PgRow,
) -> Result<WebhookSubscription, WebhookOutboundError> {
    let status_raw: String = row.try_get("status").map_err(db_err)?;
    let categories_raw: Vec<String> = row.try_get("event_categories").map_err(db_err)?;
    Ok(WebhookSubscription {
        id: row.try_get("id").map_err(db_err)?,
        realm_id: row.try_get("realm_id").map_err(db_err)?,
        endpoint_url: row.try_get("endpoint_url").map_err(db_err)?,
        secret: row.try_get("secret").map_err(db_err)?,
        event_categories: parse_categories(categories_raw)?,
        status: parse_status(&status_raw)?,
        consecutive_failures: row.try_get("consecutive_failures").map_err(db_err)?,
        last_error: row.try_get("last_error").map_err(db_err)?,
        paused_at: row.try_get("paused_at").map_err(db_err)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// 23505 on `(realm_id, endpoint_url)` — the subscription table's only
/// unique key — means a duplicate callback registration.
fn is_unique_violation(e: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db) = e {
        return db.code().as_deref() == Some("23505");
    }
    false
}

pub struct PostgresWebhookSubscriptionRepository {
    pool: Arc<PgPool>,
}

impl PostgresWebhookSubscriptionRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

impl WebhookSubscriptionRepository for PostgresWebhookSubscriptionRepository {
    async fn find_active_by_category(
        &self,
        realm_id: &str,
        category: OutboundEventCategory,
    ) -> Result<Vec<WebhookSubscription>, WebhookOutboundError> {
        let rows = sqlx::query(
            r#"
            SELECT id, realm_id, endpoint_url, secret, event_categories, status,
                   consecutive_failures, last_error, paused_at, created_at, updated_at
            FROM webhook_subscription
            WHERE realm_id = $1 AND status = 'active' AND event_categories && $2
            ORDER BY created_at
            "#,
        )
        .bind(realm_id)
        .bind(vec![category.as_str()])
        .fetch_all(&*self.pool)
        .await
        .map_err(db_err)?;

        rows.iter().map(row_to_subscription).collect()
    }

    async fn insert(
        &self,
        subscription: WebhookSubscription,
    ) -> Result<WebhookSubscription, WebhookOutboundError> {
        let result = sqlx::query(
            r#"
            INSERT INTO webhook_subscription
                (id, realm_id, endpoint_url, secret, event_categories, status,
                 consecutive_failures, last_error, paused_at, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(subscription.id)
        .bind(&subscription.realm_id)
        .bind(&subscription.endpoint_url)
        .bind(&subscription.secret)
        .bind(categories_to_strings(&subscription.event_categories))
        .bind(subscription.status.as_str())
        .bind(subscription.consecutive_failures)
        .bind(&subscription.last_error)
        .bind(subscription.paused_at)
        .bind(subscription.created_at)
        .bind(subscription.updated_at)
        .execute(&*self.pool)
        .await;

        match result {
            Ok(_) => Ok(subscription),
            Err(e) if is_unique_violation(&e) => Err(WebhookOutboundError::DuplicateEndpointUrl),
            Err(e) => Err(db_err(e)),
        }
    }

    async fn find_by_id_in_realm(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> Result<Option<WebhookSubscription>, WebhookOutboundError> {
        let row = sqlx::query(
            r#"
            SELECT id, realm_id, endpoint_url, secret, event_categories, status,
                   consecutive_failures, last_error, paused_at, created_at, updated_at
            FROM webhook_subscription
            WHERE id = $1 AND realm_id = $2
            "#,
        )
        .bind(id)
        .bind(realm_id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(db_err)?;

        row.as_ref().map(row_to_subscription).transpose()
    }

    async fn update(
        &self,
        id: Uuid,
        realm_id: &str,
        endpoint_url: &str,
        event_categories: &[OutboundEventCategory],
        reset_failures: bool,
    ) -> Result<WebhookSubscription, WebhookOutboundError> {
        let result = sqlx::query(
            r#"
            UPDATE webhook_subscription
            SET endpoint_url = $3,
                event_categories = $4,
                consecutive_failures = CASE WHEN $5 THEN 0 ELSE consecutive_failures END,
                updated_at = NOW()
            WHERE id = $1 AND realm_id = $2
            "#,
        )
        .bind(id)
        .bind(realm_id)
        .bind(endpoint_url)
        .bind(categories_to_strings(event_categories))
        .bind(reset_failures)
        .execute(&*self.pool)
        .await
        .map_err(|e| {
            if is_unique_violation(&e) {
                WebhookOutboundError::DuplicateEndpointUrl
            } else {
                db_err(e)
            }
        })?;

        if result.rows_affected() == 0 {
            return Err(WebhookOutboundError::NotFound);
        }
        self.find_by_id_in_realm(realm_id, id)
            .await?
            .ok_or(WebhookOutboundError::NotFound)
    }

    async fn set_status(
        &self,
        id: Uuid,
        realm_id: &str,
        status: WebhookSubscriptionStatus,
        reset_failure_state: bool,
    ) -> Result<WebhookSubscription, WebhookOutboundError> {
        let result = sqlx::query(
            r#"
            UPDATE webhook_subscription
            SET status = $3,
                consecutive_failures = CASE WHEN $4 THEN 0 ELSE consecutive_failures END,
                paused_at = CASE WHEN $4 THEN NULL ELSE paused_at END,
                updated_at = NOW()
            WHERE id = $1 AND realm_id = $2
            "#,
        )
        .bind(id)
        .bind(realm_id)
        .bind(status.as_str())
        .bind(reset_failure_state)
        .execute(&*self.pool)
        .await
        .map_err(db_err)?;

        if result.rows_affected() == 0 {
            return Err(WebhookOutboundError::NotFound);
        }
        self.find_by_id_in_realm(realm_id, id)
            .await?
            .ok_or(WebhookOutboundError::NotFound)
    }

    async fn rotate_secret(
        &self,
        id: Uuid,
        realm_id: &str,
        new_secret: &str,
    ) -> Result<Option<WebhookSubscription>, WebhookOutboundError> {
        let result = sqlx::query(
            r#"
            UPDATE webhook_subscription
            SET secret = $3, updated_at = NOW()
            WHERE id = $1 AND realm_id = $2
            "#,
        )
        .bind(id)
        .bind(realm_id)
        .bind(new_secret)
        .execute(&*self.pool)
        .await
        .map_err(db_err)?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.find_by_id_in_realm(realm_id, id).await
    }

    async fn delete(&self, realm_id: &str, id: Uuid) -> Result<bool, WebhookOutboundError> {
        let result =
            sqlx::query("DELETE FROM webhook_subscription WHERE id = $1 AND realm_id = $2")
                .bind(id)
                .bind(realm_id)
                .execute(&*self.pool)
                .await
                .map_err(db_err)?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_page(
        &self,
        realm_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<WebhookSubscription>, i64), WebhookOutboundError> {
        let rows = sqlx::query(
            r#"
            SELECT id, realm_id, endpoint_url, secret, event_categories, status,
                   consecutive_failures, last_error, paused_at, created_at, updated_at
            FROM webhook_subscription
            WHERE realm_id = $1
            ORDER BY created_at
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(realm_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&*self.pool)
        .await
        .map_err(db_err)?;

        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM webhook_subscription WHERE realm_id = $1")
                .bind(realm_id)
                .fetch_one(&*self.pool)
                .await
                .map_err(db_err)?;

        let subscriptions = rows
            .iter()
            .map(row_to_subscription)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((subscriptions, total))
    }

    async fn count_by_realm(&self, realm_id: &str) -> Result<i64, WebhookOutboundError> {
        sqlx::query_scalar("SELECT COUNT(*) FROM webhook_subscription WHERE realm_id = $1")
            .bind(realm_id)
            .fetch_one(&*self.pool)
            .await
            .map_err(db_err)
    }
}

pub struct PostgresOutboundEventRepository {
    pool: Arc<PgPool>,
}

impl PostgresOutboundEventRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

impl OutboundEventRepository for PostgresOutboundEventRepository {
    /// One transaction: the event row plus every delivery row, so a partial
    /// fan-out snapshot can never exist.
    async fn insert_event_with_deliveries(
        &self,
        event_id: Uuid,
        realm_id: &str,
        event_type: OutboundEventType,
        payload: serde_json::Value,
        occurred_at: DateTime<Utc>,
        subscription_ids: &[Uuid],
    ) -> Result<(), WebhookOutboundError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| WebhookOutboundError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO outbound_event (id, realm_id, event_type, payload, occurred_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(event_id)
        .bind(realm_id)
        .bind(event_type.as_str())
        .bind(payload)
        .bind(occurred_at)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;

        if !subscription_ids.is_empty() {
            sqlx::query(
                r#"
                INSERT INTO outbound_delivery (id, event_id, subscription_id, realm_id, status)
                SELECT uuidv7(), $1, s.id, $2, 'pending'
                FROM unnest($3::uuid[]) AS s(id)
                "#,
            )
            .bind(event_id)
            .bind(realm_id)
            .bind(subscription_ids.to_vec())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }

        tx.commit()
            .await
            .map_err(|e| WebhookOutboundError::Database(e.to_string()))
    }

    async fn role_names_by_ids(
        &self,
        role_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, String>, WebhookOutboundError> {
        if role_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(Uuid, String)> =
            sqlx::query_as("SELECT id, name FROM roles WHERE id = ANY($1)")
                .bind(role_ids.to_vec())
                .fetch_all(&*self.pool)
                .await
                .map_err(db_err)?;

        Ok(rows.into_iter().collect())
    }
}
