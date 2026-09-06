use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;

/// Represents a NUMERIC(12,4) value as a string to avoid external decimal dependencies
/// in the domain layer. The string must contain a valid decimal representation.
pub type DecimalStr = String;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvoiceStatus {
    Draft,
    Issued,
    Paid,
    Void,
    Overdue,
}

impl InvoiceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Issued => "issued",
            Self::Paid => "paid",
            Self::Void => "void",
            Self::Overdue => "overdue",
        }
    }

    /// Whether this status is a terminal state (no further transitions allowed).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Paid | Self::Void)
    }
}

impl std::str::FromStr for InvoiceStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(Self::Draft),
            "issued" => Ok(Self::Issued),
            "paid" => Ok(Self::Paid),
            "void" => Ok(Self::Void),
            "overdue" => Ok(Self::Overdue),
            _ => Err(CoreError::BadRequest(format!(
                "Invalid invoice status: {}",
                s
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceSource {
    AdminManual,
    UserApplication,
    ExternalSync,
}

impl InvoiceSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AdminManual => "admin_manual",
            Self::UserApplication => "user_application",
            Self::ExternalSync => "external_sync",
        }
    }
}

impl std::str::FromStr for InvoiceSource {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin_manual" => Ok(Self::AdminManual),
            "user_application" => Ok(Self::UserApplication),
            "external_sync" => Ok(Self::ExternalSync),
            _ => Err(CoreError::BadRequest(format!(
                "Invalid invoice source: {}",
                s
            ))),
        }
    }
}

/// Invoice source provider — distinguishes self-managed from externally-synced invoices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvoiceProvider {
    Manual,
    Stripe,
    Creem,
    Wechat,
}

impl InvoiceProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Stripe => "stripe",
            Self::Creem => "creem",
            Self::Wechat => "wechat",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "manual" => Some(Self::Manual),
            "stripe" => Some(Self::Stripe),
            "creem" => Some(Self::Creem),
            "wechat" => Some(Self::Wechat),
            _ => None,
        }
    }
}

impl std::str::FromStr for InvoiceProvider {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str_opt(s)
            .ok_or_else(|| CoreError::BadRequest(format!("Invalid invoice provider: {}", s)))
    }
}

/// Mode for discount / tax / shipping adjustments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdjustmentMode {
    Fixed,
    Percent,
}

impl AdjustmentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Percent => "percent",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "fixed" => Some(Self::Fixed),
            "percent" => Some(Self::Percent),
            _ => None,
        }
    }
}

/// Who performed a history event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorType {
    User,
    System,
}

impl ActorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
        }
    }
}

/// Invoice history event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvoiceEventType {
    Created,
    Updated,
    Issued,
    Paid,
    Voided,
    Overdue,
    /// Credit note created against this invoice (refund applied).
    CreditNoteCreated,
    /// Credit note on this invoice was voided (refund reversed).
    CreditNoteVoided,
}

impl InvoiceEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Issued => "issued",
            Self::Paid => "paid",
            Self::Voided => "voided",
            Self::Overdue => "overdue",
            Self::CreditNoteCreated => "credit_note_created",
            Self::CreditNoteVoided => "credit_note_voided",
        }
    }
}

// ---------------------------------------------------------------------------
// Core entities
// ---------------------------------------------------------------------------

