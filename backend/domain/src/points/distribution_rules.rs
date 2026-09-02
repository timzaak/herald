//! Points distribution rules domain model (multi-wallet grant rules).
//!
//! Replaces the old single-target model (one Mapping / Realm config -> one
//! bucket + one points strategy) with a rule set: each rule binds one target
//! account, one policy (fixed or quota) and a non-empty set of triggers, owned
//! by either an entitlement mapping or a realm registration config.
//!
//! This module defines the core types, the owner/billing-type validator
//! ([`validate_rule_for_owner`]) and the pure executor helpers used by the
//! atomic executor and the replay path: event-key builders, the
//! trigger→credit/source-type mapping, rule selection + stable sort, and the
//! replay result-folder with fail-loud corruption detection. The database
//! transaction executor itself (`execute_distribution_event_atomic`) lives in
//! the infra layer; this module holds the invariants and pure logic it shares.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::billing::BillingType;
use crate::common::entities::app_errors::CoreError;
use crate::points::entities::QuotaWindow;
use crate::points::grant_schedule::GrantPeriodType;

/// Owner of a distribution rule. A rule belongs to exactly one owner, and the
/// owner fixes which triggers and policies are legal.
///
/// - [`DistributionRuleOwner::EntitlementMapping`] rules are further
///   constrained by the mapping's [`BillingType`].
/// - [`DistributionRuleOwner::RealmRegistration`] rules only allow the
///   registration-related triggers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DistributionRuleOwner {
    EntitlementMapping(Uuid),
    RealmRegistration,
}

impl DistributionRuleOwner {
    /// Stable wire string persisted in `owner_type` (`entitlement_mapping` /
    /// `realm_registration`).
    pub fn as_str(&self) -> &'static str {
        match self {
            DistributionRuleOwner::EntitlementMapping(_) => "entitlement_mapping",
            DistributionRuleOwner::RealmRegistration => "realm_registration",
        }
    }

    /// The mapping id when this owner is an entitlement mapping, else `None`.
    pub fn mapping_id(&self) -> Option<Uuid> {
        match self {
            DistributionRuleOwner::EntitlementMapping(id) => Some(*id),
            DistributionRuleOwner::RealmRegistration => None,
        }
    }
}

/// The six automatic distribution triggers. Admin/SDK grants, system grants
/// and revocation sources are intentionally excluded: those are explicit
/// commands, not rule-fanned-out events.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DistributionTrigger {
    Topup,
    SubscriptionInitial,
    SubscriptionRenewal,
    SubscriptionUpgrade,
    Registration,
    FreePeriodicGrant,
}

impl DistributionTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            DistributionTrigger::Topup => "topup",
            DistributionTrigger::SubscriptionInitial => "subscription_initial",
            DistributionTrigger::SubscriptionRenewal => "subscription_renewal",
            DistributionTrigger::SubscriptionUpgrade => "subscription_upgrade",
            DistributionTrigger::Registration => "registration",
            DistributionTrigger::FreePeriodicGrant => "free_periodic_grant",
        }
    }
}

impl std::str::FromStr for DistributionTrigger {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "topup" => Ok(DistributionTrigger::Topup),
            "subscription_initial" => Ok(DistributionTrigger::SubscriptionInitial),
            "subscription_renewal" => Ok(DistributionTrigger::SubscriptionRenewal),
            "subscription_upgrade" => Ok(DistributionTrigger::SubscriptionUpgrade),
            "registration" => Ok(DistributionTrigger::Registration),
            "free_periodic_grant" => Ok(DistributionTrigger::FreePeriodicGrant),
            other => Err(CoreError::BadRequest(format!(
                "Invalid distribution trigger: {other}"
            ))),
        }
    }
}

impl std::fmt::Display for DistributionTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Grant policy of a rule. `Fixed` grants points (or schedules them for
/// free-periodic rules); `Quota` grants a rolling-window quota entitlement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "grant_mode", rename_all = "snake_case")]
pub enum DistributionPolicy {
    Fixed {
        amount: i64,
        validity_days: i64,
        /// Required for free-periodic fixed rules; `None` otherwise.
        grant_period_type: Option<GrantPeriodType>,
    },
    Quota {
        windows: Vec<QuotaWindow>,
    },
}

impl DistributionPolicy {
    pub fn grant_mode(&self) -> &'static str {
        match self {
            DistributionPolicy::Fixed { .. } => "fixed",
            DistributionPolicy::Quota { .. } => "quota",
        }
    }
}

/// A single distribution rule: one target account, one policy and a non-empty
/// trigger set, owned by a mapping or realm registration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PointsDistributionRule {
    pub id: Uuid,
    pub realm_id: String,
    pub owner: DistributionRuleOwner,
    pub bucket_id: Uuid,
    pub trigger_sources: Vec<DistributionTrigger>,
    pub policy: DistributionPolicy,
    pub enabled: bool,
    pub display_order: i32,
}

/// Stable distribution-rule validation errors. The API layer maps each variant
/// to a stable error code (`invalid_distribution_trigger`,
/// `invalid_distribution_policy`, `invalid_distribution_rule`,
/// `distribution_rule_conflict`); the domain only carries the semantic cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistributionRuleError {
    EmptyTriggerSources,
    TriggerNotAllowedForOwner(DistributionTrigger),
    PolicyNotAllowedForTrigger,
    InvalidFixedAmount,
    InvalidValidity,
    InvalidQuotaWindows,
    BucketOutsideRealm,
    BucketDisabled,
    RuleOutsideOwner,
}

