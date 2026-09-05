use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};
use sqlx::{FromRow, PgPool};

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::dashboard::{AuthTrendPoint, DashboardRepository, DashboardStats, UserStats};
use herald_entity::account;

pub struct PostgresDashboardRepository {
    db: Arc<DatabaseConnection>,
    pool: PgPool,
}

impl PostgresDashboardRepository {
    pub fn new(db: Arc<DatabaseConnection>, pool: PgPool) -> Self {
        Self { db, pool }
    }
}

impl DashboardRepository for PostgresDashboardRepository {
    #[allow(clippy::manual_async_fn)]
    fn get_stats(
        &self,
        realm_id: &str,
    ) -> impl std::future::Future<Output = Result<DashboardStats, CoreError>> + Send {
        async move {
            let total_users = account::Entity::find()
                .filter(account::Column::RealmId.eq(realm_id))
                .count(&*self.db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            let seven_days_ago = Utc::now() - Duration::days(7);
            let new_users = account::Entity::find()
                .filter(account::Column::RealmId.eq(realm_id))
                .filter(
                    account::Column::CreatedAt
                        .gte(sea_orm::prelude::DateTimeWithTimeZone::from(seven_days_ago)),
                )
                .count(&*self.db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            let active_users: i64 = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COUNT(DISTINCT actor_id)
                FROM audit_events
                WHERE realm_id = $1
                  AND action = 'auth.login'
                  AND created_at >= NOW() - INTERVAL '7 days'
                "#,
            )
            .bind(realm_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?;

            let auth_trend = self.fetch_auth_trend(realm_id).await?;

            Ok(DashboardStats {
                user_stats: UserStats {
                    total_users: total_users as i64,
                    new_users: new_users as i64,
                    active_users,
                },
                auth_trend,
            })
        }
    }
}

impl PostgresDashboardRepository {
    async fn fetch_auth_trend(&self, realm_id: &str) -> Result<Vec<AuthTrendPoint>, CoreError> {
        #[derive(Debug, FromRow)]
        struct TrendRow {
            date: NaiveDate,
            success_count: i64,
            failure_count: i64,
        }

        let rows: Vec<TrendRow> = sqlx::query_as::<_, TrendRow>(
            r#"
            SELECT
                DATE(created_at) AS date,
                COUNT(*) FILTER (WHERE action = 'auth.login') AS success_count,
                COUNT(*) FILTER (WHERE action = 'auth.login_failed') AS failure_count
            FROM audit_events
            WHERE realm_id = $1
              AND action IN ('auth.login', 'auth.login_failed')
              AND created_at >= CURRENT_DATE - INTERVAL '29 days'
              AND created_at < CURRENT_DATE + INTERVAL '1 day'
            GROUP BY DATE(created_at)
            ORDER BY date
            "#,
        )
        .bind(realm_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?;

        // Build map from date to counts, then fill missing dates with zero.
        let mut map: BTreeMap<NaiveDate, (i64, i64)> = rows
            .into_iter()
            .map(|r| (r.date, (r.success_count, r.failure_count)))
            .collect();

        let today = Utc::now().date_naive();
        let start_date = today - Duration::days(29);

        let mut trend = Vec::with_capacity(30);
        let mut d = start_date;
        while d <= today {
            let (success_count, failure_count) = map.remove(&d).unwrap_or((0, 0));
            trend.push(AuthTrendPoint {
                date: d.to_string(),
                success_count,
                failure_count,
            });
            d += Duration::days(1);
        }

        Ok(trend)
    }
}