/// Seller configuration at realm level. Auto-fills seller fields on invoice creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceSellerConfig {
    pub realm_id: String,
    pub seller_name: String,
    pub seller_address: String,
    pub seller_email: Option<String>,
    pub seller_phone: Option<String>,
    pub seller_tax_id: String,
    pub default_payment_terms: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Invoice entity — persisted in the `invoice` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub id: Uuid,
    pub realm_id: String,
    pub invoice_number: String,
    pub source: InvoiceSource,
    pub account_id: Option<Uuid>,
    pub applicant_user_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub payment_attempt_id: Option<Uuid>,
    pub status: InvoiceStatus,
    pub currency: String,

    // Provider fields
    pub provider: InvoiceProvider,
    pub payment_provider: Option<String>,
    pub external_invoice_id: Option<String>,
    pub external_order_id: Option<String>,
    pub external_status: Option<String>,
    pub external_hosted_url: Option<String>,
    pub external_pdf_url: Option<String>,
    pub external_payload: Option<serde_json::Value>,
    pub tax_details: Option<serde_json::Value>,

    // Dates
    pub issue_date: Option<chrono::NaiveDate>,
    pub due_date: Option<chrono::NaiveDate>,
    pub issued_at: Option<DateTime<Utc>>,
    pub paid_at: Option<DateTime<Utc>>,
    pub voided_at: Option<DateTime<Utc>>,

    // Monetary amounts (smallest currency unit)
    pub subtotal: i64,
    pub discount_amount: i64,
    pub tax_amount: i64,
    pub shipping_amount: i64,
    pub total: i64,

    // Cached refund aggregates (smallest currency unit).
    pub amount_refunded: i64,
    pub amount_remaining: i64,

    // Adjustment mode + raw input value
    pub discount_mode: Option<AdjustmentMode>,
    pub discount_value: Option<DecimalStr>,
    pub tax_mode: Option<AdjustmentMode>,
    pub tax_value: Option<DecimalStr>,
    pub shipping_mode: Option<AdjustmentMode>,
    pub shipping_value: Option<DecimalStr>,

    // Buyer
    pub billing_name: Option<String>,
    pub billing_address: Option<String>,
    pub billing_email: Option<String>,
    pub billing_phone: Option<String>,
    pub billing_tax_id: Option<String>,

    // Seller snapshot
    pub seller_name: Option<String>,
    pub seller_address: Option<String>,
    pub seller_email: Option<String>,
    pub seller_phone: Option<String>,
    pub seller_tax_id: Option<String>,

    // Extra
    pub notes: Option<String>,
    pub payment_terms: Option<String>,
    pub void_reason: Option<String>,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A single line item within an invoice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceLineItem {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub sort_order: i32,
    pub name: String,
    pub description: Option<String>,
    pub quantity: DecimalStr,
    pub unit_price: i64,
    pub subtotal: i64,
}

/// Invoice history / audit event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceHistory {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub event_type: InvoiceEventType,
    pub actor_user_id: Option<Uuid>,
    pub actor_type: ActorType,
    pub changes: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Invoice number counter row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceNumberCounter {
    pub realm_id: String,
    pub year: i32,
    pub next_seq: i64,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Composite view types
// ---------------------------------------------------------------------------

/// Full invoice detail including line items and history, used for detail views and PDF generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceDetail {
    pub invoice: Invoice,
    pub line_items: Vec<InvoiceLineItem>,
    pub history: Vec<InvoiceHistory>,
}

/// Summary row for list views (no line items, no history).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceSummary {
    pub id: Uuid,
    pub realm_id: String,
    pub invoice_number: String,
    pub source: InvoiceSource,
    pub account_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub payment_attempt_id: Option<Uuid>,
    pub status: InvoiceStatus,
    pub currency: String,
    pub total: i64,
    pub amount_refunded: i64,
    pub billing_name: Option<String>,
    pub due_date: Option<chrono::NaiveDate>,
    pub created_at: DateTime<Utc>,

    // Provider fields
    pub provider: InvoiceProvider,
    pub payment_provider: Option<String>,
    pub external_invoice_id: Option<String>,
    pub external_hosted_url: Option<String>,
    pub external_pdf_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Input / command types
// ---------------------------------------------------------------------------

