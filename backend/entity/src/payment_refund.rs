use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "payment_refunds")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub realm_id: String,
    /// Payment provider that issued the refund (stripe, creem)
    pub payment_provider: String,
    /// Owning payment attempt (points/role revocation source id)
    pub payment_attempt_id: Uuid,
    /// Provider refund id (Stripe re_..., Creem refund id) — idempotency unit
    pub refund_id: String,
    /// This refund's own amount (minimal currency unit), not the cumulative total
    pub amount: i64,
    /// Immutable snapshot of the original payment amount (gate denominator)
    pub original_payment_amount: i64,
    /// Cumulative refunded total including this refund (full-refund gate input)
    pub cumulative_refunded_after: i64,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