impl std::fmt::Display for DistributionRuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DistributionRuleError::EmptyTriggerSources => {
                write!(f, "distribution rule must declare at least one trigger")
            }
            DistributionRuleError::TriggerNotAllowedForOwner(t) => {
                write!(f, "trigger '{}' is not allowed for this rule owner", t)
            }
            DistributionRuleError::PolicyNotAllowedForTrigger => {
                write!(f, "policy is not allowed for the declared triggers")
            }
            DistributionRuleError::InvalidFixedAmount => {
                write!(f, "fixed policy requires a positive points amount")
            }
            DistributionRuleError::InvalidValidity => {
                write!(f, "validity_days must be >= 0")
            }
            DistributionRuleError::InvalidQuotaWindows => {
                write!(f, "quota policy requires valid quota windows")
            }
            DistributionRuleError::BucketOutsideRealm => {
                write!(f, "target bucket is outside the rule's realm")
            }
            DistributionRuleError::BucketDisabled => {
                write!(f, "target bucket is disabled")
            }
            DistributionRuleError::RuleOutsideOwner => {
                write!(f, "rule does not belong to the declared owner")
            }
        }
    }
}

impl std::error::Error for DistributionRuleError {}

impl From<DistributionRuleError> for CoreError {
    fn from(err: DistributionRuleError) -> Self {
        CoreError::BadRequest(err.to_string())
    }
}

/// The set of triggers a billing type permits on a mapping-owned rule:
/// - `one_time` -> `topup` only.
/// - `recurring` -> subscription initial/renewal/upgrade.
/// - `non_renewing` -> subscription initial only.
fn allowed_triggers_for_billing_type(billing_type: BillingType) -> &'static [DistributionTrigger] {
    match billing_type {
        BillingType::OneTime => &[DistributionTrigger::Topup],
        BillingType::Recurring => &[
            DistributionTrigger::SubscriptionInitial,
            DistributionTrigger::SubscriptionRenewal,
            DistributionTrigger::SubscriptionUpgrade,
        ],
        BillingType::NonRenewing => &[DistributionTrigger::SubscriptionInitial],
    }
}

/// Validate a rule against its owner and (for mapping owners) billing type.
///
/// Encodes the owner/trigger/policy invariants so a rule cannot be persisted
/// with an illegal combination. Bucket realm/disabled checks are enforced at
/// the repository boundary where the bucket row is actually loaded; this
/// validator covers the self-contained rule shape only.
pub fn validate_rule_for_owner(
    rule: &PointsDistributionRule,
    billing_type: Option<BillingType>,
) -> Result<(), DistributionRuleError> {
    // De-duplicate while preserving order so repeated triggers do not slip
    // through as "non-empty" without being a real set.
    let mut seen = std::collections::HashSet::new();
    let trigger_sources: Vec<DistributionTrigger> = rule
        .trigger_sources
        .iter()
        .filter(|t| seen.insert(**t))
        .copied()
        .collect();

    if trigger_sources.is_empty() {
        return Err(DistributionRuleError::EmptyTriggerSources);
    }

    // Owner + billing-type -> allowed trigger subset.
    let allowed: &[DistributionTrigger] = match &rule.owner {
        DistributionRuleOwner::EntitlementMapping(_) => {
            let billing_type = billing_type.ok_or(
                // A mapping rule cannot be validated without its billing type.
                DistributionRuleError::TriggerNotAllowedForOwner(trigger_sources[0]),
            )?;
            allowed_triggers_for_billing_type(billing_type)
        }
        DistributionRuleOwner::RealmRegistration => &[
            DistributionTrigger::Registration,
            DistributionTrigger::FreePeriodicGrant,
        ],
    };
    for trigger in &trigger_sources {
        if !allowed.contains(trigger) {
            return Err(DistributionRuleError::TriggerNotAllowedForOwner(*trigger));
        }
    }

    // Policy shape + trigger-specific policy constraints.
    match &rule.policy {
        DistributionPolicy::Fixed {
            amount,
            validity_days,
            grant_period_type,
        } => {
            if *amount <= 0 {
                return Err(DistributionRuleError::InvalidFixedAmount);
            }
            if *validity_days < 0 {
                return Err(DistributionRuleError::InvalidValidity);
            }
            // grant_period_type is only meaningful for free-periodic rules; a
            // registration rule is a one-time fixed grant (no period).
            let is_free_periodic =
                trigger_sources.contains(&DistributionTrigger::FreePeriodicGrant);
            let is_registration = trigger_sources.contains(&DistributionTrigger::Registration);
            if !is_free_periodic && grant_period_type.is_some() {
                return Err(DistributionRuleError::PolicyNotAllowedForTrigger);
            }
            if is_registration && grant_period_type.is_some() {
                return Err(DistributionRuleError::PolicyNotAllowedForTrigger);
            }
        }
        DistributionPolicy::Quota { windows } => {
            // Quota is only valid for subscription / free-periodic credit, never
            // for registration (registration is a one-time fixed grant).
            if trigger_sources.contains(&DistributionTrigger::Registration) {
                return Err(DistributionRuleError::PolicyNotAllowedForTrigger);
            }
            if windows.is_empty()
                || windows.len() > 8
                || windows
                    .iter()
                    .any(|window| window.window_seconds <= 0 || window.limit < 0)
            {
                return Err(DistributionRuleError::InvalidQuotaWindows);
            }
        }
    }

    Ok(())
}

/// Write-side input for one distribution rule within an owner's upsert set.
///
/// `id` is `None` for a brand-new rule under this owner and `Some` for an
/// update to an existing rule owned by the same parent resource. The owner
/// (`owner_type` + mapping id / realm registration) is fixed by the parent
/// resource and is NOT carried here, so a caller cannot forge cross-owner
/// writes. Time-stamps (`created_at` / `updated_at`) are repository-owned and
/// read-only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleUpsert {
    /// `None` ⟺ create a new rule under the parent owner;
    /// `Some(id)` ⟺ update the existing rule with that id (must already belong
    /// to the same owner — otherwise the upsert rejects with
    /// [`DistributionRuleError::RuleOutsideOwner`] / a 409 conflict).
    pub id: Option<Uuid>,
    pub bucket_id: Uuid,
    pub trigger_sources: Vec<DistributionTrigger>,
    pub policy: DistributionPolicy,
    /// Explicit enable/disable. Disabling a referenced rule sets
    /// `enabled = false` but keeps the row (DEC-007) — there is no delete path.
    pub enabled: bool,
    pub display_order: i32,
}

