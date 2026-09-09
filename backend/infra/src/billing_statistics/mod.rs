use std::collections::BTreeMap;

use chrono::NaiveDate;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use herald_domain::billing_statistics::{
    BillingStatisticsRepository, BucketConsumption, ConsumptionTrendPoint, CurrencyAmount,
    PaymentStats, PaymentTrendPoint, PointsConsumptionStats, ProviderPaymentStats, StatsWindow,
};
use herald_domain::common::entities::app_errors::CoreError;

/// Half-open window filter shared by every statistics query: `[start, today+1)`
/// in server-local calendar days, so the current day is fully included. The
/// alias qualifies `created_at` for the bucket-grouping query, whose join of
/// `points_transactions pt` with `credit_buckets cb` is ambiguous (both carry
/// `created_at`); pass `""` for single-table queries.
///
/// The trend queries build their day axis with `generate_series` over this
/// same `CURRENT_DATE` calendar, so axis and filter share one source of truth.
/// Recomputing the axis in Rust from `Utc::now()` would misalign by one day on
/// a non-UTC session timezone and silently drop the out-of-axis trend rows.
fn window_filter(alias: &str) -> String {
    format!(
        "{alias}created_at >= CURRENT_DATE - MAKE_INTERVAL(days => $2::int - 1) AND \
         {alias}created_at < CURRENT_DATE + INTERVAL '1 day'"
    )
}

pub struct PostgresBillingStatisticsRepository {
    pool: PgPool,
}

impl PostgresBillingStatisticsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl BillingStatisticsRepository for PostgresBillingStatisticsRepository {
    #[allow(clippy::manual_async_fn)]
    fn get_payment_stats(
        &self,
        realm_id: &str,
        window: StatsWindow,
    ) -> impl std::future::Future<Output = Result<PaymentStats, CoreError>> + Send {
        let realm_id = realm_id.to_string();
        async move { self.fetch_payment_stats(&realm_id, window).await }
    }

    #[allow(clippy::manual_async_fn)]
    fn get_points_consumption_stats(
        &self,
        realm_id: &str,
        window: StatsWindow,
    ) -> impl std::future::Future<Output = Result<PointsConsumptionStats, CoreError>> + Send {
        let realm_id = realm_id.to_string();
        async move { self.fetch_points_consumption_stats(&realm_id, window).await }
    }
}

