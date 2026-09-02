use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::billing::invoice::ActorType;
use crate::common::entities::app_errors::CoreError;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Source of a credit note — Stripe (externally synced) or Manual (admin-created).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CreditNoteSource {
    Stripe,
    Manual,
}

impl CreditNoteSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stripe => "stripe",
            Self::Manual => "manual",
        }
    }

    /// Actor to attribute invoice_history rows to: Manual refunds are operator-driven,
    /// Stripe refunds originate from webhooks (system).
    pub fn default_actor_type(self) -> ActorType {
        match self {
            Self::Stripe => ActorType::System,
            Self::Manual => ActorType::User,
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "stripe" => Some(Self::Stripe),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

impl std::str::FromStr for CreditNoteSource {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str_opt(s)
            .ok_or_else(|| CoreError::BadRequest(format!("Invalid credit note source: {}", s)))
    }
}

/// Lifecycle of a credit note.
///
/// - `Active`: applies to the invoice (refund amount deducted from `amount_remaining`).
/// - `Voided`: reversed; the refund amount was added back to `amount_remaining`.
///
/// Stripe voids a credit note via `credit_note.voided` events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CreditNoteStatus {
    Active,
    Voided,
}

impl CreditNoteStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Voided => "voided",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "voided" => Some(Self::Voided),
            _ => None,
        }
    }
}

impl std::str::FromStr for CreditNoteStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str_opt(s)
            .ok_or_else(|| CoreError::BadRequest(format!("Invalid credit note status: {}", s)))
    }
}

// ---------------------------------------------------------------------------
// Core entities
// ---------------------------------------------------------------------------

/// Credit note entity — persisted in the `credit_note` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditNote {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub realm_id: String,
    /// Refund amount in the smallest currency unit (positive integer).
    pub amount: i64,
    pub currency: String,
    pub source: CreditNoteSource,
    /// Lifecycle: active (applies to invoice) or voided (refund reversed).
    pub status: CreditNoteStatus,
    /// Stripe credit note ID; present only for `source = Stripe` (used as idempotency key).
    pub external_credit_note_id: Option<String>,
    /// Admin-provided memo; present only for `source = Manual`.
    pub memo: Option<String>,
    /// User who created the credit note; present only for `source = Manual`.
    pub created_by_user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Input / command types
// ---------------------------------------------------------------------------

/// Input for creating a new credit note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCreditNote {
    pub invoice_id: Uuid,
    pub realm_id: String,
    /// Refund amount in the smallest currency unit (must be > 0).
    pub amount: i64,
    pub currency: String,
    pub source: CreditNoteSource,
    pub external_credit_note_id: Option<String>,
    pub memo: Option<String>,
    pub created_by_user_id: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// Traits (ports)
// ---------------------------------------------------------------------------

/// Repository for credit note persistence operations.
#[allow(clippy::manual_async_fn)]
pub trait CreditNoteRepository: Send + Sync {
    /// List all credit notes for a given invoice (within a realm).
    fn find_by_invoice_id(
        &self,
        realm_id: &str,
        invoice_id: Uuid,
    ) -> impl Future<Output = Result<Vec<CreditNote>, CoreError>> + Send;

    /// Look up a credit note by its external (Stripe) ID. Used for idempotency in
    /// `credit_note.created` webhook handlers.
    fn find_by_external_id(
        &self,
        external_credit_note_id: &str,
    ) -> impl Future<Output = Result<Option<CreditNote>, CoreError>> + Send;

    /// Transactional: create the credit note AND atomically update the parent invoice's
    /// `amount_refunded` / `amount_remaining`. Must reject if `amount` would push
    /// `amount_refunded` beyond `total`.
    fn create_credit_note_and_update_invoice(
        &self,
        input: NewCreditNote,
    ) -> impl Future<Output = Result<CreditNote, CoreError>> + Send;

    /// Transactional: mark a credit note as voided and reverse its amount on the parent
    /// invoice (`amount_refunded -= amount`, `amount_remaining += amount`). Idempotent:
    /// if the credit note is already voided, the existing row is returned. Writes an
    /// `invoice_history` row with `event_type = credit_note_voided`.
    fn void_credit_note_by_external_id(
        &self,
        realm_id: &str,
        external_credit_note_id: &str,
    ) -> impl Future<Output = Result<CreditNote, CoreError>> + Send;
}

impl<T: CreditNoteRepository> CreditNoteRepository for Arc<T> {
    fn find_by_invoice_id(
        &self,
        realm_id: &str,
        invoice_id: Uuid,
    ) -> impl Future<Output = Result<Vec<CreditNote>, CoreError>> + Send {
        (**self).find_by_invoice_id(realm_id, invoice_id)
    }

    fn find_by_external_id(
        &self,
        external_credit_note_id: &str,
    ) -> impl Future<Output = Result<Option<CreditNote>, CoreError>> + Send {
        (**self).find_by_external_id(external_credit_note_id)
    }

    fn create_credit_note_and_update_invoice(
        &self,
        input: NewCreditNote,
    ) -> impl Future<Output = Result<CreditNote, CoreError>> + Send {
        (**self).create_credit_note_and_update_invoice(input)
    }

    fn void_credit_note_by_external_id(
        &self,
        realm_id: &str,
        external_credit_note_id: &str,
    ) -> impl Future<Output = Result<CreditNote, CoreError>> + Send {
        (**self).void_credit_note_by_external_id(realm_id, external_credit_note_id)
    }
}