impl RuleUpsert {
    /// Build a fully-resolved [`PointsDistributionRule`] under the given owner
    /// and realm, assigning a fresh id when `self.id` is `None`. Used by the
    /// upsert services to materialize the validated rule before persistence.
    pub fn into_rule_for_owner(
        self,
        realm_id: &str,
        owner: DistributionRuleOwner,
    ) -> PointsDistributionRule {
        PointsDistributionRule {
            id: self.id.unwrap_or_else(Uuid::now_v7),
            realm_id: realm_id.to_string(),
            owner,
            bucket_id: self.bucket_id,
            trigger_sources: self.trigger_sources,
            policy: self.policy,
            enabled: self.enabled,
            display_order: self.display_order,
        }
    }
}

/// A rule that references a Credit Bucket, surfaced on the bucket management
/// views. Read-only; the bucket views batch-load the referencing rules.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DistributionRuleReference {
    pub rule_id: Uuid,
    /// `entitlement_mapping` / `realm_registration`.
    pub owner_type: String,
    /// Present only when `owner_type = entitlement_mapping`.
    pub entitlement_mapping_id: Option<Uuid>,
    pub trigger_sources: Vec<String>,
    pub enabled: bool,
}

/// A distribution event to be executed atomically by the executor. Carries the
/// stable identity the executor uses to insert/lock the
/// `points_distribution_events` row.
#[derive(Debug, Clone)]
pub struct DistributionEvent {
    pub realm_id: String,
    pub user_id: Uuid,
    pub owner: DistributionRuleOwner,
    pub trigger: DistributionTrigger,
    pub event_key: String,
    pub source_id: String,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
}

// ---- event-key format --------------------------------------------------
//
// The `(realm, user, trigger, event_key)` tuple uniquely identifies a logical
// distribution event (the `points_distribution_events` UNIQUE constraint
// serializes concurrent execution). The event_key suffix encodes the business
// locator and MUST stay stable across replays of the same business event:
//
//   topup                      payment:{attempt_id}
//   subscription_initial       subscription:{subscription_id}:period:{period_start}
//   subscription_renewal       subscription:{subscription_id}:period:{period_start}
//   subscription_upgrade       subscription:{subscription_id}:upgrade:{provider_event_id}
//   registration               registration:{user_id}
//   free_periodic_grant        free:{user_id}:{rule_id}:period:{period_number}
//
// Builders live here so every caller constructs the same key shape; the
// `period_start` / `period_number` tokens are the RFC3339 / integer string the
// caller already has, so the builders never touch the database.

/// Build the topup event key from a payment attempt id.
pub fn event_key_for_payment(attempt_id: Uuid) -> String {
    format!("payment:{attempt_id}")
}

/// Build the subscription initial / renewal event key from the subscription id
/// and the normalized period-start token (RFC3339). Initial and renewal share
/// the period anchor so a replayed period webhook converges on the same row.
pub fn event_key_for_subscription_period(subscription_id: Uuid, period_start: &str) -> String {
    format!("subscription:{subscription_id}:period:{period_start}")
}

/// Build the subscription upgrade event key from the subscription id and the
/// provider's upgrade event id (unique per upgrade event).
pub fn event_key_for_subscription_upgrade(
    subscription_id: Uuid,
    provider_event_id: &str,
) -> String {
    format!("subscription:{subscription_id}:upgrade:{provider_event_id}")
}

/// Build the registration event key. A registration event selects both
/// `Registration` and `FreePeriodicGrant` rules in one transaction; both rule
/// sets share this single event row so a new user's whole initial grant set is
/// atomic.
pub fn event_key_for_registration(user_id: Uuid) -> String {
    format!("registration:{user_id}")
}

/// Build the free-periodic-grant event key for a subsequent period of a
/// scheduled rule. `period_number` is the 1-based schedule period.
pub fn event_key_for_free_periodic(user_id: Uuid, rule_id: Uuid, period_number: u32) -> String {
    format!("free:{user_id}:{rule_id}:period:{period_number}")
}

// ---- trigger → credit/source-type mapping ------------------------------
//
// Each automatic trigger fixes the `CreditType` and `CreditSourceType` written
// to ledger/transaction/entitlement rows so the executor never has to guess.
// The mapping is total over the six automatic triggers.

use crate::points::entities::{CreditSourceType, CreditType, QuotaSourceType};

/// The `(credit_type, source_type)` pair a trigger writes, used by the executor
/// to materialize ledger/transaction/entitlement rows.
pub fn credit_pair_for_trigger(trigger: DistributionTrigger) -> (CreditType, CreditSourceType) {
    match trigger {
        DistributionTrigger::Topup => (CreditType::TopupCredit, CreditSourceType::Topup),
        DistributionTrigger::SubscriptionInitial => (
            CreditType::SubscriptionCredit,
            CreditSourceType::SubscriptionInitial,
        ),
        DistributionTrigger::SubscriptionRenewal => (
            CreditType::SubscriptionCredit,
            CreditSourceType::SubscriptionRenewal,
        ),
        DistributionTrigger::SubscriptionUpgrade => (
            CreditType::SubscriptionCredit,
            CreditSourceType::SubscriptionUpgrade,
        ),
        DistributionTrigger::Registration => (
            CreditType::RegistrationCredit,
            CreditSourceType::Registration,
        ),
        DistributionTrigger::FreePeriodicGrant => (
            CreditType::FreePeriodicCredit,
            CreditSourceType::FreePeriodicGrant,
        ),
    }
}