impl PostgresBillingStatisticsRepository {
    async fn fetch_payment_stats(
        &self,
        realm_id: &str,
        window: StatsWindow,
    ) -> Result<PaymentStats, CoreError> {
        // The provider × currency rows partition the same finalized-attempt
        // set the response needs everywhere, so grand totals and per-currency
        // amounts are derived from them in one fold instead of extra queries.
        // Only finalized attempts enter the statistics: pending or otherwise
        // non-terminal statuses must not touch any numerator or denominator.
        #[derive(Debug, FromRow)]
        struct TrendRow {
            date: NaiveDate,
            succeeded_count: i64,
            failed_count: i64,
        }

        let filter = window_filter("");
        // The two queries are independent; run them concurrently instead of
        // paying their roundtrips back to back.
        let (provider_rows, trend_rows) = tokio::try_join!(
            async {
                sqlx::query_as::<_, ProviderCurrencyRow>(&format!(
                    r#"
                    SELECT payment_provider, currency,
                           COUNT(*) FILTER (WHERE status = 'Succeeded')::BIGINT AS succeeded_count,
                           COUNT(*) FILTER (WHERE status = 'Failed')::BIGINT     AS failed_count,
                           COALESCE(SUM(amount) FILTER (WHERE status = 'Succeeded'), 0)::BIGINT AS succeeded_amount
                    FROM payment_attempts
                    WHERE realm_id = $1 AND status IN ('Succeeded', 'Failed') AND {filter}
                    GROUP BY payment_provider, currency
                    ORDER BY payment_provider, currency
                    "#
                ))
                .bind(realm_id)
                .bind(window.days())
                .fetch_all(&self.pool)
                .await
                .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))
            },
            async {
                // The generate_series axis IS the zero-fill: one row per
                // calendar day of the window, ascending, with unmatched days
                // counting zero — covering exactly the range `window_filter`
                // selects.
                sqlx::query_as::<_, TrendRow>(
                    r#"
                    SELECT gs::date AS date,
                           COUNT(a.id) FILTER (WHERE a.status = 'Succeeded')::BIGINT AS succeeded_count,
                           COUNT(a.id) FILTER (WHERE a.status = 'Failed')::BIGINT     AS failed_count
                    FROM generate_series(
                           CURRENT_DATE - MAKE_INTERVAL(days => $2::int - 1),
                           CURRENT_DATE,
                           INTERVAL '1 day') AS gs
                    LEFT JOIN payment_attempts a
                           ON a.realm_id = $1 AND a.status IN ('Succeeded', 'Failed')
                          AND a.created_at >= gs AND a.created_at < gs + INTERVAL '1 day'
                    GROUP BY gs
                    ORDER BY gs
                    "#,
                )
                .bind(realm_id)
                .bind(window.days())
                .fetch_all(&self.pool)
                .await
                .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))
            }
        )?;

        let rollup = roll_up_provider_rows(provider_rows);

        Ok(PaymentStats {
            window_days: window.days(),
            succeeded_count: rollup.succeeded_count,
            failed_count: rollup.failed_count,
            amounts_by_currency: rollup.amounts_by_currency,
            providers: rollup.providers,
            payment_trend: trend_rows
                .into_iter()
                .map(|r| PaymentTrendPoint {
                    date: r.date.to_string(),
                    succeeded_count: r.succeeded_count,
                    failed_count: r.failed_count,
                })
                .collect(),
        })
    }

    async fn fetch_points_consumption_stats(
        &self,
        realm_id: &str,
        window: StatsWindow,
    ) -> Result<PointsConsumptionStats, CoreError> {
        // Consumption is `type = 'consume'` only; revocation types are not in
        // the filter set, so reclaimed points never offset these figures.
        #[derive(Debug, FromRow)]
        struct TotalsRow {
            total_consumed: i64,
            consuming_users: i64,
        }

        #[derive(Debug, FromRow)]
        struct BucketRow {
            bucket_id: Uuid,
            bucket_key: String,
            bucket_name: String,
            consumed_points: i64,
        }

        #[derive(Debug, FromRow)]
        struct TrendRow {
            date: NaiveDate,
            consumed_points: i64,
        }

        let filter = window_filter("");
        let filter_pt = window_filter("pt.");
        // The three queries are independent; run them concurrently instead of
        // paying their roundtrips back to back.
        let (totals, bucket_rows, trend_rows) = tokio::try_join!(
            async {
                sqlx::query_as::<_, TotalsRow>(&format!(
                    r#"
                    SELECT COALESCE(SUM(ABS(amount)), 0)::BIGINT AS total_consumed,
                           COUNT(DISTINCT user_id)::BIGINT        AS consuming_users
                    FROM points_transactions
                    WHERE realm_id = $1 AND type = 'consume' AND {filter}
                    "#
                ))
                .bind(realm_id)
                .bind(window.days())
                .fetch_one(&self.pool)
                .await
                .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))
            },
            async {
                sqlx::query_as::<_, BucketRow>(&format!(
                    r#"
                    SELECT pt.bucket_id, cb.bucket_key, cb.name AS bucket_name,
                           SUM(ABS(pt.amount))::BIGINT AS consumed_points
                    FROM points_transactions pt
                    JOIN credit_buckets cb ON cb.id = pt.bucket_id
                    WHERE pt.realm_id = $1 AND pt.type = 'consume'
                      AND {filter_pt}
                    GROUP BY pt.bucket_id, cb.bucket_key, cb.name
                    ORDER BY consumed_points DESC, pt.bucket_id
                    "#
                ))
                .bind(realm_id)
                .bind(window.days())
                .fetch_all(&self.pool)
                .await
                .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))
            },
            async {
                // Same generate_series axis as the payment trend: the DB
                // returns the full zero-filled day axis over exactly the
                // range `window_filter` selects.
                sqlx::query_as::<_, TrendRow>(
                    r#"
                    SELECT gs::date AS date, COALESCE(SUM(ABS(t.amount)), 0)::BIGINT AS consumed_points
                    FROM generate_series(
                           CURRENT_DATE - MAKE_INTERVAL(days => $2::int - 1),
                           CURRENT_DATE,
                           INTERVAL '1 day') AS gs
                    LEFT JOIN points_transactions t
                           ON t.realm_id = $1 AND t.type = 'consume'
                          AND t.created_at >= gs AND t.created_at < gs + INTERVAL '1 day'
                    GROUP BY gs
                    ORDER BY gs
                    "#,
                )
                .bind(realm_id)
                .bind(window.days())
                .fetch_all(&self.pool)
                .await
                .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))
            }
        )?;

        Ok(PointsConsumptionStats {
            window_days: window.days(),
            total_consumed_points: totals.total_consumed,
            consuming_users: totals.consuming_users,
            buckets: bucket_rows
                .into_iter()
                .map(|r| BucketConsumption {
                    bucket_id: r.bucket_id,
                    bucket_key: r.bucket_key,
                    bucket_name: r.bucket_name,
                    consumed_points: r.consumed_points,
                })
                .collect(),
            consumption_trend: trend_rows
                .into_iter()
                .map(|r| ConsumptionTrendPoint {
                    date: r.date.to_string(),
                    consumed_points: r.consumed_points,
                })
                .collect(),
        })
    }
}

