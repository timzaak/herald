use std::sync::Arc;
use uuid::Uuid;

use crate::authentication::Identity;
use crate::common::entities::app_errors::CoreError;
use crate::common::policies::ensure_policy;
use crate::points::{
    DistributionEvent, DistributionRuleOwner, DistributionRuleSelection, DistributionTrigger,
    dtos::{ConsumePointsInput, GrantPointsInput, GrantPointsOutput, RevokePointsOutput},
    entities::{
        ConsumptionAllocationView, CreditSourceType, CreditType, Paginated, PointsBalance,
        PointsQuotaEntitlement, PointsTransaction, PointsWallet, QuotaWindowView, RechargeType,
        RevocationType, WalletStatus,
    },
    errors::PointsErrorExt,
    event_key_for_free_periodic,
    policies::PointsPolicy,
    ports::{PointsRepository, TransactionFilters, WalletFilters},
};

/// Points Service - Business logic for points management
/// Includes permission-based authorization checks using PointsPolicy
pub struct PointsService<R, P>
where
    R: PointsRepository,
    P: PointsPolicy,
{
    repository: Arc<R>,
    policy: Arc<P>,
}

impl<R, P> PointsService<R, P>
where
    R: PointsRepository + Send + Sync,
    P: PointsPolicy,
{
    pub fn new(repository: Arc<R>, policy: Arc<P>) -> Self {
        Self { repository, policy }
    }

    /// Require realm-wide points management permission.
    pub async fn ensure_can_manage_points(&self, identity: Identity) -> Result<(), CoreError> {
        ensure_policy(
            self.policy.can_manage_points(identity).await,
            "Insufficient permissions to manage points",
        )
    }

    /// Get points wallet for a user
    pub async fn get_wallet(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
    ) -> Result<PointsWallet, CoreError> {
        // Check view permissions
        ensure_policy(
            self.policy
                .can_view_points(identity.clone(), Some(user_id))
                .await,
            "Insufficient permissions to view points wallet",
        )?;

        // Check realm boundary
        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot access points from a different realm".to_string(),
            ));
        }

        // Credit Buckets model: a wallet is per-(user,
        // bucket); a "get wallet for a user" is a user-total view (sum across
        // the user's per-bucket wallet rows; 0 if none). We must NOT auto-
        // create a `bucket_id = None` wallet row here — `points_wallets.
        // bucket_id` is NOT NULL, and wallets are created lazily only when a
        // grant/consume targets a specific bucket. So when no wallet row
        // exists we return a synthesized zero-balance view.
        match self.repository.find_by_user_id(realm_id, user_id).await? {
            Some(account) => Ok(account),
            None => {
                tracing::info!(
                    "No points wallet for user {}; returning zero-balance user-total view",
                    user_id
                );
                Ok(Self::synthesized_empty_wallet(realm_id, user_id))
            }
        }
    }

    /// Build a zero-balance user-total wallet view (`bucket_id = None`) for a
    /// user who has no wallet row yet. Mirrors the aggregate shape returned by
    /// `find_by_user_id` for an empty user.
    fn synthesized_empty_wallet(realm_id: &str, user_id: Uuid) -> PointsWallet {
        let now = chrono::Utc::now();
        PointsWallet {
            id: Uuid::nil(),
            user_id,
            realm_id: realm_id.to_string(),
            bucket_id: None,
            total_topup_granted: 0,
            total_subscription_granted: 0,
            total_recharged: 0,
            total_consumed: 0,
            status: WalletStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    /// Get points balance for a user
    /// Switched to derived SUM: the 5 typed balances and
    /// `total_balance` are projected from `compute_available_balance` (same
    /// predicate as consumption — "seen balance == spendable balance"), so
    /// future-effective pre-grant rows never leak into the user-visible
    /// balance. `analytics` (`total_recharged` / `total_consumed`) still come
    /// from the wallet Stored columns (lifetime totals, unaffected by
    /// `effective_at`). The active-entitlement
    /// realization (`reconcile_due_for_user`) runs FIRST and writes already
    /// due free-periodic schedule grants. Failure is fail-loud: callers must
    /// see the write error instead of stale balance or `InsufficientBalance`.
    /// Window-quota availability for `subscription_credit` / `free_periodic_credit`
    /// is folded into the typed balances on top of the pool-side derived SUM,
    /// so the returned balance reflects both ledger (pool) and window-model
    /// entitlements.
    pub async fn get_balance(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
    ) -> Result<PointsBalance, CoreError> {
        // Permission + realm boundary checks (same gate as before).
        ensure_policy(
            self.policy
                .can_view_points(identity.clone(), Some(user_id))
                .await,
            "Insufficient permissions to view points balance",
        )?;
        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot access points from a different realm".to_string(),
            ));
        }

        let now = chrono::Utc::now();

        // Read-path realization. Independent committed short
        // transaction; the derived SUM below runs in a separate transaction
        // and under the project default READ COMMITTED sees the committed
        // realization rows. Failure is fail-loud (5xx) — never silently
        // degrade to an old balance or `InsufficientBalance`.
        self.reconcile_due_for_user(realm_id, user_id, now).await?;

        // Analytics still from Stored wallet columns (lifetime totals).
        let account = self.get_wallet(identity, realm_id, user_id).await?;

        // Derived SUM by credit_type (same predicate as consumption).
        let derived = self
            .repository
            .compute_available_balance(realm_id, user_id, &[], now)
            .await?;

        // Window-quota availability for the window-model credit types
        // (subscription + free-periodic). Folded into the balance so the
        // user-visible total matches what `consume_points_atomic` can spend.
        let window_balances = self
            .compute_window_balance_by_credit_type(realm_id, user_id, now)
            .await?;

        Ok(Self::build_balance_from_derived(
            account,
            derived,
            window_balances,
        ))
    }

    /// Client-app scoped variant of [`get_balance`]: every spendable figure
    /// (typed balances, total) is restricted to the Credit Buckets the client
    /// app explicitly covers — the same coverage set `consume_points_atomic`
    /// spends from. Used by the external API so a client-app-bound API key
    /// sees only the points of its own app, not the user's realm-wide total.
    /// Wallet analytics (`total_recharged` / `total_consumed`) remain the
    /// Stored wallet lifetime totals.
    pub async fn get_balance_for_client_app(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
        client_app_id: Uuid,
    ) -> Result<PointsBalance, CoreError> {
        // Permission + realm boundary checks (same gate as get_balance).
        ensure_policy(
            self.policy
                .can_view_points(identity.clone(), Some(user_id))
                .await,
            "Insufficient permissions to view points balance",
        )?;
        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot access points from a different realm".to_string(),
            ));
        }

        let now = chrono::Utc::now();

        // Read-path realization, same as get_balance (fail-loud).
        self.reconcile_due_for_user(realm_id, user_id, now).await?;
        let account = self.get_wallet(identity, realm_id, user_id).await?;

        let covered = self
            .repository
            .find_covered_bucket_ids(realm_id, client_app_id)
            .await?;

        // An app that covers no buckets must yield a zero balance — NOT fall
        // back to the unfiltered view (an empty bucket slice means "all
        // buckets" in compute_available_balance).
        let (derived, window_balances) = if covered.is_empty() {
            (Vec::new(), Default::default())
        } else {
            let derived = self
                .repository
                .compute_available_balance(realm_id, user_id, &covered, now)
                .await?;
            let covered_set: std::collections::HashSet<Uuid> = covered.iter().copied().collect();
            let window_balances = self
                .compute_window_balance_for_buckets(realm_id, user_id, Some(&covered_set), now)
                .await?;
            (derived, window_balances)
        };

        Ok(Self::build_balance_from_derived(
            account,
            derived,
            window_balances,
        ))
    }

    /// Compute per-credit-type window-quota availability across all buckets
    /// for a user. Returns a map with entries for `SubscriptionCredit` and
    /// `FreePeriodicCredit` when active quota entitlements exist; missing
    /// entries mean zero window availability for that credit type.
    async fn compute_window_balance_by_credit_type(
        &self,
        realm_id: &str,
        user_id: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<std::collections::HashMap<CreditType, i64>, CoreError> {
        self.compute_window_balance_for_buckets(realm_id, user_id, None, now)
            .await
    }

    /// Bucket-filtered variant of [`compute_window_balance_by_credit_type`]:
    /// `allowed_buckets = Some(set)` restricts the aggregation to entitlements
    /// in those buckets (client-app scoped views); `None` aggregates all.
    async fn compute_window_balance_for_buckets(
        &self,
        realm_id: &str,
        user_id: Uuid,
        allowed_buckets: Option<&std::collections::HashSet<Uuid>>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<std::collections::HashMap<CreditType, i64>, CoreError> {
        use std::collections::{HashMap, HashSet};

        let mut balances = HashMap::new();
        for credit_type in [
            CreditType::SubscriptionCredit,
            CreditType::FreePeriodicCredit,
        ] {
            let entitlements = self
                .repository
                .find_active_quota_entitlements(realm_id, user_id, None, credit_type, now)
                .await?;
            if entitlements.is_empty() {
                continue;
            }
            let bucket_ids: HashSet<Uuid> = entitlements
                .iter()
                .map(|e| e.bucket_id)
                .filter(|bucket_id| {
                    allowed_buckets.is_none_or(|allowed| allowed.contains(bucket_id))
                })
                .collect();
            if bucket_ids.is_empty() {
                continue;
            }
            let mut total = 0i64;
            for bucket_id in bucket_ids {
                total += self
                    .compute_window_available_for_credit_type(
                        realm_id,
                        user_id,
                        bucket_id,
                        credit_type,
                        now,
                    )
                    .await?;
            }
            balances.insert(credit_type, total);
        }
        Ok(balances)
    }

    /// Build `PointsBalance` from derived SUM + window-quota availability +
    /// Stored analytics. The 5 typed balances and `total_balance` come from
    /// the derived SUM keyed by `CreditType` plus the window-model
    /// contribution; analytics (`total_recharged` / `total_consumed`) are
    /// passed through from the Stored wallet (analytics remain Stored).
    fn build_balance_from_derived(
        account: PointsWallet,
        derived: Vec<(CreditType, i64)>,
        window_balances: std::collections::HashMap<CreditType, i64>,
    ) -> PointsBalance {
        let mut topup = 0i64;
        let mut subscription = 0i64;
        let mut granted = 0i64;
        let mut registration = 0i64;
        let mut free_periodic = 0i64;
        for (credit_type, amount) in derived {
            match credit_type {
                CreditType::TopupCredit => topup += amount,
                CreditType::SubscriptionCredit => subscription += amount,
                CreditType::GrantedCredit => granted += amount,
                CreditType::RegistrationCredit => registration += amount,
                CreditType::FreePeriodicCredit => free_periodic += amount,
            }
        }
        // Fold in window-quota availability for the window-model credit types.
        // Under the new model the pool side for these types is zero, so this
        // is additive; it also safely coexists with any legacy ledger rows.
        subscription += window_balances
            .get(&CreditType::SubscriptionCredit)
            .copied()
            .unwrap_or(0);
        free_periodic += window_balances
            .get(&CreditType::FreePeriodicCredit)
            .copied()
            .unwrap_or(0);
        let total_balance = topup + subscription + granted + registration + free_periodic;
        PointsBalance {
            user_id: account.user_id,
            balance: total_balance,
            topup_balance: topup,
            subscription_balance: subscription,
            granted_balance: granted,
            registration_balance: registration,
            free_periodic_balance: free_periodic,
            total_recharged: account.total_recharged,
            total_consumed: account.total_consumed,
            unit: "points".to_string(),
            updated_at: account.updated_at,
        }
    }

    /// Read-path realization for already-due free-periodic schedules.
    /// Subscription schedules are intentionally excluded: paid-state must come
    /// from provider webhooks, not from request-time guessing. Each call is
    /// bounded to a small batch so the request path cannot perform unbounded
    /// catch-up work.
    /// Fail-loud: any scan/write error is surfaced verbatim. Callers must not
    /// degrade a realization fault into stale balance or `InsufficientBalance`.
    pub async fn reconcile_due_for_user(
        &self,
        realm_id: &str,
        user_id: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), CoreError> {
        const READ_PATH_REALIZATION_LIMIT: u64 = 3;

        let schedules = self
            .repository
            .find_due_free_grant_schedules_for_user(
                realm_id,
                user_id,
                now,
                READ_PATH_REALIZATION_LIMIT,
            )
            .await?;

        for schedule in schedules {
            if schedule.should_stop() {
                continue;
            }

            let period_number = (schedule.granted_periods + 1).try_into().map_err(|_| {
                CoreError::InternalServerError(format!(
                    "invalid grant period for schedule {}: {}",
                    schedule.id,
                    schedule.granted_periods + 1
                ))
            })?;
            let event_key = event_key_for_free_periodic(
                schedule.user_id,
                schedule.distribution_rule_id,
                period_number,
            );
            self.repository
                .execute_distribution_event_atomic(
                    DistributionEvent {
                        realm_id: realm_id.to_string(),
                        user_id,
                        owner: DistributionRuleOwner::RealmRegistration,
                        trigger: DistributionTrigger::FreePeriodicGrant,
                        event_key: event_key.clone(),
                        source_id: event_key,
                        effective_from: schedule.next_grant_time,
                        effective_until: None,
                    },
                    DistributionRuleSelection::ScheduledRule(schedule.distribution_rule_id),
                )
                .await?;
        }

        Ok(())
    }

    /// List wallets in a realm. `points.manage` holders see all users' wallets;
    /// `points.view`-only callers are hard-scoped to their own (mirrors
    /// `list_transactions`).
    pub async fn list_wallets(
        &self,
        identity: Identity,
        realm_id: &str,
        filters: WalletFilters,
    ) -> Result<Paginated<PointsWallet>, CoreError> {
        // View gate (handler also checks points.view; this is the domain
        // defense-in-depth). can_view_points(_, None) is true for a points.view-only
        // user viewing their own data and for points.manage holders.
        ensure_policy(
            self.policy.can_view_points(identity.clone(), None).await,
            "Insufficient permissions to list wallets",
        )?;

        // Check realm boundary
        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot access points from a different realm".to_string(),
            ));
        }

        // Only points managers list across users; points.view alone is scoped to self.
        // HARD-SCOPE non-managers to their own wallets:
        // - `user_id` is server-injected (NOT a query param in ListWalletsQuery),
        // so the client cannot set or override it.
        // - `search` is the only client field that can target another user; we drop
        // it so the ONLY user binding on the query is `user_id = caller`.
        // Remaining filters (bucket_id/status/paging) only narrow within the caller's
        // own rows. Mirrors list_transactions below, with explicit search stripping.
        let can_view_all = self.policy.can_manage_points(identity.clone()).await;
        if !can_view_all && let Ok(current_user_id) = identity.user_id().parse::<Uuid>() {
            let mut restricted = filters;
            restricted.user_id = Some(current_user_id);
            restricted.search = None;
            return self.repository.list_wallets(realm_id, restricted).await;
        }

        self.repository.list_wallets(realm_id, filters).await
    }

    /// Consume points from a user's account using ledger-based consumption.
    /// Domain coordination entry: permission /
    /// input / realm-boundary validation only, then delegates to
    /// `repository.consume_points_atomic`. The window-first + overflow-to-pool
    /// single-transaction mix happens INSIDE the infra atomic path,
    /// which calls the pure `plan_mixed_consume` to split `window_part` /
    /// `pool_part`. The consume request/response contract is unchanged.
    /// Consumption priority (pool side): expiration-based (soonest expiring
    /// first, permanent last).
    #[tracing::instrument(
        // Governance: identity carries user_id/realm_id;
        // input payload references user_id/client_app_id; realm_id is
        // conservatively skipped. Only the low-cardinality operation type and
        // db.system are recorded (no raw SQL, no ids).
        skip(self, identity, realm_id, input),
        fields(db.system = "postgres", db.operation = "consume_points")
    )]
    pub async fn consume_points(
        &self,
        identity: Identity,
        realm_id: &str,
        input: ConsumePointsInput,
    ) -> Result<Vec<PointsTransaction>, CoreError> {
        // Check consume permissions
        ensure_policy(
            self.policy.can_consume_points(identity.clone()).await,
            "Insufficient permissions to consume points",
        )?;

        // Validate input
        let (user_id, client_app_id, amount, description) = input.try_into()?;

        // Check realm boundary
        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot consume points from a different realm".to_string(),
            ));
        }

        // NOTE: The legacy single-wallet precheck (find_by_user_id → status +
        // balance check) is intentionally removed. With the multi-bucket model
        // a user holds one wallet row per Bucket, so
        // `find_by_user_id` (`.one()`) returns an arbitrary row and its
        // `total_balance` reflects a single pool — not the covered set. The
        // authoritative precheck is the infra layer's `consume_points_atomic`,
        // which sums `remaining_amount` across ALL covered-set ledgers
        // (`find_active_ledgers_by_expiration_for_update`) and reports the real
        // coverage-set availability via `insufficient_points`. Per-bucket
        // wallets are also created lazily inside the consume transaction via
        // `ensure_wallet_in_tx`, so no pre-created single wallet is needed.

        // Realize already-due free-periodic grants before opening the consume
        // transaction. This is the worker-down correctness backstop; failures
        // propagate as system errors instead of being masked as insufficient
        // balance.
        self.reconcile_due_for_user(realm_id, user_id, chrono::Utc::now())
            .await?;

        let saved_transactions = self
            .repository
            .consume_points_atomic(realm_id, user_id, client_app_id, amount, description, None)
            .await?;

        tracing::info!(
            realm_id = %realm_id,
            user_id = %user_id,
            amount,
            txn_count = saved_transactions.len(),
            "Points consumed successfully (window-first mix, per-bucket transactions)"
        );

        Ok(saved_transactions)
    }

    /// Idempotency replay. Reassemble the original consume result
    /// set from its primary transaction id WITHOUT re-deducting. Used by the
    /// HTTP-layer Redis-cache replay path when `check_or_create` returns a
    /// cached primary transaction: the primary → correlation_id → all N sibling
    /// per-bucket transactions. Legacy single-pool rows replay as 1 transaction.
    /// No permission check is performed here — the caller (HTTP layer) has
    /// already authorized the request, and the primary transaction id comes from
    /// our own idempotency cache, not from untrusted input.
    pub async fn replay_consume(
        &self,
        realm_id: &str,
        primary_txn_id: Uuid,
    ) -> Result<Vec<PointsTransaction>, CoreError> {
        self.repository
            .replay_consume_by_primary(realm_id, primary_txn_id)
            .await
    }

    /// Surface the ledger-level allocations of a consume by its `correlation_id`.
    /// Used by the SDK consume response to populate the
    /// `allocations` slice without re-deducting. Legacy single-pool rows (NULL
    /// correlation_id) return an empty vec.
    /// No permission check: the caller (HTTP layer) has already authorized the
    /// request and the correlation_id comes from our own consume result.
    pub async fn find_consumption_allocations_by_correlation_id(
        &self,
        realm_id: &str,
        correlation_id: &str,
    ) -> Result<Vec<ConsumptionAllocationView>, CoreError> {
        self.repository
            .find_consumption_allocations_by_correlation_id(realm_id, correlation_id)
            .await
    }

    /// Get a single transaction by ID
    pub async fn get_transaction(
        &self,
        identity: Identity,
        realm_id: &str,
        transaction_id: Uuid,
    ) -> Result<PointsTransaction, CoreError> {
        // Check view permissions
        ensure_policy(
            self.policy.can_view_points(identity.clone(), None).await,
            "Insufficient permissions to view transaction",
        )?;

        // Check realm boundary
        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot access transaction from a different realm".to_string(),
            ));
        }

        let transaction = self
            .repository
            .find_transaction_by_id(realm_id, transaction_id)
            .await?
            .ok_or(CoreError::NotFound)?;

        Ok(transaction)
    }

    /// List transactions with filters
    #[tracing::instrument(
        // Governance: identity carries user_id/realm_id; filters carry
        // user_id/bucket_id; realm_id conservatively skipped.
        skip(self, identity, realm_id, filters),
        fields(db.system = "postgres", db.operation = "list_transactions")
    )]
    pub async fn list_transactions(
        &self,
        identity: Identity,
        realm_id: &str,
        filters: TransactionFilters,
    ) -> Result<Paginated<PointsTransaction>, CoreError> {
        // Check view permissions
        ensure_policy(
            self.policy
                .can_view_points(identity.clone(), filters.user_id)
                .await,
            "Insufficient permissions to view transactions",
        )?;

        // Check realm boundary
        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot access transactions from a different realm".to_string(),
            ));
        }

        // Only points managers can query across users; points.view alone is scoped to self.
        let can_view_all = self.policy.can_manage_points(identity.clone()).await;
        if !can_view_all && let Ok(current_user_id) = identity.user_id().parse::<Uuid>() {
            // Override filters to only show current user's transactions
            let mut restricted_filters = filters;
            restricted_filters.user_id = Some(current_user_id);
            return self
                .repository
                .find_transactions(realm_id, restricted_filters)
                .await;
        }

        self.repository.find_transactions(realm_id, filters).await
    }

    /// Grant points to a user (admin endpoint)
    /// Performs permission check and realm boundary check, validates input,
    /// then delegates to `grant_points_internal`.
    #[tracing::instrument(
        // Governance: identity carries user_id/realm_id; input payload
        // carries the target user_id; realm_id conservatively skipped.
        skip(self, identity, realm_id, input),
        fields(db.system = "postgres", db.operation = "grant_points")
    )]
    pub async fn grant_points(
        &self,
        identity: Identity,
        realm_id: &str,
        input: GrantPointsInput,
    ) -> Result<GrantPointsOutput, CoreError> {
        // Permission check
        ensure_policy(
            self.policy.can_manage_points(identity.clone()).await,
            "Insufficient permissions to grant points",
        )?;

        // Realm boundary check
        if !identity.has_access_to_realm(realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot grant points from a different realm".to_string(),
            ));
        }

        self.execute_grant(realm_id, input).await
    }

    /// Grant points to a user (SDK/ext endpoint)
    /// Skips identity/policy checks -- those are handled by the caller
    /// at the middleware level (API Key authentication).
    pub async fn grant_points_for_sdk(
        &self,
        realm_id: &str,
        input: GrantPointsInput,
    ) -> Result<GrantPointsOutput, CoreError> {
        self.execute_grant(realm_id, input).await
    }

    /// Shared implementation for granting points
    async fn execute_grant(
        &self,
        realm_id: &str,
        input: GrantPointsInput,
    ) -> Result<GrantPointsOutput, CoreError> {
        // Validate input
        input.validate()?;

        // Compute expires_at from validity_days
        let expires_at = input
            .validity_days
            .map(|days| chrono::Utc::now() + chrono::Duration::days(days));

        // Build description including the user-provided reason
        let description = Some(format!(
            "{}: {} points granted ({})",
            input.source_type.as_str(),
            input.amount,
            input.reason
        ));

        // Grant points via internal method. Admin/SDK grants are immediately
        // available (`effective_at = None`); exposing an `effective_at` entry
        // point on the grant API is explicitly out of scope.
        let ledger_id = self
            .grant_points_internal(
                realm_id,
                input.user_id,
                input.bucket_id,
                CreditType::GrantedCredit,
                input.source_type,
                input.amount,
                expires_at,
                None,
                Some(input.source_id),
                description,
                None,
            )
            .await?;

        // Derived fill: `granted_balance`/`total_balance`
        // come from `compute_available_balance` post-grant (same source as
        // `get_balance`), NOT from the wallet Stored columns. This keeps the
        // grant response consistent with the derived-balance world and
        // prevents any future-effective rows from leaking into the response.
        let now = chrono::Utc::now();
        let derived = self
            .repository
            .compute_available_balance(realm_id, input.user_id, &[input.bucket_id], now)
            .await?;
        let (granted_balance, total_balance) =
            derived
                .into_iter()
                .fold((0i64, 0i64), |(g, t), (credit_type, amount)| {
                    let new_t = t + amount;
                    let new_g = if credit_type == CreditType::GrantedCredit {
                        g + amount
                    } else {
                        g
                    };
                    (new_g, new_t)
                });

        Ok(GrantPointsOutput {
            transaction_id: ledger_id,
            user_id: input.user_id,
            amount: input.amount,
            granted_balance,
            total_balance,
            expires_at,
        })
    }

    /// Recharge points for a user (internal method for billing webhooks)
    pub async fn recharge_points_internal(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        amount: i64,
        recharge_type: RechargeType,
        external_ref_id: Option<String>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<PointsTransaction, CoreError> {
        if amount <= 0 {
            return Err(CoreError::invalid_amount(
                "Recharge amount must be positive",
            ));
        }

        // NOTE: No aggregate wallet-status precheck here. `find_by_user_id` returns
        // the cross-bucket aggregate wallet (most-restrictive status wins), but
        // recharge targets a SPECIFIC bucket — so a frozen sibling bucket would
        // wrongly block a healthy one. The authoritative per-bucket status check
        // is the infra layer's `recharge_points_atomic`, which resolves the
        // bucket wallet via `ensure_wallet_in_tx` and rejects non-active status
        // (same convention as consume — see NOTE above `consume_points`).

        let (credit_type, source_type) = match recharge_type {
            RechargeType::Subscribe => (
                CreditType::SubscriptionCredit,
                CreditSourceType::SubscriptionInitial,
            ),
            RechargeType::Renewal => (
                CreditType::SubscriptionCredit,
                CreditSourceType::SubscriptionRenewal,
            ),
        };

        let transaction = self
            .repository
            .recharge_points_atomic(
                realm_id,
                user_id,
                bucket_id,
                credit_type,
                source_type,
                amount,
                expires_at,
                None,
                external_ref_id,
            )
            .await?;

        tracing::info!(
            realm_id = %realm_id,
            user_id = %user_id,
            amount,
            recharge_type = %recharge_type.as_str(),
            balance_after = %transaction.balance_after,
            "Points recharged successfully"
        );

        Ok(transaction)
    }

    /// Revoke points by credit type (internal method for subscription cancellation and refunds)
    /// Revokes all unused points of a specific credit type for a user.
    /// This is used for subscription cancellation, refunds, and expiration.
    /// # Arguments
    /// * `realm_id` - The realm ID
    /// * `user_id` - The user ID
    /// * `credit_type` - The type of credit to revoke (topup or subscription)
    /// * `revocation_type` - The reason for revocation
    /// * `reason` - Human-readable reason
    /// # Returns
    /// Revocation output with details of revoked points
    pub async fn revoke_points_by_credit_type(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        revocation_type: RevocationType,
        reason: String,
    ) -> Result<RevokePointsOutput, CoreError> {
        let result = self
            .repository
            .revoke_points_by_credit_type_atomic(
                realm_id,
                user_id,
                bucket_id,
                credit_type,
                revocation_type,
                reason,
                None,
                None,
            )
            .await?;

        tracing::info!(
            realm_id = %realm_id,
            user_id = %user_id,
            credit_type = %credit_type.as_str(),
            total_revoked = result.total_revoked,
            ledger_count = result.ledger_ids.len(),
            revocation_type = %revocation_type.as_str(),
            "Points revoked successfully"
        );

        Ok(result)
    }

    /// Revoke remaining points from a specific ledger identified by source_id.
    /// Unlike `revoke_points_by_credit_type`, this targets only the single ledger
    /// whose `source_id` matches, preventing over-broad revocation of unrelated credits.
    pub async fn revoke_points_by_source_id(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        source_id: &str,
        revocation_type: RevocationType,
        reason: String,
    ) -> Result<RevokePointsOutput, CoreError> {
        let result = self
            .repository
            .revoke_points_by_source_id_atomic(
                realm_id,
                user_id,
                bucket_id,
                source_id,
                revocation_type,
                reason,
                None,
                None,
            )
            .await?;

        tracing::info!(
            realm_id = %realm_id,
            user_id = %user_id,
            source_id = %source_id,
            total_revoked = result.total_revoked,
            revocation_type = %revocation_type.as_str(),
            "Points revoked by source_id successfully"
        );

        Ok(result)
    }

    pub async fn revoke_topup_source_proportional(
        &self,
        realm_id: &str,
        user_id: Uuid,
        source_id: &str,
        refund_amount: i64,
        original_payment_amount: i64,
        refund_id: &str,
    ) -> Result<RevokePointsOutput, CoreError> {
        if original_payment_amount <= 0 {
            return Err(CoreError::BadRequest(
                "Original payment amount must be positive".to_string(),
            ));
        }

        if refund_amount <= 0 {
            return Err(CoreError::BadRequest(
                "Refund amount must be positive".to_string(),
            ));
        }

        if refund_amount > original_payment_amount {
            return Err(CoreError::BadRequest(
                "Refund amount cannot exceed original payment".to_string(),
            ));
        }

        let result = self
            .repository
            .revoke_topup_source_proportional_atomic(
                realm_id,
                user_id,
                source_id,
                refund_amount,
                original_payment_amount,
                refund_id,
            )
            .await?;

        if result.total_revoked == 0 {
            tracing::info!(
                realm_id = %realm_id,
                user_id = %user_id,
                refund_id = %refund_id,
                "No active topup ledgers found for proportional revocation"
            );
        } else {
            tracing::info!(
                realm_id = %realm_id,
                user_id = %user_id,
                refund_id = %refund_id,
                refund_amount = refund_amount,
                original_payment_amount = original_payment_amount,
                total_revoked = result.total_revoked,
                ledger_count = result.ledger_ids.len(),
                "Proportionally revoked topup points"
            );
        }

        Ok(result)
    }

    /// Internal method to grant points directly to ledger
    /// This is used by background services (registration, scheduler)
    /// and bypasses the public API layer validation.
    /// # Arguments
    /// * `realm_id` - The realm ID
    /// * `user_id` - The user ID
    /// * `credit_type` - Type of credit to grant
    /// * `source_type` - Source of the grant
    /// * `amount` - Amount to grant
    /// * `expires_at` - Optional expiration time (None = permanent)
    /// * `source_id` - Optional source ID for traceability
    /// # Returns
    /// Ok(ledger_id) on success -- the ID of the created credit ledger entry
    /// # Errors
    /// - InvalidAmount if amount <= 0
    /// - Database errors
    /// # Security
    /// This is an internal method (NOT an HTTP endpoint) and will only be called
    /// from trusted internal services (RegistrationService, GrantScheduler).
    /// No authorization checks are performed.
    pub async fn grant_points_internal(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        source_type: crate::points::entities::CreditSourceType,
        amount: i64,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        // Expected effective time. `None` ⟺ immediately
        // available (current behavior); `Some(t)` ⟺ enters the available
        // set only when `effective_at <= NOW()`. Passed through to
        // `grant_points_atomic`.
        effective_at: Option<chrono::DateTime<chrono::Utc>>,
        source_id: Option<String>,
        description: Option<String>,
        idempotency_key: Option<String>,
    ) -> Result<Uuid, CoreError> {
        if amount <= 0 {
            return Err(CoreError::BadRequest(
                "Grant amount must be positive".to_string(),
            ));
        }

        // NOTE: No aggregate wallet-status precheck here. `find_by_user_id` returns
        // the cross-bucket aggregate wallet (most-restrictive status wins), but
        // grant targets a SPECIFIC bucket — so a frozen sibling bucket would
        // wrongly block a healthy one. The authoritative per-bucket status check
        // is the infra layer's `grant_points_atomic`, which resolves the
        // bucket wallet via `ensure_wallet_in_tx` and rejects non-active status
        // (same convention as consume — see NOTE above `consume_points`).

        let saved_ledger = self
            .repository
            .grant_points_atomic(
                realm_id,
                user_id,
                bucket_id,
                credit_type,
                source_type,
                amount,
                expires_at,
                effective_at,
                source_id,
                description,
                idempotency_key,
            )
            .await?;

        tracing::info!(
            realm_id = %realm_id,
            user_id = %user_id,
            amount,
            credit_type = ?credit_type,
            source_type = ?source_type,
            expires_at = ?expires_at,
            effective_at = ?effective_at,
            "Points granted internally"
        );

        Ok(saved_ledger.id)
    }

    /// Compute the per-window quota view for a (user, bucket), aggregating
    /// across all active subscription + free-periodic quota entitlements.
    /// For each active entitlement's snapshot window, queries the consume
    /// aggregation port (`sum_consume_in_window`) for the sliding window
    /// `[now - window_seconds, now]`, derives `remaining = max(0, limit - used)`,
    /// then folds windows by stable `key` taking the **minimum remaining**
    /// across entitlements (tightest constraint wins). `is_tightest`
    /// flags the minimum-remaining window; `exhausted` flags remaining == 0.
    /// `resets_at` is the approximate next reset point of the tightest window.
    /// Returns one `QuotaWindowView` per distinct `key`. Empty when the user
    /// has no active quota entitlement for this bucket.
    pub async fn compute_quota_windows_view(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<QuotaWindowView>, CoreError> {
        self.compute_quota_windows_view_for_credit_types(
            realm_id,
            user_id,
            bucket_id,
            &[
                CreditType::SubscriptionCredit,
                CreditType::FreePeriodicCredit,
            ],
            now,
        )
        .await
    }

    async fn compute_quota_windows_view_for_credit_types(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_types: &[CreditType],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<QuotaWindowView>, CoreError> {
        // Active entitlements for the requested window credit types. Each entitlement
        // carries its own credit_type, which selects the consume stream to
        // aggregate against.
        let mut entitlements = Vec::new();
        for &credit_type in credit_types {
            entitlements.extend(
                self.repository
                    .find_active_quota_entitlements(
                        realm_id,
                        user_id,
                        Some(bucket_id),
                        credit_type,
                        now,
                    )
                    .await?,
            );
        }

        // Pre-fetch used amounts for every distinct (credit_type, window_seconds)
        // pair in the active entitlements' snapshots. Done in the async body
        // (closures cannot `.await`); the pure aggregator then consumes a
        // synchronous lookup. De-dupes repeated window lengths across
        // entitlements sharing a key.
        let mut used_map: std::collections::HashMap<(CreditType, i64), i64> =
            std::collections::HashMap::new();
        for ent in &entitlements {
            for w in &ent.quota_windows {
                let k = (ent.credit_type, w.window_seconds);
                if used_map.contains_key(&k) {
                    continue;
                }
                let window_start = now - chrono::Duration::seconds(w.window_seconds);
                let used = self
                    .repository
                    .sum_consume_in_window(
                        realm_id,
                        user_id,
                        bucket_id,
                        ent.credit_type,
                        window_start,
                    )
                    .await?;
                used_map.insert(k, used);
            }
        }

        let mut used_lookup = |credit_type: CreditType, window_seconds: i64| -> i64 {
            used_map
                .get(&(credit_type, window_seconds))
                .copied()
                .unwrap_or(0)
        };

        Ok(aggregate_quota_windows(
            &entitlements,
            &mut used_lookup,
            now,
        ))
    }

    /// Compute the window-quota available amount for consume coordination
    /// `min over (active entitlement windows) of (limit - used)`,
    /// i.e. the tightest window's remaining. Returns 0 when no active quota
    /// entitlement exists (window-quota contributes nothing; pool side handles
    /// the rest).
    pub async fn compute_window_available(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, CoreError> {
        let views = self
            .compute_quota_windows_view(realm_id, user_id, bucket_id, now)
            .await?;
        Ok(views.iter().map(|v| v.remaining).min().unwrap_or(0))
    }

    async fn compute_window_available_for_credit_type(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, CoreError> {
        let views = self
            .compute_quota_windows_view_for_credit_types(
                realm_id,
                user_id,
                bucket_id,
                &[credit_type],
                now,
            )
            .await?;
        Ok(views.iter().map(|v| v.remaining).min().unwrap_or(0))
    }
}

// These are pure functions separated from the async service wiring so the
// window-availability orchestration (multi-window min aggregation, key
// stability, slide recovery) is unit-testable without a DB or a port mock.

/// Derive a stable display `key` from a window length in seconds.
/// Common lengths map to human-readable keys (`5h`/`day`/`week`/`month`);
/// any length that does not map cleanly falls back to `"{seconds}s"`. The key
/// is the frontend's stable window identity: the same
/// length always yields the same key, so re-renders / config edits do not
/// drift the window row identity. Month ≈ 30d (assumption A3).
pub fn derive_window_key(window_seconds: i64) -> String {
    const HOUR: i64 = 3_600;
    const DAY: i64 = 86_400;

    match window_seconds {
        s if s == 5 * HOUR => "5h".to_string(),
        s if s > 0 && s % DAY == 0 => {
            let days = s / DAY;
            match days {
                1 => "day".to_string(),
                7 => "week".to_string(),
                30 => "month".to_string(),
                _ => format!("{days}d"),
            }
        }
        s if s > 0 && s % HOUR == 0 => {
            let hours = s / HOUR;
            match hours {
                1 => "1h".to_string(),
                _ => format!("{hours}h"),
            }
        }
        s if s > 0 => format!("{s}s"),
        // Non-positive lengths are invalid config; surface a stable key rather
        // than panic so a bad snapshot never crashes the read path. Validation
        // rejects these at grant time.
        _ => format!("{window_seconds}s"),
    }
}

/// Aggregate active entitlements into per-key window views, taking the
/// minimum remaining across entitlements that share a window key.
/// `used_lookup(credit_type, window_seconds) -> i64` supplies the consumed
/// amount for a window; in production this is the `sum_consume_in_window`
/// port, in tests it is a pure closure (enabling slide-recovery tests that
/// vary the consumed amount by window length).
/// Aggregation rules:
///
/// - Windows are grouped by `key` across ALL entitlements (subscription +
///   free-periodic) for this (user, bucket).
/// - Per key: `used = max(used over entitlements sharing key)`,
///   `limit = sum(limit over entitlements sharing key)`,
///   `remaining = max(0, limit - used)`.
///
/// Rationale: a window key is defined by its length (e.g. `week`), so
/// multiple entitlements with the same key share the SAME sliding consume
/// window — their used amounts are identical, and limits stack. Taking
/// `max(used)` (identical across the key) and summing limits yields the
/// correct shared-window remaining. This matches "各窗口剩余最小值" for
/// DISTINCT lengths and "并集" for same-length entitlements.
///
/// - `is_tightest` flags the minimum-remaining window (ties: first by key
///   order).
/// - `exhausted` flags `remaining == 0`.
/// - `resets_at` = `now + window_seconds` for the window's nominal reset
///   cadence (approximate; precise oldest-consume reset is D1, deferred).
fn aggregate_quota_windows(
    entitlements: &[PointsQuotaEntitlement],
    used_lookup: &mut dyn FnMut(CreditType, i64) -> i64,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<QuotaWindowView> {
    use std::collections::HashMap;

    // Per-key accumulator: (limit_sum, used_max, window_seconds).
    // window_seconds is identical across rows sharing a key (key is derived
    // from length), so the first-seen value is canonical.
    let mut by_key: HashMap<String, (i64, i64, i64)> = HashMap::new();
    for ent in entitlements {
        for w in &ent.quota_windows {
            let used = used_lookup(ent.credit_type, w.window_seconds).max(0);
            by_key
                .entry(w.key.clone())
                .and_modify(|(limit_sum, used_max, _sec)| {
                    *limit_sum += w.limit;
                    if used > *used_max {
                        *used_max = used;
                    }
                })
                .or_insert((w.limit, used, w.window_seconds));
        }
    }

    let mut views: Vec<QuotaWindowView> = by_key
        .into_iter()
        .map(|(key, (limit, used, window_seconds))| {
            let remaining = (limit - used).max(0);
            QuotaWindowView {
                key,
                limit,
                used,
                remaining,
                window_seconds,
                resets_at: Some(now + chrono::Duration::seconds(window_seconds)),
                is_tightest: false,
                exhausted: remaining == 0,
            }
        })
        .collect();

    // Mark the tightest (minimum remaining) window. Stable tiebreak by key so
    // the flag is deterministic across re-aggregations.
    if let Some(min_remaining) = views.iter().map(|v| v.remaining).min() {
        views.sort_by(|a, b| a.key.cmp(&b.key));
        let mut tightest_set = false;
        for v in &mut views {
            if !tightest_set && v.remaining == min_remaining {
                v.is_tightest = true;
                tightest_set = true;
            }
        }
    } else {
        views.sort_by(|a, b| a.key.cmp(&b.key));
    }

    views
}

/// Planned split of a single consume across the window-quota and pool sides
/// Produced by the pure `plan_mixed_consume`
/// orchestrator; the infra `consume_points_atomic` path applies it
/// inside one transaction.
/// Invariants:
///
/// - `window_part + pool_part == amount` for the `Ok` variant.
/// - `window_part <= window_available` (window side never overspends).
/// - `pool_part <= pool_available` (pool side never overspends).
/// - When `amount > window_available + pool_available` the consume is
///   rejected wholesale (`Insufficient`) — no partial deduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixedConsumePlan {
    /// Apply `window_part` to the window-quota side and `pool_part` to the
    /// pool side. Either part may be 0 (window-only / pool-only consume).
    Ok { window_part: i64, pool_part: i64 },
    /// Total availability (`window_available + pool_available`) is below the
    /// requested `amount`. The caller rejects the consume wholesale.
    Insufficient,
}

/// Pure consume-mix orchestrator.
///
/// Splits a requested `amount` into a window-quota part and a pool part:
///
/// - `window_part = min(amount, window_available)` (window-first).
/// - `pool_part = amount - window_part` (overflow to pool).
/// - If `amount > window_available + pool_available` → `Insufficient`
///   (reject wholesale, no partial deduction).
///
/// `window_available` is the tightest-window remaining
/// (`compute_window_available`, min over active windows); `pool_available` is
/// the pool-side aggregate (`compute_available_balance` over pool credit
/// types). Both are computed by the infra path inside the consume transaction
/// and passed here; this function is pure so the split + overspend
/// guard is unit-testable without a DB.
///
/// Negative inputs are treated as 0 availability (defensive: a negative
/// remaining from a shrunk quota clamps to 0 upstream, but this guard keeps
/// the overspend invariant even if a negative slips through).
pub fn plan_mixed_consume(
    window_available: i64,
    pool_available: i64,
    amount: i64,
) -> MixedConsumePlan {
    let window_avail = window_available.max(0);
    let pool_avail = pool_available.max(0);

    // Overspend guard: reject wholesale when total availability is below the
    // requested amount. No partial deduction.
    if amount > window_avail + pool_avail {
        return MixedConsumePlan::Insufficient;
    }

    // Window-first split. min(amount, window_avail); the rest overflows to pool.
    let window_part = amount.min(window_avail);
    let pool_part = amount - window_part;

    MixedConsumePlan::Ok {
        window_part,
        pool_part,
    }
}

// Governance tests.
// Covers: domain points service `consume_points`,
// `list_transactions`, `grant_points` instrument skip correctness.
// WHY: these methods take `identity` (carries user_id/realm_id), `realm_id`,
// and `input`/`filters` (reference user_id/bucket_id). If the `#[instrument]`
// macro ever stops skipping those, the identifiers leak into a span field.
// Source-scan baseline, anchored per method to the
// immediately-preceding `#[tracing::instrument(...)]`.
#[cfg(test)]
mod instrument_skip_tests {
    const SRC: &str = include_str!("service.rs");

    fn instrument_body_preceding(fn_name: &str) -> String {
        let needle = format!("fn {fn_name}");
        let fn_pos = SRC
            .find(&needle)
            .unwrap_or_else(|| panic!("fn {fn_name} not found in source"));
        let attr_start = SRC[..fn_pos]
            .rfind("#[tracing::instrument(")
            .unwrap_or_else(|| panic!("no #[tracing::instrument( preceding fn {fn_name}"));
        let body_start = attr_start + "#[tracing::instrument(".len();
        // Find the attribute close: the first line at/after body_start whose
        // trimmed content is exactly `)]`. This handles indented closes (e.g.
        // inside an `impl` block) and ignores inline `))]` sequences such as
        // `#[validate(length(...))]` that appear on struct fields.
        let tail = &SRC[body_start..];
        let mut consumed = 0usize;
        for line in tail.lines() {
            let prev = consumed;
            consumed += line.len() + 1; // +1 for the line separator
            if line.trim() == ")]" {
                return tail[..prev].to_string();
            }
        }
        panic!("unterminated #[tracing::instrument( for fn {fn_name}")
    }

    #[test]
    fn instrument_skip_points_consume_excludes_identity_realm_input() {
        let body = instrument_body_preceding("consume_points");
        for required in ["identity", "realm_id", "input"] {
            assert!(
                body.contains(required),
                "consume_points must skip `{required}`; body was:\n{body}"
            );
        }
        for banned in ["token", "password", "email", "secret", "user_id"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "consume_points span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_points_list_transactions_excludes_identity_filters() {
        let body = instrument_body_preceding("list_transactions");
        for required in ["identity", "realm_id", "filters"] {
            assert!(
                body.contains(required),
                "list_transactions must skip `{required}`; body was:\n{body}"
            );
        }
        for banned in ["token", "password", "email", "secret", "user_id"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "list_transactions span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_points_grant_excludes_identity_realm_input() {
        let body = instrument_body_preceding("grant_points");
        for required in ["identity", "realm_id", "input"] {
            assert!(
                body.contains(required),
                "grant_points must skip `{required}`; body was:\n{body}"
            );
        }
        for banned in ["token", "password", "email", "secret", "user_id"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "grant_points span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }
}

// window-quota availability unit tests.
// These target the pure orchestration (`derive_window_key` +
// `aggregate_quota_windows`) — the value-bearing logic of window availability.
// They do NOT touch the DB or the PointsRepository port; the async service
// methods (`compute_quota_windows_view` / `compute_window_available`) are thin
// wiring over these pure functions and the port calls, validated end-to-end by
// scenario tests.
#[cfg(test)]
mod window_tests {
    use super::*;
    use crate::points::entities::{QuotaEntitlementStatus, QuotaSourceType, QuotaWindow};

    const HOUR: i64 = 3_600;
    const DAY: i64 = 86_400;
    const WEEK: i64 = 7 * DAY;
    const MONTH: i64 = 30 * DAY;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-06-29T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn window(window_seconds: i64, limit: i64) -> QuotaWindow {
        QuotaWindow {
            window_seconds,
            limit,
            key: derive_window_key(window_seconds),
        }
    }

    fn entitlement(credit_type: CreditType, windows: &[QuotaWindow]) -> PointsQuotaEntitlement {
        PointsQuotaEntitlement {
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            realm_id: "realm".to_string(),
            bucket_id: Uuid::nil(),
            credit_type,
            source_type: QuotaSourceType::SubscriptionInitial,
            source_id: "src".to_string(),
            quota_windows: windows.to_vec(),
            effective_from: now(),
            effective_until: None,
            status: QuotaEntitlementStatus::Active,
            idempotency_key: "idem".to_string(),
            distribution_event_id: None,
            distribution_rule_id: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn window_key_common_lengths_map_to_stable_human_keys() {
        // Same length -> same key (stability under re-derivation).
        assert_eq!(derive_window_key(5 * HOUR), "5h");
        assert_eq!(derive_window_key(5 * HOUR), derive_window_key(5 * HOUR));

        assert_eq!(derive_window_key(DAY), "day");
        assert_eq!(derive_window_key(WEEK), "week");
        assert_eq!(derive_window_key(MONTH), "month");
    }

    #[test]
    fn window_key_month_is_30_days_canonical() {
        // A3: month ≈ 30d. 30d and 31d must NOT both map to "month"
        // (would collide distinct lengths into one frontend identity).
        assert_eq!(derive_window_key(30 * DAY), "month");
        assert_ne!(derive_window_key(31 * DAY), "month");
        assert_eq!(derive_window_key(31 * DAY), "31d");
    }

    #[test]
    fn window_key_distinct_lengths_yield_distinct_keys() {
        let keys = [
            derive_window_key(5 * HOUR),
            derive_window_key(DAY),
            derive_window_key(WEEK),
            derive_window_key(MONTH),
        ];
        let unique: std::collections::HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert_eq!(
            unique.len(),
            keys.len(),
            "distinct lengths must yield distinct keys: {keys:?}"
        );
    }

    #[test]
    fn window_key_non_whole_divisor_seconds_fall_back_to_seconds_form() {
        // 90 minutes is neither a whole day nor a whole hour -> seconds form.
        assert_eq!(derive_window_key(90 * 60), format!("{}s", 90 * 60));
        // 12h is a whole-hour length but has no special-case mapping -> "12h".
        assert_eq!(derive_window_key(12 * HOUR), "12h");
    }

    #[test]
    fn window_key_never_panics_on_invalid_length() {
        // Bad snapshot (<=0) must not crash the read path; stable fallback key.
        assert_eq!(derive_window_key(0), "0s");
        assert_eq!(derive_window_key(-60), "-60s");
    }

    #[test]
    fn window_view_tightest_flag_marks_minimum_remaining_across_distinct_lengths() {
        // One entitlement with two distinct-length windows (week + month).
        // Used amounts chosen so week (remaining 30) is tighter than month (remaining 50).
        let ent = entitlement(
            CreditType::SubscriptionCredit,
            &[window(WEEK, 100), window(MONTH, 200)],
        );
        let mut used = |_: CreditType, secs: i64| {
            if secs == WEEK {
                70
            } else if secs == MONTH {
                150
            } else {
                0
            }
        };

        let views = aggregate_quota_windows(&[ent], &mut used, now());
        let week = views.iter().find(|v| v.key == "week").unwrap();
        let month = views.iter().find(|v| v.key == "month").unwrap();

        assert_eq!(week.remaining, 30);
        assert!(week.is_tightest, "week (min remaining) must be tightest");
        assert!(!month.is_tightest);
        assert_eq!(month.remaining, 50);
        // exhausted flag is remaining == 0.
        assert!(!week.exhausted);
        assert!(!month.exhausted);
    }

    #[test]
    fn window_view_same_key_across_entitlements_stacks_limit_takes_min_remaining() {
        // Two entitlements each granting a `week` window (same key/length).
        // Same-length same-key windows SHARE the sliding consume window, so:
        // limit = 80 + 120 = 200, used = 50 (max, identical across the key)
        // remaining = 200 - 50 = 150
        let ent1 = entitlement(CreditType::SubscriptionCredit, &[window(WEEK, 80)]);
        let ent2 = entitlement(CreditType::FreePeriodicCredit, &[window(WEEK, 120)]);
        let mut used = |_: CreditType, secs: i64| if secs == WEEK { 50 } else { 0 };

        let views = aggregate_quota_windows(&[ent1, ent2], &mut used, now());
        assert_eq!(views.len(), 1, "same-key windows must collapse to one view");
        let v = &views[0];
        assert_eq!(v.key, "week");
        assert_eq!(v.limit, 200);
        assert_eq!(v.used, 50);
        assert_eq!(v.remaining, 150);
        assert!(v.is_tightest, "single window is trivially tightest");
    }

    #[test]
    fn window_view_exhausted_window_drives_available_to_zero() {
        // Week window fully consumed (used == limit) -> remaining 0, exhausted.
        // Month window still has budget. Tightest is the exhausted week.
        let ent = entitlement(
            CreditType::SubscriptionCredit,
            &[window(WEEK, 100), window(MONTH, 500)],
        );
        // Both windows consumed by 100 (week fully, month partially).
        let mut used = |_: CreditType, _: i64| 100;

        let views = aggregate_quota_windows(&[ent], &mut used, now());
        let week = views.iter().find(|v| v.key == "week").unwrap();
        let month = views.iter().find(|v| v.key == "month").unwrap();
        assert!(week.exhausted);
        assert_eq!(week.remaining, 0);
        assert!(week.is_tightest);
        assert!(!month.exhausted);
        assert_eq!(month.remaining, 400);

        // compute_window_available semantics: min remaining across windows.
        let min_remaining = views.iter().map(|v| v.remaining).min().unwrap();
        assert_eq!(min_remaining, 0);
    }

    #[test]
    fn window_view_used_clamped_at_zero_when_lookup_underflows_limit() {
        // If used > limit (e.g. quota shrunk after grant, or negative aggregation),
        // remaining clamps to 0 rather than going negative.
        let ent = entitlement(CreditType::FreePeriodicCredit, &[window(DAY, 10)]);
        let mut used = |_: CreditType, _: i64| 99;

        let views = aggregate_quota_windows(&[ent], &mut used, now());
        assert_eq!(views[0].remaining, 0);
        assert!(views[0].exhausted);
    }

    // WHY this test exists: window availability is a pure function of the
    // consume stream + window length. As the window slides, old consumes age
    // out and `used` drops — the orchestration must reflect that drop
    // immediately (no cached/stale remaining). This test fixes the
    // orchestration's contract: feeding a smaller `used` for the same window
    // yields a larger `remaining`, which is exactly the slide-recovery behavior
    // the consume path and dashboard rely on (test_window_slide_recovery
    // exercises the SQL slide end-to-end; here we pin the orchestration layer).

    #[test]
    fn window_view_slide_recovery_remaining_restores_as_used_drops() {
        let ent = entitlement(CreditType::SubscriptionCredit, &[window(WEEK, 100)]);

        // Before slide: 80 consumed in the week window -> remaining 20.
        let mut used_pre = |_: CreditType, _: i64| 80;
        let pre = aggregate_quota_windows(std::slice::from_ref(&ent), &mut used_pre, now());
        assert_eq!(pre[0].remaining, 20);

        // After slide: the same 80 units aged out (window_start advanced),
        // used Lookup now returns 10 -> remaining restores to 90.
        let mut used_post = |_: CreditType, _: i64| 10;
        let post = aggregate_quota_windows(&[ent], &mut used_post, now());
        assert_eq!(post[0].remaining, 90);
        assert!(post[0].remaining > pre[0].remaining);
    }

    #[test]
    fn window_view_empty_entitlements_yields_empty_view() {
        // No active quota entitlement -> no windows. compute_window_available
        // returns 0 (window side contributes nothing; pool handles the rest).
        let mut used = |_: CreditType, _: i64| 0;
        let views = aggregate_quota_windows(&[], &mut used, now());
        assert!(views.is_empty());
    }

    #[test]
    fn window_view_resets_at_advances_by_window_seconds() {
        // resets_at is a nominal reset cadence (now + window_seconds), used by
        // the dashboard "resets in ~Nh" hint. Verify it tracks window length.
        let ent = entitlement(CreditType::SubscriptionCredit, &[window(HOUR * 5, 100)]);
        let now = now();
        let mut used = |_: CreditType, _: i64| 0;
        let views = aggregate_quota_windows(&[ent], &mut used, now);
        assert_eq!(
            views[0].resets_at,
            Some(now + chrono::Duration::seconds(5 * HOUR))
        );
    }
}

// mixed-consume plan unit tests.
// These target the pure overspend-guard orchestrator (`plan_mixed_consume`)
// — the core of the consume mix. They do NOT touch the DB
// or the port; the in-transaction application (window-first deduction +
// overflow-to-pool) is infra, validated end-to-end by scenario tests.
// WHY: a bug here means either silent overspend (window/pool part exceeds
// availability) or a wrongly-rejected consume. Each test pins one arm of the
// decision so a regression in the split or the guard fails loudly.
#[cfg(test)]
mod mixed_consume_tests {
    use super::*;

    #[test]
    fn mixed_consume_window_covers_whole_amount_pool_part_zero() {
        // amount <= window_available ⟹ window-first, pool untouched.
        let plan = plan_mixed_consume(100, 50, 30);
        assert_eq!(
            plan,
            MixedConsumePlan::Ok {
                window_part: 30,
                pool_part: 0
            }
        );
    }

    #[test]
    fn mixed_consume_window_partial_overflow_to_pool() {
        // window covers part; remainder overflows atomically to pool.
        let plan = plan_mixed_consume(40, 60, 70);
        assert_eq!(
            plan,
            MixedConsumePlan::Ok {
                window_part: 40,
                pool_part: 30
            }
        );
    }

    #[test]
    fn mixed_consume_window_zero_pool_only() {
        // No window availability ⟹ entire consume drawn from pool.
        let plan = plan_mixed_consume(0, 80, 80);
        assert_eq!(
            plan,
            MixedConsumePlan::Ok {
                window_part: 0,
                pool_part: 80
            }
        );
    }

    #[test]
    fn mixed_consume_total_insufficient_rejects_wholesale() {
        // amount > window + pool ⟹ Insufficient. Wholesale reject — NO
        // partial deduction (the core anti-overspend invariant).
        let plan = plan_mixed_consume(40, 50, 100);
        assert_eq!(plan, MixedConsumePlan::Insufficient);
    }

    #[test]
    fn mixed_consume_exact_total_coverage_succeeds() {
        // amount == window + pool exactly ⟹ Ok (boundary: not insufficient).
        let plan = plan_mixed_consume(40, 60, 100);
        assert_eq!(
            plan,
            MixedConsumePlan::Ok {
                window_part: 40,
                pool_part: 60
            }
        );
    }

    #[test]
    fn mixed_consume_amount_zero_boundary_yields_zero_parts() {
        // amount = 0 ⟹ both parts 0 (no-op consume is well-defined, not
        // Insufficient). Pins that the guard uses strict `>`.
        let plan = plan_mixed_consume(10, 20, 0);
        assert_eq!(
            plan,
            MixedConsumePlan::Ok {
                window_part: 0,
                pool_part: 0
            }
        );
    }

    #[test]
    fn mixed_consume_no_availability_nonzero_amount_is_insufficient() {
        // Both sides 0, amount > 0 ⟹ Insufficient (not Ok with 0 parts).
        let plan = plan_mixed_consume(0, 0, 1);
        assert_eq!(plan, MixedConsumePlan::Insufficient);
    }

    #[test]
    fn mixed_consume_clamps_negative_availability_to_zero() {
        // Defensive: a negative remaining (shrunk quota / aggregation glitch)
        // is clamped to 0 so the overspend invariant holds even if a negative
        // slips through. Here window=-5 behaves like 0, so 6 from pool-only.
        let plan = plan_mixed_consume(-5, 10, 6);
        assert_eq!(
            plan,
            MixedConsumePlan::Ok {
                window_part: 0,
                pool_part: 6
            }
        );

        // And negative pool alone cannot rescue an insufficient consume.
        let plan = plan_mixed_consume(3, -2, 10);
        assert_eq!(plan, MixedConsumePlan::Insufficient);
    }
}

// Read-path realization unit tests.
// These pin the domain boundary for `reconcile_due_for_user`: scan only due
// free-periodic schedules, realize a bounded batch through the pregrant port,
// and propagate write failures instead of masking them as stale balance.
#[cfg(test)]
mod reconcile_evolution_tests {
    use super::*;
    use crate::points::grant_schedule::{GrantPeriodType, PointsGrantSchedule};
    use crate::points::policies::AllowAllPointsPolicy;
    use crate::points::ports::MockPointsRepository;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-06-29T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn due_free_schedule() -> PointsGrantSchedule {
        PointsGrantSchedule {
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            realm_id: "realm".to_string(),
            bucket_id: Uuid::nil(),
            subscription_id: None,
            entitlement_key: None,
            grant_period_type: GrantPeriodType::Daily,
            base_time: now(),
            next_grant_time: now(),
            points_per_period: 100,
            validity_days: 7,
            granted_periods: 0,
            max_periods: None,
            active: true,
            distribution_event_id: Uuid::nil(),
            distribution_rule_id: Uuid::nil(),
            created_at: now(),
            updated_at: now(),
        }
    }

    #[tokio::test]
    async fn reconcile_returns_ok_no_op_when_no_due_free_schedule() {
        // No due free schedule means the read path is a no-op; subscription
        // state is not guessed by this method.
        let mut repo = MockPointsRepository::new();
        repo.expect_find_due_free_grant_schedules_for_user()
            .times(1)
            .withf(|realm_id, user_id, _, limit| {
                realm_id == "realm" && *user_id == Uuid::nil() && *limit == 3
            })
            .returning(|_, _, _, _| Box::pin(async { Ok(Vec::new()) }));
        repo.expect_execute_distribution_event_atomic().times(0);

        let svc = PointsService::new(Arc::new(repo), Arc::new(AllowAllPointsPolicy));
        let res = svc
            .reconcile_due_for_user("realm", Uuid::nil(), now())
            .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn reconcile_realizes_due_free_schedule() {
        let mut repo = MockPointsRepository::new();
        let schedule = due_free_schedule();
        let expected_schedule = schedule.clone();
        // `reconcile_due_for_user` realizes a due schedule by executing ONE
        // FreePeriodicGrant distribution event bound to the schedule's rule,
        // using event_key/source_id = event_key_for_free_periodic(...,period 1).
        let expected_event_key = event_key_for_free_periodic(
            expected_schedule.user_id,
            expected_schedule.distribution_rule_id,
            1,
        );
        let expected_effective_from = expected_schedule.next_grant_time;
        let expected_rule_id = expected_schedule.distribution_rule_id;
        repo.expect_find_due_free_grant_schedules_for_user()
            .times(1)
            .returning(move |_, _, _, _| {
                Box::pin({
                    let schedule = schedule.clone();
                    async move { Ok(vec![schedule]) }
                })
            });
        repo.expect_execute_distribution_event_atomic()
            .times(1)
            .withf(move |event, selection| {
                event.realm_id == "realm"
                    && event.user_id == Uuid::nil()
                    && matches!(event.owner, DistributionRuleOwner::RealmRegistration)
                    && matches!(event.trigger, DistributionTrigger::FreePeriodicGrant)
                    && event.event_key == expected_event_key
                    && event.source_id == expected_event_key
                    && event.effective_from == expected_effective_from
                    && event.effective_until.is_none()
                    && matches!(
                        selection,
                        DistributionRuleSelection::ScheduledRule(rule_id)
                            if *rule_id == expected_rule_id
                    )
            })
            .returning(|_, _| Box::pin(async move { Ok(Vec::new()) }));

        let svc = PointsService::new(Arc::new(repo), Arc::new(AllowAllPointsPolicy));
        let res = svc
            .reconcile_due_for_user("realm", Uuid::nil(), now())
            .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn reconcile_propagates_realization_write_failure() {
        let mut repo = MockPointsRepository::new();
        let schedule = due_free_schedule();
        repo.expect_find_due_free_grant_schedules_for_user()
            .times(1)
            .returning(move |_, _, _, _| {
                Box::pin({
                    let schedule = schedule.clone();
                    async move { Ok(vec![schedule]) }
                })
            });
        repo.expect_execute_distribution_event_atomic()
            .times(1)
            .returning(|_, _| {
                Box::pin(async {
                    Err(CoreError::DatabaseError(
                        "distribution event execution failed".to_string(),
                    ))
                })
            });

        let svc = PointsService::new(Arc::new(repo), Arc::new(AllowAllPointsPolicy));
        let err = svc
            .reconcile_due_for_user("realm", Uuid::nil(), now())
            .await
            .expect_err("realization write errors must fail loud");
        assert!(matches!(err, CoreError::DatabaseError(_)));
    }
}