/// Map a trigger to the quota-source-type the
/// `points_quota_entitlements.source_type` CHECK constraint accepts
/// (`subscription_*` / `free_periodic_grant`). Quota rules only exist for
/// subscription and free-periodic triggers, so this is total over the legal
/// quota triggers. A topup / registration quota is rejected upstream by the
/// rule validator and never reaches the executor.
pub fn quota_source_type_for_trigger(
    trigger: DistributionTrigger,
) -> Result<QuotaSourceType, CoreError> {
    match trigger {
        DistributionTrigger::SubscriptionInitial => Ok(QuotaSourceType::SubscriptionInitial),
        DistributionTrigger::SubscriptionRenewal => Ok(QuotaSourceType::SubscriptionRenewal),
        DistributionTrigger::SubscriptionUpgrade => Ok(QuotaSourceType::SubscriptionUpgrade),
        DistributionTrigger::FreePeriodicGrant => Ok(QuotaSourceType::FreePeriodicGrant),
        other => Err(CoreError::BadRequest(format!(
            "quota policy not allowed for trigger '{}'",
            other
        ))),
    }
}

// ---- selection / stable sort / dedup -----------------------------------

/// Select and stably order the rules that fire for a trigger.
///
/// - Keeps only rules whose `trigger_sources` contain `trigger`.
/// - Keeps only `enabled` rules (a disabled rule never participates in a NEW
///   event; it still participates via captured snapshots / completed events).
/// - Stably sorts by `(display_order, rule_id)` so ordering is deterministic
///   across calls and across config versions.
/// - De-duplicates by `rule_id` (a rule appearing twice in the input — e.g. a
///   registration event feeding both Registration and FreePeriodicGrant rules —
///   fires exactly once).
///
/// Pure function: no DB, no I/O. The executor's `CurrentOwnerRules` /
/// `ScheduledRule` paths call this on the rule set they loaded.
pub fn select_and_sort_rules(
    rules: &[PointsDistributionRule],
    trigger: DistributionTrigger,
) -> Vec<&PointsDistributionRule> {
    let mut picked: Vec<&PointsDistributionRule> = rules
        .iter()
        .filter(|r| r.enabled && r.trigger_sources.contains(&trigger))
        .collect();
    picked.sort_by(|a, b| {
        a.display_order
            .cmp(&b.display_order)
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut out: Vec<&PointsDistributionRule> = Vec::with_capacity(picked.len());
    for r in picked {
        if !out.iter().any(|o| o.id == r.id) {
            out.push(r);
        }
    }
    out
}

// ---- replay result folding + corruption detection ----------------------

/// Error raised when a completed event's persisted result rows do not match its
/// recorded `result_count`. This is a data-corruption signal (lost / duplicated
/// result row), not a normal control-flow path: a normal transaction failure
/// rolls back and never produces a `completed` row, so a completed event must
/// always reconstruct exactly `result_count` logical results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributionReplayCorruption {
    pub event_id: Uuid,
    pub expected: i32,
    pub actual: usize,
}

impl std::fmt::Display for DistributionReplayCorruption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "distribution event {} result corruption: expected {} results, reconstructed {}",
            self.event_id, self.expected, self.actual
        )
    }
}

impl std::error::Error for DistributionReplayCorruption {}

impl From<DistributionReplayCorruption> for CoreError {
    fn from(err: DistributionReplayCorruption) -> Self {
        CoreError::InternalServerError(err.to_string())
    }
}

/// Raw result rows reconstructed from a completed event by the replay path
/// (queried by `distribution_event_id` from each result table). `ledger_rows`
/// and `entitlement_rows` are paired with their rule id; `schedule_rows`
/// represent the primary result of a free-periodic fixed rule and the matching
/// ledger of its first period is folded into the schedule result rather than
/// emitted as a separate Fixed result.
#[derive(Debug, Clone, Default)]
pub struct ReplayResultRows {
    /// `(rule_id, bucket_id, ledger_id, amount)` — one per fixed ledger that is
    /// NOT the first-period ledger of a schedule.
    pub ledger_rows: Vec<(Uuid, Uuid, Uuid, i64)>,
    /// `(rule_id, bucket_id, entitlement_id)` — one per quota rule.
    pub entitlement_rows: Vec<(Uuid, Uuid, Uuid)>,
    /// `(rule_id, bucket_id, schedule_id, first_ledger_id)` — one per
    /// free-periodic fixed schedule. The first-ledger id is resolved via
    /// `points_grant_records.ledger_id` so the ledger is folded out, not
    /// double-counted.
    pub schedule_rows: Vec<(Uuid, Uuid, Uuid, Uuid)>,
}