/// Everything the payment stats response derives from the provider × currency
/// rows, folded in a single pass.
struct PaymentRollup {
    succeeded_count: i64,
    failed_count: i64,
    amounts_by_currency: Vec<CurrencyAmount>,
    providers: Vec<ProviderPaymentStats>,
}

/// Assemble provider × currency rows (ordered by provider, currency) into the
/// nested provider → amounts-by-currency response shape, plus the grand
/// totals and the realm-wide per-currency amounts. The rows partition the
/// finalized attempt set, so the grand counts are the row-count sums; a
/// currency enters `amounts_by_currency` only when it saw at least one
/// succeeded attempt (amounts are never summed across currencies anywhere in
/// this contract, and a failed-only currency has no amount to report).
fn roll_up_provider_rows(rows: Vec<ProviderCurrencyRow>) -> PaymentRollup {
    let mut succeeded_count = 0;
    let mut failed_count = 0;
    // (succeeded attempts, succeeded amount) per currency; BTreeMap keeps the
    // currency ordering stable.
    let mut by_currency: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut providers: Vec<ProviderPaymentStats> = Vec::new();
    for row in rows {
        succeeded_count += row.succeeded_count;
        failed_count += row.failed_count;
        let currency = by_currency.entry(row.currency.clone()).or_insert((0, 0));
        currency.0 += row.succeeded_count;
        currency.1 += row.succeeded_amount;
        match providers
            .iter_mut()
            .find(|p| p.payment_provider == row.payment_provider)
        {
            Some(provider) => {
                provider.succeeded_count += row.succeeded_count;
                provider.failed_count += row.failed_count;
                provider.amounts_by_currency.push(CurrencyAmount {
                    currency: row.currency,
                    amount: row.succeeded_amount,
                });
            }
            None => providers.push(ProviderPaymentStats {
                payment_provider: row.payment_provider.clone(),
                succeeded_count: row.succeeded_count,
                failed_count: row.failed_count,
                amounts_by_currency: vec![CurrencyAmount {
                    currency: row.currency,
                    amount: row.succeeded_amount,
                }],
            }),
        }
    }
    PaymentRollup {
        succeeded_count,
        failed_count,
        amounts_by_currency: by_currency
            .into_iter()
            .filter(|(_, (succeeded, _))| *succeeded > 0)
            .map(|(currency, (_, amount))| CurrencyAmount { currency, amount })
            .collect(),
        providers,
    }
}

#[derive(Debug, FromRow)]
struct ProviderCurrencyRow {
    payment_provider: String,
    currency: String,
    succeeded_count: i64,
    failed_count: i64,
    succeeded_amount: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollup_groups_providers_and_derives_totals_and_currencies() {
        // EUR carries only failed attempts: it must stay out of the realm-wide
        // amounts (no succeeded amount exists to report) while its failed
        // count still reaches the grand totals through the wechat row.
        let rows = vec![
            ProviderCurrencyRow {
                payment_provider: "stripe".into(),
                currency: "USD".into(),
                succeeded_count: 9,
                failed_count: 1,
                succeeded_amount: 900,
            },
            ProviderCurrencyRow {
                payment_provider: "stripe".into(),
                currency: "EUR".into(),
                succeeded_count: 0,
                failed_count: 3,
                succeeded_amount: 0,
            },
            ProviderCurrencyRow {
                payment_provider: "wechat".into(),
                currency: "CNY".into(),
                succeeded_count: 2,
                failed_count: 2,
                succeeded_amount: 40,
            },
        ];
        let rollup = roll_up_provider_rows(rows);
        assert_eq!(rollup.succeeded_count, 11);
        assert_eq!(rollup.failed_count, 6);
        // Currency-sorted, and EUR excluded as failed-only.
        assert_eq!(rollup.amounts_by_currency.len(), 2);
        assert_eq!(rollup.amounts_by_currency[0].currency, "CNY");
        assert_eq!(rollup.amounts_by_currency[0].amount, 40);
        assert_eq!(rollup.amounts_by_currency[1].currency, "USD");
        assert_eq!(rollup.amounts_by_currency[1].amount, 900);

        assert_eq!(rollup.providers.len(), 2);
        assert_eq!(rollup.providers[0].payment_provider, "stripe");
        assert_eq!(rollup.providers[0].succeeded_count, 9);
        assert_eq!(rollup.providers[0].failed_count, 4);
        assert_eq!(rollup.providers[0].amounts_by_currency.len(), 2);
        assert_eq!(rollup.providers[0].amounts_by_currency[0].amount, 900);
        assert_eq!(rollup.providers[0].amounts_by_currency[1].amount, 0);
        assert_eq!(rollup.providers[1].payment_provider, "wechat");
        assert_eq!(rollup.providers[1].amounts_by_currency[0].currency, "CNY");
    }
}