/// Input for creating a new invoice (draft).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewInvoice {
    pub realm_id: String,
    pub source: InvoiceSource,
    pub account_id: Uuid,
    pub applicant_user_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub payment_attempt_id: Option<Uuid>,
    pub currency: String,

    pub line_items: Vec<NewLineItem>,

    /// The user performing the create action (for audit history).
    pub actor_user_id: Option<Uuid>,

    // Buyer
    pub billing_name: String,
    pub billing_address: String,
    pub billing_email: Option<String>,
    pub billing_phone: Option<String>,
    pub billing_tax_id: String,

    // Seller snapshot (caller should populate from seller config)
    pub seller_name: String,
    pub seller_address: String,
    pub seller_email: Option<String>,
    pub seller_phone: Option<String>,
    pub seller_tax_id: String,

    // Adjustment inputs
    pub discount_mode: Option<AdjustmentMode>,
    pub discount_value: Option<DecimalStr>,
    pub tax_mode: Option<AdjustmentMode>,
    pub tax_value: Option<DecimalStr>,
    pub shipping_mode: Option<AdjustmentMode>,
    pub shipping_value: Option<DecimalStr>,

    pub due_date: chrono::NaiveDate,
    pub payment_terms: Option<String>,
    pub notes: Option<String>,
}

/// Input for a single line item when creating/updating an invoice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewLineItem {
    pub name: String,
    pub description: Option<String>,
    pub quantity: DecimalStr,
    pub unit_price: i64,
}

/// Input for updating an existing draft invoice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInvoiceDraft {
    pub realm_id: String,
    pub invoice_id: Uuid,

    /// The user performing the update action (for audit history).
    pub actor_user_id: Option<Uuid>,

    // Optional fields — only provided fields are updated.
    pub billing_name: Option<String>,
    pub billing_address: Option<String>,
    pub billing_email: Option<String>,
    pub billing_phone: Option<String>,
    pub billing_tax_id: Option<String>,

    pub seller_name: Option<String>,
    pub seller_address: Option<String>,
    pub seller_email: Option<String>,
    pub seller_phone: Option<String>,
    pub seller_tax_id: Option<String>,

    pub line_items: Option<Vec<NewLineItem>>,

    pub discount_mode: Option<AdjustmentMode>,
    pub discount_value: Option<DecimalStr>,
    pub tax_mode: Option<AdjustmentMode>,
    pub tax_value: Option<DecimalStr>,
    pub shipping_mode: Option<AdjustmentMode>,
    pub shipping_value: Option<DecimalStr>,

    pub due_date: Option<chrono::NaiveDate>,
    pub payment_terms: Option<String>,
    pub notes: Option<String>,
}

/// Command to transition invoice status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceStatusTransition {
    pub realm_id: String,
    pub invoice_id: Uuid,
    pub target_status: InvoiceStatus,
    pub actor_user_id: Option<Uuid>,
    pub actor_type: ActorType,
    /// Reason required for void transition.
    pub void_reason: Option<String>,
    /// Issue date to set when transitioning to Issued. If None, repository defaults to today.
    pub issue_date: Option<chrono::NaiveDate>,
    /// Paid timestamp to set when transitioning to Paid. If None, repository defaults to now.
    pub paid_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Attribution-based filter for the admin invoice list.
///
/// `Missing` selects externally-synced invoices that have NO local attribution:
/// `provider != 'manual' AND subscription_id IS NULL AND payment_attempt_id IS NULL`.
/// These are the rows an admin needs to investigate (webhook attribution gap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttributionFilter {
    Missing,
}

/// Filters for listing invoices.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InvoiceListFilters {
    pub status: Option<InvoiceStatus>,
    pub source: Option<InvoiceSource>,
    pub provider: Option<InvoiceProvider>,
    /// When true, restrict the list to externally-synced invoices
    /// (`provider <> 'manual'`). Set by the read path under invoice policy
    /// `none` (PRD invoice.md 行为矩阵 "发票列表"); not exposed as a
    /// request-side query param.
    #[serde(default)]
    pub external_only: bool,
    pub search: Option<String>,
    pub date_from: Option<chrono::NaiveDate>,
    pub date_to: Option<chrono::NaiveDate>,
    /// When `Some(Missing)`, restrict to externally-synced invoices lacking
    /// both `subscription_id` and `payment_attempt_id`.
    #[serde(default)]
    pub attribution: Option<AttributionFilter>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