/// Fold the raw replay rows into the logical [`DistributionGrantResult`] set in
/// the deterministic first-execution order, and validate the logical result
/// count against the recorded `result_count`.
///
/// Order: ledger (Fixed) → entitlement (Quota) → schedule (Schedule), each
/// group stably sorted by `(display_order, rule_id)` using the order the rows
/// arrive in (the executor persisted them in first-execution order). A schedule
/// absorbs its first-ledger: that ledger id is removed from the ledger set
/// before counting so it is not emitted as a second Fixed result.
///
/// Returns `Err(DistributionReplayCorruption)` when the logical count ≠
/// `result_count` (fail-loud on data corruption).
pub fn fold_replay_results(
    rows: ReplayResultRows,
    result_count: i32,
    event_id: Uuid,
) -> Result<Vec<DistributionGrantResult>, DistributionReplayCorruption> {
    // First-period ledgers attached to schedules are folded into the Schedule
    // result and must not be double-counted as Fixed results.
    let folded_ledger_ids: std::collections::HashSet<Uuid> = rows
        .schedule_rows
        .iter()
        .map(|(_, _, _, lid)| *lid)
        .collect();

    let mut results = Vec::new();
    for (rule_id, bucket_id, ledger_id, amount) in &rows.ledger_rows {
        if folded_ledger_ids.contains(ledger_id) {
            continue;
        }
        results.push(DistributionGrantResult::Fixed {
            rule_id: *rule_id,
            bucket_id: *bucket_id,
            ledger_id: *ledger_id,
            amount: *amount,
        });
    }
    for (rule_id, bucket_id, entitlement_id) in &rows.entitlement_rows {
        results.push(DistributionGrantResult::Quota {
            rule_id: *rule_id,
            bucket_id: *bucket_id,
            entitlement_id: *entitlement_id,
        });
    }
    for (rule_id, bucket_id, schedule_id, first_ledger_id) in &rows.schedule_rows {
        results.push(DistributionGrantResult::Schedule {
            rule_id: *rule_id,
            bucket_id: *bucket_id,
            schedule_id: *schedule_id,
            first_ledger_id: *first_ledger_id,
        });
    }

    if results.len() as i32 != result_count {
        return Err(DistributionReplayCorruption {
            event_id,
            expected: result_count,
            actual: results.len(),
        });
    }
    Ok(results)
}

/// How the executor resolves the rule set for a first-time event.
#[derive(Debug, Clone)]
pub enum DistributionRuleSelection {
    /// Payment attempts snapshot their matched topup / subscription_initial
    /// rules at creation; fulfillment replays that snapshot.
    CapturedPaymentRules(Vec<CapturedRuleRef>),
    /// Resolve the owner's currently-enabled rules matching the trigger.
    CurrentOwnerRules,
    /// A single schedule-bound rule, used for subsequent free-periodic periods.
    ScheduledRule(Uuid),
}

/// A rule + target-bucket reference captured at payment attempt creation.
#[derive(Debug, Clone)]
pub struct CapturedRuleRef {
    pub rule_id: Uuid,
    pub bucket_id: Uuid,
}

/// Logical grant result produced by executing one rule within an event. The
/// executor persists these via the shared `distribution_event_id` and the
/// replay path reconstructs them by querying ledger / quota entitlement /
/// schedule rows.
#[derive(Debug, Clone)]
pub enum DistributionGrantResult {
    Fixed {
        rule_id: Uuid,
        bucket_id: Uuid,
        ledger_id: Uuid,
        amount: i64,
    },
    Quota {
        rule_id: Uuid,
        bucket_id: Uuid,
        entitlement_id: Uuid,
    },
    Schedule {
        rule_id: Uuid,
        bucket_id: Uuid,
        schedule_id: Uuid,
        first_ledger_id: Uuid,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping_owner() -> DistributionRuleOwner {
        DistributionRuleOwner::EntitlementMapping(Uuid::now_v7())
    }

    fn realm_owner() -> DistributionRuleOwner {
        DistributionRuleOwner::RealmRegistration
    }

    fn rule(
        owner: DistributionRuleOwner,
        triggers: &[DistributionTrigger],
        policy: DistributionPolicy,
    ) -> PointsDistributionRule {
        PointsDistributionRule {
            id: Uuid::now_v7(),
            realm_id: "realm".to_string(),
            owner,
            bucket_id: Uuid::now_v7(),
            trigger_sources: triggers.to_vec(),
            policy,
            enabled: true,
            display_order: 0,
        }
    }

    #[test]
    fn empty_trigger_sources_rejected() {
        let r = rule(
            mapping_owner(),
            &[],
            DistributionPolicy::Fixed {
                amount: 10,
                validity_days: 0,
                grant_period_type: None,
            },
        );
        assert_eq!(
            validate_rule_for_owner(&r, Some(BillingType::OneTime)),
            Err(DistributionRuleError::EmptyTriggerSources)
        );
    }

    #[test]
    fn one_time_mapping_allows_only_topup_fixed() {
        let r = rule(
            mapping_owner(),
            &[DistributionTrigger::Topup],
            DistributionPolicy::Fixed {
                amount: 100,
                validity_days: 30,
                grant_period_type: None,
            },
        );
        assert!(validate_rule_for_owner(&r, Some(BillingType::OneTime)).is_ok());

        // subscription_initial not allowed for one_time
        let bad = rule(
            mapping_owner(),
            &[DistributionTrigger::SubscriptionInitial],
            DistributionPolicy::Fixed {
                amount: 100,
                validity_days: 0,
                grant_period_type: None,
            },
        );
        assert!(matches!(
            validate_rule_for_owner(&bad, Some(BillingType::OneTime)),
            Err(DistributionRuleError::TriggerNotAllowedForOwner(_))
        ));
    }

    #[test]
    fn non_renewing_mapping_allows_only_subscription_initial() {
        let r = rule(
            mapping_owner(),
            &[DistributionTrigger::SubscriptionInitial],
            DistributionPolicy::Fixed {
                amount: 50,
                validity_days: 0,
                grant_period_type: None,
            },
        );
        assert!(validate_rule_for_owner(&r, Some(BillingType::NonRenewing)).is_ok());

        let bad = rule(
            mapping_owner(),
            &[DistributionTrigger::SubscriptionRenewal],
            DistributionPolicy::Fixed {
                amount: 50,
                validity_days: 0,
                grant_period_type: None,
            },
        );
        assert!(matches!(
            validate_rule_for_owner(&bad, Some(BillingType::NonRenewing)),
            Err(DistributionRuleError::TriggerNotAllowedForOwner(_))
        ));
    }

    #[test]
    fn recurring_mapping_allows_subscription_triggers() {
        let triggers = [
            DistributionTrigger::SubscriptionInitial,
            DistributionTrigger::SubscriptionRenewal,
            DistributionTrigger::SubscriptionUpgrade,
        ];
        for t in triggers {
            let r = rule(
                mapping_owner(),
                &[t],
                DistributionPolicy::Fixed {
                    amount: 1,
                    validity_days: 0,
                    grant_period_type: None,
                },
            );
            assert!(
                validate_rule_for_owner(&r, Some(BillingType::Recurring)).is_ok(),
                "trigger {t:?} should be allowed for recurring"
            );
        }
    }

    #[test]
    fn fixed_amount_must_be_positive() {
        let r = rule(
            mapping_owner(),
            &[DistributionTrigger::Topup],
            DistributionPolicy::Fixed {
                amount: 0,
                validity_days: 0,
                grant_period_type: None,
            },
        );
        assert_eq!(
            validate_rule_for_owner(&r, Some(BillingType::OneTime)),
            Err(DistributionRuleError::InvalidFixedAmount)
        );
    }

    #[test]
    fn negative_validity_rejected() {
        let r = rule(
            mapping_owner(),
            &[DistributionTrigger::Topup],
            DistributionPolicy::Fixed {
                amount: 1,
                validity_days: -1,
                grant_period_type: None,
            },
        );
        assert_eq!(
            validate_rule_for_owner(&r, Some(BillingType::OneTime)),
            Err(DistributionRuleError::InvalidValidity)
        );
    }

    #[test]
    fn quota_requires_non_empty_windows() {
        let r = rule(
            mapping_owner(),
            &[DistributionTrigger::SubscriptionInitial],
            DistributionPolicy::Quota { windows: vec![] },
        );
        assert_eq!(
            validate_rule_for_owner(&r, Some(BillingType::Recurring)),
            Err(DistributionRuleError::InvalidQuotaWindows)
        );
    }

    #[test]
    fn quota_with_valid_windows_ok_for_recurring() {
        let r = rule(
            mapping_owner(),
            &[DistributionTrigger::SubscriptionInitial],
            DistributionPolicy::Quota {
                windows: vec![QuotaWindow {
                    window_seconds: 3600,
                    limit: 100,
                    key: "1h".to_string(),
                }],
            },
        );
        assert!(validate_rule_for_owner(&r, Some(BillingType::Recurring)).is_ok());
    }

    #[test]
    fn realm_registration_rejects_subscription_triggers() {
        let r = rule(
            realm_owner(),
            &[DistributionTrigger::SubscriptionInitial],
            DistributionPolicy::Fixed {
                amount: 1,
                validity_days: 0,
                grant_period_type: None,
            },
        );
        assert!(matches!(
            validate_rule_for_owner(&r, None),
            Err(DistributionRuleError::TriggerNotAllowedForOwner(_))
        ));
    }

    #[test]
    fn realm_registration_allows_registration_fixed_no_period() {
        let r = rule(
            realm_owner(),
            &[DistributionTrigger::Registration],
            DistributionPolicy::Fixed {
                amount: 100,
                validity_days: 0,
                grant_period_type: None,
            },
        );
        assert!(validate_rule_for_owner(&r, None).is_ok());

        // registration rule may not set a grant period
        let bad = rule(
            realm_owner(),
            &[DistributionTrigger::Registration],
            DistributionPolicy::Fixed {
                amount: 100,
                validity_days: 0,
                grant_period_type: Some(GrantPeriodType::Daily),
            },
        );
        assert_eq!(
            validate_rule_for_owner(&bad, None),
            Err(DistributionRuleError::PolicyNotAllowedForTrigger)
        );
    }

    #[test]
    fn realm_registration_rejects_quota_for_registration() {
        let r = rule(
            realm_owner(),
            &[DistributionTrigger::Registration],
            DistributionPolicy::Quota {
                windows: vec![QuotaWindow {
                    window_seconds: 3600,
                    limit: 10,
                    key: "1h".to_string(),
                }],
            },
        );
        assert_eq!(
            validate_rule_for_owner(&r, None),
            Err(DistributionRuleError::PolicyNotAllowedForTrigger)
        );
    }

    #[test]
    fn free_periodic_fixed_may_set_grant_period() {
        let r = rule(
            realm_owner(),
            &[DistributionTrigger::FreePeriodicGrant],
            DistributionPolicy::Fixed {
                amount: 10,
                validity_days: 7,
                grant_period_type: Some(GrantPeriodType::Daily),
            },
        );
        assert!(validate_rule_for_owner(&r, None).is_ok());
    }

    #[test]
    fn free_periodic_quota_ok_for_realm_registration() {
        let r = rule(
            realm_owner(),
            &[DistributionTrigger::FreePeriodicGrant],
            DistributionPolicy::Quota {
                windows: vec![QuotaWindow {
                    window_seconds: 86400,
                    limit: 5,
                    key: "day".to_string(),
                }],
            },
        );
        assert!(validate_rule_for_owner(&r, None).is_ok());
    }

    #[test]
    fn duplicate_triggers_deduplicated_not_counted_as_two() {
        let r = rule(
            mapping_owner(),
            &[DistributionTrigger::Topup, DistributionTrigger::Topup],
            DistributionPolicy::Fixed {
                amount: 1,
                validity_days: 0,
                grant_period_type: None,
            },
        );
        assert!(validate_rule_for_owner(&r, Some(BillingType::OneTime)).is_ok());
    }

    #[test]
    fn mapping_rule_without_billing_type_rejected() {
        let r = rule(
            mapping_owner(),
            &[DistributionTrigger::Topup],
            DistributionPolicy::Fixed {
                amount: 1,
                validity_days: 0,
                grant_period_type: None,
            },
        );
        assert!(matches!(
            validate_rule_for_owner(&r, None),
            Err(DistributionRuleError::TriggerNotAllowedForOwner(_))
        ));
    }

    fn upsert(
        id: Option<Uuid>,
        triggers: &[DistributionTrigger],
        policy: DistributionPolicy,
    ) -> RuleUpsert {
        RuleUpsert {
            id,
            bucket_id: Uuid::now_v7(),
            trigger_sources: triggers.to_vec(),
            policy,
            enabled: true,
            display_order: 0,
        }
    }

    fn fixed_policy(amount: i64) -> DistributionPolicy {
        DistributionPolicy::Fixed {
            amount,
            validity_days: 0,
            grant_period_type: None,
        }
    }

    /// `RuleUpsert` with `id = None` must materialize a fresh rule whose owner
    /// is the parent owner passed to `into_rule_for_owner` (the caller cannot
    /// forge an owner), and whose id is a freshly generated non-nil UUID.
    #[test]
    fn upsert_without_id_binds_to_parent_owner_and_assigns_fresh_id() {
        let owner = mapping_owner();
        let resolved = upsert(None, &[DistributionTrigger::Topup], fixed_policy(10))
            .into_rule_for_owner("realm", owner.clone());
        assert_eq!(resolved.owner, owner);
        assert_eq!(resolved.realm_id, "realm");
        assert_ne!(resolved.id, Uuid::nil());
        // Validate cleanly against the parent owner's billing type.
        assert!(validate_rule_for_owner(&resolved, Some(BillingType::OneTime)).is_ok());
    }

    /// `RuleUpsert` with `id = Some(existing)` preserves that id so the upsert
    /// path updates the existing rule in place rather than creating a duplicate.
    #[test]
    fn upsert_with_id_preserves_existing_rule_id() {
        let existing = Uuid::now_v7();
        let resolved = upsert(
            Some(existing),
            &[DistributionTrigger::Topup],
            fixed_policy(5),
        )
        .into_rule_for_owner("realm", realm_owner());
        assert_eq!(resolved.id, existing);
    }

    /// `DistributionRuleOwner::EntitlementMapping` exposes its mapping id and
    /// `RealmRegistration` exposes None — the upsert owner-check in the
    /// repository relies on this to reject cross-owner writes.
    #[test]
    fn owner_mapping_id_round_trips() {
        let id = Uuid::now_v7();
        assert_eq!(
            DistributionRuleOwner::EntitlementMapping(id).mapping_id(),
            Some(id)
        );
        assert_eq!(DistributionRuleOwner::RealmRegistration.mapping_id(), None);
        assert_eq!(
            DistributionRuleOwner::EntitlementMapping(id).as_str(),
            "entitlement_mapping"
        );
        assert_eq!(
            DistributionRuleOwner::RealmRegistration.as_str(),
            "realm_registration"
        );
    }

    // ---- executor pure-logic tests -------------------------------------

    fn rule_with(
        id: u128,
        display_order: i32,
        triggers: &[DistributionTrigger],
        policy: DistributionPolicy,
        enabled: bool,
    ) -> PointsDistributionRule {
        PointsDistributionRule {
            id: Uuid::from_u128(id),
            realm_id: "realm".to_string(),
            owner: mapping_owner(),
            bucket_id: Uuid::now_v7(),
            trigger_sources: triggers.to_vec(),
            policy,
            enabled,
            display_order,
        }
    }

    /// Selection keeps only rules that declare the trigger and are enabled,
    /// ordered by `(display_order, rule_id)` so a later-config re-resolution is
    /// deterministic. A disabled rule and a rule that does not declare the
    /// trigger are both excluded.
    #[test]
    fn select_and_sort_filters_and_orders_stably() {
        let rules = vec![
            rule_with(2, 10, &[DistributionTrigger::Topup], fixed_policy(2), true),
            rule_with(1, 5, &[DistributionTrigger::Topup], fixed_policy(1), false),
            rule_with(
                3,
                1,
                &[DistributionTrigger::SubscriptionInitial],
                fixed_policy(3),
                true,
            ),
            rule_with(4, 1, &[DistributionTrigger::Topup], fixed_policy(4), true),
        ];
        let picked = select_and_sort_rules(&rules, DistributionTrigger::Topup);
        // Rule 1 is disabled; rule 3 does not declare topup. Rule 4 (order 1)
        // precedes rule 2 (order 10).
        let ids: Vec<u128> = picked.iter().map(|r| r.id.as_u128()).collect();
        assert_eq!(ids, vec![4, 2]);
    }

    /// A rule appearing twice in the input (e.g. a registration event feeding
    /// both Registration and FreePeriodicGrant rule sets) is de-duplicated so it
    /// fires exactly once.
    #[test]
    fn select_and_sort_deduplicates_by_rule_id() {
        let r = rule_with(
            7,
            0,
            &[
                DistributionTrigger::Registration,
                DistributionTrigger::FreePeriodicGrant,
            ],
            fixed_policy(1),
            true,
        );
        let rules = vec![r.clone(), r];
        // The rule declares both triggers, so both trigger queries pick it, but
        // de-dup keeps one.
        let reg = select_and_sort_rules(&rules, DistributionTrigger::Registration);
        let free = select_and_sort_rules(&rules, DistributionTrigger::FreePeriodicGrant);
        assert_eq!(reg.len(), 1);
        assert_eq!(free.len(), 1);
    }

    /// `select_and_sort_rules` is stable: equal `(display_order, rule_id)`
    /// never happens (rule_id is unique), but the function must not reorder
    /// by any other field. Two rules, same display_order, must sort by id.
    #[test]
    fn select_and_sort_ties_break_by_rule_id() {
        let rules = vec![
            rule_with(20, 0, &[DistributionTrigger::Topup], fixed_policy(1), true),
            rule_with(10, 0, &[DistributionTrigger::Topup], fixed_policy(1), true),
        ];
        let picked = select_and_sort_rules(&rules, DistributionTrigger::Topup);
        let ids: Vec<u128> = picked.iter().map(|r| r.id.as_u128()).collect();
        assert_eq!(ids, vec![10, 20]);
    }

    /// Each trigger maps to a fixed `(credit_type, source_type)` pair the
    /// executor writes, covering all six automatic triggers.
    #[test]
    fn credit_pair_mapping_is_total() {
        use crate::points::entities::{CreditSourceType, CreditType};
        assert_eq!(
            credit_pair_for_trigger(DistributionTrigger::Topup),
            (CreditType::TopupCredit, CreditSourceType::Topup)
        );
        assert_eq!(
            credit_pair_for_trigger(DistributionTrigger::SubscriptionInitial),
            (
                CreditType::SubscriptionCredit,
                CreditSourceType::SubscriptionInitial
            )
        );
        assert_eq!(
            credit_pair_for_trigger(DistributionTrigger::SubscriptionRenewal),
            (
                CreditType::SubscriptionCredit,
                CreditSourceType::SubscriptionRenewal
            )
        );
        assert_eq!(
            credit_pair_for_trigger(DistributionTrigger::SubscriptionUpgrade),
            (
                CreditType::SubscriptionCredit,
                CreditSourceType::SubscriptionUpgrade
            )
        );
        assert_eq!(
            credit_pair_for_trigger(DistributionTrigger::Registration),
            (
                CreditType::RegistrationCredit,
                CreditSourceType::Registration
            )
        );
        assert_eq!(
            credit_pair_for_trigger(DistributionTrigger::FreePeriodicGrant),
            (
                CreditType::FreePeriodicCredit,
                CreditSourceType::FreePeriodicGrant
            )
        );
    }

    /// Event-key builders produce the stable key shapes the
    /// `points_distribution_events` unique constraint serializes on.
    #[test]
    fn event_key_builders_match_documented_format() {
        let attempt = Uuid::from_u128(0xA);
        let sub = Uuid::from_u128(0xB);
        let user = Uuid::from_u128(0xC);
        let rule = Uuid::from_u128(0xD);
        assert_eq!(
            event_key_for_payment(attempt),
            "payment:00000000-0000-0000-0000-00000000000a"
        );
        assert_eq!(
            event_key_for_subscription_period(sub, "2026-01-01T00:00:00Z"),
            "subscription:00000000-0000-0000-0000-00000000000b:period:2026-01-01T00:00:00Z"
        );
        assert_eq!(
            event_key_for_subscription_upgrade(sub, "evt_123"),
            "subscription:00000000-0000-0000-0000-00000000000b:upgrade:evt_123"
        );
        assert_eq!(
            event_key_for_registration(user),
            "registration:00000000-0000-0000-0000-00000000000c"
        );
        assert_eq!(
            event_key_for_free_periodic(user, rule, 3),
            "free:00000000-0000-0000-0000-00000000000c:00000000-0000-0000-0000-00000000000d:period:3"
        );
    }

    /// Replay folds ledger/quota/schedule rows into the logical result set,
    /// folding a schedule's first-ledger out so it is not double-counted as a
    /// Fixed result, and returns the results when the count matches.
    #[test]
    fn fold_replay_results_matches_count_and_folds_schedule_ledger() {
        let event = Uuid::from_u128(1);
        let r_fixed = Uuid::from_u128(10);
        let r_quota = Uuid::from_u128(11);
        let r_sched = Uuid::from_u128(12);
        let b = Uuid::from_u128(99);
        let first_ledger = Uuid::from_u128(200);
        let rows = ReplayResultRows {
            ledger_rows: vec![
                (r_fixed, b, Uuid::from_u128(100), 50),
                // The schedule's first-period ledger must be folded out.
                (r_sched, b, first_ledger, 10),
            ],
            entitlement_rows: vec![(r_quota, b, Uuid::from_u128(300))],
            schedule_rows: vec![(r_sched, b, Uuid::from_u128(400), first_ledger)],
        };
        // 3 logical results: Fixed + Quota + Schedule (schedule ledger folded).
        let folded = fold_replay_results(rows, 3, event).expect("counts match");
        assert_eq!(folded.len(), 3);
        assert!(matches!(
            folded[0],
            DistributionGrantResult::Fixed { rule_id, amount, .. } if rule_id == r_fixed && amount == 50
        ));
        assert!(matches!(
            folded[1],
            DistributionGrantResult::Quota { rule_id, .. } if rule_id == r_quota
        ));
        assert!(matches!(
            folded[2],
            DistributionGrantResult::Schedule { rule_id, first_ledger_id, .. }
                if rule_id == r_sched && first_ledger_id == first_ledger
        ));
    }

    /// A zero-rule completed event reconstructs an empty result set; count must
    /// be 0.
    #[test]
    fn fold_replay_results_zero_rule_event() {
        let event = Uuid::from_u128(2);
        let folded = fold_replay_results(ReplayResultRows::default(), 0, event).expect("empty ok");
        assert!(folded.is_empty());
    }

    /// A completed event whose reconstructed result count disagrees with the
    /// recorded `result_count` is corruption and must fail loud.
    #[test]
    fn fold_replay_results_fail_loud_on_count_mismatch() {
        let event = Uuid::from_u128(3);
        let rows = ReplayResultRows {
            ledger_rows: vec![(
                Uuid::from_u128(10),
                Uuid::from_u128(99),
                Uuid::from_u128(100),
                5,
            )],
            entitlement_rows: vec![],
            schedule_rows: vec![],
        };
        let err = fold_replay_results(rows, 2, event).expect_err("mismatch must fail loud");
        assert_eq!(
            err,
            DistributionReplayCorruption {
                event_id: event,
                expected: 2,
                actual: 1,
            }
        );
    }
}