/// Paginated result wrapper for invoice list queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedInvoices<T> {
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
    pub data: Vec<T>,
}

/// Input data for upserting an externally-synced invoice (e.g. from Stripe webhook or Creem callback).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalInvoiceData {
    pub realm_id: String,
    pub provider: InvoiceProvider,
    pub payment_provider: Option<String>,
    pub external_invoice_id: Option<String>,
    pub external_order_id: Option<String>,
    pub external_status: Option<String>,
    pub external_hosted_url: Option<String>,
    pub external_pdf_url: Option<String>,
    pub external_payload: Option<serde_json::Value>,
    pub tax_details: Option<serde_json::Value>,
    pub account_id: Option<Uuid>,
    /// Buyer attribution: the user who applied for / drove this purchase.
    /// COALESCE-preserved on upsert so the first resolver wins.
    pub applicant_user_id: Option<Uuid>,
    /// Buyer snapshot filled from the provider payload (e.g. Stripe
    /// `customer_name` / `customer_email`). All COALESCE-preserved.
    pub billing_name: Option<String>,
    pub billing_email: Option<String>,
    pub billing_phone: Option<String>,
    pub billing_address: Option<String>,
    pub currency: String,
    pub total: i64,
    pub status: InvoiceStatus,
    /// Local attribution: which subscription this external invoice belongs to.
    /// Real attribution is filled by webhook handlers; passing None preserves
    /// prior behavior via upsert COALESCE.
    pub subscription_id: Option<Uuid>,
    /// Local attribution: which payment attempt this external invoice belongs to.
    /// Real attribution is filled by webhook handlers; passing None preserves
    /// prior behavior via upsert COALESCE.
    pub payment_attempt_id: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// Traits (ports)
// ---------------------------------------------------------------------------

/// Repository for invoice persistence operations.
#[allow(clippy::manual_async_fn)]
pub trait InvoiceRepository: Send + Sync {
    fn create_invoice(
        &self,
        input: NewInvoice,
    ) -> impl Future<Output = Result<Invoice, CoreError>> + Send;

    fn update_draft(
        &self,
        input: UpdateInvoiceDraft,
    ) -> impl Future<Output = Result<Invoice, CoreError>> + Send;

    fn find_with_items(
        &self,
        realm_id: &str,
        invoice_id: Uuid,
    ) -> impl Future<Output = Result<Option<InvoiceDetail>, CoreError>> + Send;

    fn list_admin(
        &self,
        realm_id: &str,
        filters: InvoiceListFilters,
    ) -> impl Future<Output = Result<PaginatedInvoices<InvoiceSummary>, CoreError>> + Send;

    fn list_user(
        &self,
        realm_id: &str,
        user_id: Uuid,
        filters: InvoiceListFilters,
    ) -> impl Future<Output = Result<PaginatedInvoices<InvoiceSummary>, CoreError>> + Send;

    fn transition_status(
        &self,
        input: InvoiceStatusTransition,
    ) -> impl Future<Output = Result<Invoice, CoreError>> + Send;

    fn find_seller_config(
        &self,
        realm_id: &str,
    ) -> impl Future<Output = Result<Option<InvoiceSellerConfig>, CoreError>> + Send;

    fn upsert_seller_config(
        &self,
        config: InvoiceSellerConfig,
    ) -> impl Future<Output = Result<InvoiceSellerConfig, CoreError>> + Send;

    fn list_overdue_candidates(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Invoice>, CoreError>> + Send;

    /// Atomically reserve and return the next invoice number for a given realm+year.
    /// Uses SELECT FOR UPDATE row lock internally.
    fn next_invoice_number(
        &self,
        realm_id: &str,
        year: i32,
    ) -> impl Future<Output = Result<String, CoreError>> + Send;

    /// Upsert an externally-synced invoice. Matches on (realm_id, external_invoice_id) or
    /// (realm_id, external_order_id). Creates if not found, updates if exists.
    fn upsert_external_invoice(
        &self,
        data: ExternalInvoiceData,
    ) -> impl Future<Output = Result<Invoice, CoreError>> + Send;

    /// Look up an invoice by its external (provider) ID within a realm. Used by webhook
    /// handlers (e.g. Stripe `credit_note.created`) to resolve the local invoice.
    fn find_by_external_invoice_id(
        &self,
        realm_id: &str,
        external_invoice_id: &str,
    ) -> impl Future<Output = Result<Option<Invoice>, CoreError>> + Send;
}

impl<T: InvoiceRepository> InvoiceRepository for Arc<T> {
    fn create_invoice(
        &self,
        input: NewInvoice,
    ) -> impl Future<Output = Result<Invoice, CoreError>> + Send {
        (**self).create_invoice(input)
    }

    fn update_draft(
        &self,
        input: UpdateInvoiceDraft,
    ) -> impl Future<Output = Result<Invoice, CoreError>> + Send {
        (**self).update_draft(input)
    }

    fn find_with_items(
        &self,
        realm_id: &str,
        invoice_id: Uuid,
    ) -> impl Future<Output = Result<Option<InvoiceDetail>, CoreError>> + Send {
        (**self).find_with_items(realm_id, invoice_id)
    }

    fn list_admin(
        &self,
        realm_id: &str,
        filters: InvoiceListFilters,
    ) -> impl Future<Output = Result<PaginatedInvoices<InvoiceSummary>, CoreError>> + Send {
        (**self).list_admin(realm_id, filters)
    }

    fn list_user(
        &self,
        realm_id: &str,
        user_id: Uuid,
        filters: InvoiceListFilters,
    ) -> impl Future<Output = Result<PaginatedInvoices<InvoiceSummary>, CoreError>> + Send {
        (**self).list_user(realm_id, user_id, filters)
    }

    fn transition_status(
        &self,
        input: InvoiceStatusTransition,
    ) -> impl Future<Output = Result<Invoice, CoreError>> + Send {
        (**self).transition_status(input)
    }

    fn find_seller_config(
        &self,
        realm_id: &str,
    ) -> impl Future<Output = Result<Option<InvoiceSellerConfig>, CoreError>> + Send {
        (**self).find_seller_config(realm_id)
    }

    fn upsert_seller_config(
        &self,
        config: InvoiceSellerConfig,
    ) -> impl Future<Output = Result<InvoiceSellerConfig, CoreError>> + Send {
        (**self).upsert_seller_config(config)
    }

    fn list_overdue_candidates(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Invoice>, CoreError>> + Send {
        (**self).list_overdue_candidates(now, limit)
    }

    fn next_invoice_number(
        &self,
        realm_id: &str,
        year: i32,
    ) -> impl Future<Output = Result<String, CoreError>> + Send {
        (**self).next_invoice_number(realm_id, year)
    }

    fn upsert_external_invoice(
        &self,
        data: ExternalInvoiceData,
    ) -> impl Future<Output = Result<Invoice, CoreError>> + Send {
        (**self).upsert_external_invoice(data)
    }

    fn find_by_external_invoice_id(
        &self,
        realm_id: &str,
        external_invoice_id: &str,
    ) -> impl Future<Output = Result<Option<Invoice>, CoreError>> + Send {
        (**self).find_by_external_invoice_id(realm_id, external_invoice_id)
    }
}

/// PDF generator for invoice documents.
#[allow(clippy::manual_async_fn)]
pub trait InvoicePdfGenerator: Send + Sync {
    fn generate(
        &self,
        invoice: &InvoiceDetail,
    ) -> impl Future<Output = Result<Vec<u8>, CoreError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoice_status_terminal_states() {
        assert!(InvoiceStatus::Paid.is_terminal());
        assert!(InvoiceStatus::Void.is_terminal());
        assert!(!InvoiceStatus::Draft.is_terminal());
        assert!(!InvoiceStatus::Issued.is_terminal());
        assert!(!InvoiceStatus::Overdue.is_terminal());
    }
}
