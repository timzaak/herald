use chrono::Utc;
use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;

use super::invoice::{
    ActorType, AdjustmentMode, Invoice, InvoiceProvider, InvoiceRepository, InvoiceStatus,
    InvoiceStatusTransition, NewInvoice, NewLineItem, UpdateInvoiceDraft,
};

// ---------------------------------------------------------------------------
// Overdue marking result
// ---------------------------------------------------------------------------

/// Result summary of an overdue invoice marking run.
#[derive(Debug)]
pub struct OverdueMarkResult {
    pub candidates: usize,
    pub marked: usize,
    pub errors: usize,
}

// ---------------------------------------------------------------------------
// Status machine (pure validation)
// ---------------------------------------------------------------------------

/// Validate whether a status transition is allowed and return an error if not.
///
/// Rules:
/// - `draft -> issued`: requires at least 1 line item, total > 0
/// - `draft -> void`: allowed
/// - `issued -> paid`: allowed
/// - `issued -> void`: allowed (with reason)
/// - `issued -> overdue`: system-only, requires due_date < today
/// - `overdue -> paid`: allowed
/// - `overdue -> void`: allowed
/// - `paid` and `void` are terminal states — reject any transition
pub fn validate_status_transition(
    current: InvoiceStatus,
    target: InvoiceStatus,
    line_item_count: usize,
    total: i64,
    actor_type: ActorType,
    has_due_date_passed: bool,
    void_reason: Option<&str>,
) -> Result<(), CoreError> {
    if current.is_terminal() {
        return Err(CoreError::Conflict(format!(
            "Invoice is in terminal state '{}' and cannot be transitioned",
            current.as_str()
        )));
    }

    match (current, target) {
        (InvoiceStatus::Draft, InvoiceStatus::Issued) => {
            if line_item_count == 0 {
                return Err(CoreError::BadRequest(
                    "Cannot issue an invoice without line items".to_string(),
                ));
            }
            if total <= 0 {
                return Err(CoreError::BadRequest(
                    "Cannot issue an invoice with total <= 0".to_string(),
                ));
            }
            Ok(())
        }
        (InvoiceStatus::Draft, InvoiceStatus::Void) => Ok(()),
        (InvoiceStatus::Issued, InvoiceStatus::Paid) => Ok(()),
        (InvoiceStatus::Issued, InvoiceStatus::Void) => {
            if void_reason.is_none() || void_reason.unwrap().is_empty() {
                return Err(CoreError::BadRequest(
                    "Void reason is required when voiding an issued invoice".to_string(),
                ));
            }
            Ok(())
        }
        (InvoiceStatus::Issued, InvoiceStatus::Overdue) => {
            if actor_type != ActorType::System {
                return Err(CoreError::Forbidden(
                    "Only the system can mark an invoice as overdue".to_string(),
                ));
            }
            if !has_due_date_passed {
                return Err(CoreError::BadRequest(
                    "Invoice due date has not passed yet".to_string(),
                ));
            }
            Ok(())
        }
        (InvoiceStatus::Overdue, InvoiceStatus::Paid) => Ok(()),
        (InvoiceStatus::Overdue, InvoiceStatus::Void) => Ok(()),
        _ => Err(CoreError::Conflict(format!(
            "Invalid status transition from '{}' to '{}'",
            current.as_str(),
            target.as_str()
        ))),
    }
}

// ---------------------------------------------------------------------------
// Amount calculation (pure functions)
// ---------------------------------------------------------------------------

/// Calculate a single line item subtotal: `round(quantity * unit_price)`.
///
/// `quantity` is a decimal string (e.g. "1.5", "2.000").
/// `unit_price` is in the smallest currency unit (e.g. cents).
/// Returns the subtotal in the smallest currency unit.
pub fn calculate_line_item_subtotal(quantity: &str, unit_price: i64) -> Result<i64, CoreError> {
    let qty: f64 = quantity
        .parse()
        .map_err(|_| CoreError::BadRequest(format!("Invalid quantity value: {}", quantity)))?;
    let result = qty * unit_price as f64;
    Ok(result.round() as i64)
}

/// Calculate all derived amounts for an invoice from line items and adjustment inputs.
///
/// Returns `(subtotal, discount_amount, tax_amount, shipping_amount, total)`.
/// All values are in the smallest currency unit.
pub fn calculate_invoice_amounts(
    line_items: &[NewLineItem],
    discount_mode: Option<AdjustmentMode>,
    discount_value: Option<&str>,
    tax_mode: Option<AdjustmentMode>,
    tax_value: Option<&str>,
    shipping_mode: Option<AdjustmentMode>,
    shipping_value: Option<&str>,
) -> Result<InvoiceAmounts, CoreError> {
    // Subtotal = sum of line item subtotals
    let mut subtotal: i64 = 0;
    for item in line_items {
        let item_subtotal = calculate_line_item_subtotal(&item.quantity, item.unit_price)?;
        subtotal += item_subtotal;
    }

    let discount_amount = calculate_adjustment(discount_mode, discount_value, subtotal)?;
    let tax_amount = calculate_adjustment(tax_mode, tax_value, subtotal)?;
    let shipping_amount = calculate_adjustment(shipping_mode, shipping_value, subtotal)?;

    let total = subtotal - discount_amount + tax_amount + shipping_amount;

    Ok(InvoiceAmounts {
        subtotal,
        discount_amount,
        tax_amount,
        shipping_amount,
        total,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceAmounts {
    pub subtotal: i64,
    pub discount_amount: i64,
    pub tax_amount: i64,
    pub shipping_amount: i64,
    pub total: i64,
}

/// Calculate an adjustment amount based on mode and value.
///
/// - `Fixed` mode: the value is the amount directly (in smallest currency unit).
/// - `Percent` mode: the value is a percentage of the subtotal, rounded to the
///   smallest currency unit.
fn calculate_adjustment(
    mode: Option<AdjustmentMode>,
    value: Option<&str>,
    subtotal: i64,
) -> Result<i64, CoreError> {
    let (mode, value) = match (mode, value) {
        (Some(m), Some(v)) => (m, v),
        _ => return Ok(0),
    };

    match mode {
        AdjustmentMode::Fixed => {
            let amount: i64 = value
                .parse()
                .map_err(|_| CoreError::BadRequest(format!("Invalid fixed value: {}", value)))?;
            Ok(amount)
        }
        AdjustmentMode::Percent => {
            let pct: f64 = value
                .parse()
                .map_err(|_| CoreError::BadRequest(format!("Invalid percent value: {}", value)))?;
            let result = (subtotal as f64 * pct / 100.0).round();
            Ok(result as i64)
        }
    }
}

// ---------------------------------------------------------------------------
// Invoice number formatting
// ---------------------------------------------------------------------------

/// Format an invoice number as `INV-{YEAR}-{SEQ:04}`.
pub fn format_invoice_number(year: i32, seq: i64) -> String {
    format!("INV-{}-{:04}", year, seq)
}

// ---------------------------------------------------------------------------
// Provider guards (pure validation)
// ---------------------------------------------------------------------------

/// Reject write operations on externally-managed invoices.
///
/// Returns `Ok(())` for Manual provider (self-managed invoices).
/// Returns `Forbidden` for any external provider (Stripe, Creem, etc.).
pub fn validate_external_invoice_readonly(provider: InvoiceProvider) -> Result<(), CoreError> {
    if provider != InvoiceProvider::Manual {
        return Err(CoreError::Forbidden(
            "This invoice is managed by the payment provider".to_string(),
        ));
    }
    Ok(())
}

/// Reject manual invoice creation for Merchant-of-Record (MoR) transactions.
///
/// Creem, Apple (App Store) and Google (Google Play) act as Merchant of
/// Record, so Herald must not create a competing invoice — regardless of the
/// realm's invoice_policy (support-iap PRD §4.1: invoice_policy 不影响该约束).
pub fn validate_not_mor_provider(payment_provider: Option<&str>) -> Result<(), CoreError> {
    if matches!(
        payment_provider,
        Some("creem") | Some("apple") | Some("google")
    ) {
        return Err(CoreError::BadRequest(format!(
            "{} transactions are managed by the platform as Merchant of Record",
            payment_provider.unwrap_or_default()
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Invoice policy config (parse from JSON config_value)
// ---------------------------------------------------------------------------

/// Parsed invoice policy configuration from realm_config.
#[derive(Debug, Clone)]
pub struct InvoicePolicyConfig {
    pub policy: String,
    pub provider_capabilities: serde_json::Value,
}

/// Parse invoice policy from a JSON config_value string.
///
/// Expected format: `{"policy":"provider_first","provider_capabilities":{...}}`
pub fn parse_invoice_policy_config(config_value: &str) -> Result<InvoicePolicyConfig, CoreError> {
    let parsed: serde_json::Value = serde_json::from_str(config_value)
        .map_err(|e| CoreError::BadRequest(format!("Invalid invoice policy config JSON: {}", e)))?;

    let policy = parsed
        .get("policy")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            CoreError::BadRequest("Invoice policy config missing 'policy' field".to_string())
        })?
        .to_string();

    let provider_capabilities = parsed
        .get("provider_capabilities")
        .cloned()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    Ok(InvoicePolicyConfig {
        policy,
        provider_capabilities,
    })
}

/// Check whether the current invoice policy allows creating invoices.
///
/// Returns `Ok(())` for "provider_first" and "manual_only".
/// Returns `Forbidden` for "none".
pub fn validate_invoice_policy_allows_creation(
    config: &InvoicePolicyConfig,
) -> Result<(), CoreError> {
    if config.policy == "none" {
        return Err(CoreError::Forbidden(
            "Invoice creation is disabled by policy".to_string(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Stripe status mapping (pure function)
// ---------------------------------------------------------------------------

/// Map a Stripe invoice status string to the Herald `InvoiceStatus` enum.
///
/// Mapping:
/// - "draft" -> Draft
/// - "open" -> Issued
/// - "paid" -> Paid
/// - "void" / "voided" -> Void
/// - "uncollectible" -> Void (simplified; original status preserved in external_status)
pub fn map_stripe_invoice_status(stripe_status: &str) -> Result<InvoiceStatus, CoreError> {
    match stripe_status {
        "draft" => Ok(InvoiceStatus::Draft),
        "open" => Ok(InvoiceStatus::Issued),
        "paid" => Ok(InvoiceStatus::Paid),
        "void" | "voided" => Ok(InvoiceStatus::Void),
        "uncollectible" => Ok(InvoiceStatus::Void),
        _ => Err(CoreError::BadRequest(format!(
            "Unknown Stripe invoice status: {}",
            stripe_status
        ))),
    }
}

// ---------------------------------------------------------------------------
// InvoiceService
// ---------------------------------------------------------------------------

/// Domain service for invoice operations.
///
/// Orchestrates status transitions, amount calculation, and invoice number
/// generation. Accepts a repository through generic parameter for dependency
/// inversion.
pub struct InvoiceService<R> {
    repo: R,
}

impl<R: InvoiceRepository> InvoiceService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    /// Create a draft invoice (admin manual creation).
    ///
    /// Generates an invoice number via repository counter, then delegates
    /// persistence to repository. The repository is responsible for storing
    /// line items and recording history.
    pub async fn create_invoice(&self, input: NewInvoice) -> Result<Invoice, CoreError> {
        // The repository handles invoice number generation, line items, and history.
        self.repo.create_invoice(input).await
    }

    /// Update a draft invoice.
    ///
    /// Validates that the invoice is still in draft status, then delegates to
    /// repository for update. Amounts are recalculated server-side.
    pub async fn update_draft(&self, input: UpdateInvoiceDraft) -> Result<Invoice, CoreError> {
        // Verify the invoice exists and is in draft status
        let detail = self
            .repo
            .find_with_items(&input.realm_id, input.invoice_id)
            .await?
            .ok_or(CoreError::NotFound)?;

        if detail.invoice.status != InvoiceStatus::Draft {
            return Err(CoreError::Conflict(format!(
                "Cannot update invoice in '{}' status, only draft invoices can be updated",
                detail.invoice.status.as_str()
            )));
        }

        let invoice = self.repo.update_draft(input).await?;
        Ok(invoice)
    }

    /// Issue a draft invoice (transition draft -> issued).
    ///
    /// Validates that the invoice has at least 1 line item and total > 0,
    /// then transitions status via repository.
    pub async fn issue(
        &self,
        realm_id: &str,
        invoice_id: Uuid,
        actor_user_id: Option<Uuid>,
        actor_type: ActorType,
    ) -> Result<Invoice, CoreError> {
        let detail = self
            .repo
            .find_with_items(realm_id, invoice_id)
            .await?
            .ok_or(CoreError::NotFound)?;

        validate_status_transition(
            detail.invoice.status,
            InvoiceStatus::Issued,
            detail.line_items.len(),
            detail.invoice.total,
            actor_type,
            false, // not relevant for draft->issued
            None,
        )?;

        self.repo
            .transition_status(InvoiceStatusTransition {
                realm_id: realm_id.to_string(),
                invoice_id,
                target_status: InvoiceStatus::Issued,
                actor_user_id,
                actor_type,
                void_reason: None,
                issue_date: None,
                paid_at: None,
            })
            .await
    }

    /// Void an invoice (transition to void status).
    ///
    /// Validates the transition is allowed. For issued invoices, a void_reason
    /// is required.
    pub async fn void(
        &self,
        realm_id: &str,
        invoice_id: Uuid,
        actor_user_id: Option<Uuid>,
        actor_type: ActorType,
        void_reason: Option<String>,
    ) -> Result<Invoice, CoreError> {
        let detail = self
            .repo
            .find_with_items(realm_id, invoice_id)
            .await?
            .ok_or(CoreError::NotFound)?;

        validate_status_transition(
            detail.invoice.status,
            InvoiceStatus::Void,
            detail.line_items.len(),
            detail.invoice.total,
            actor_type,
            false,
            void_reason.as_deref(),
        )?;

        self.repo
            .transition_status(InvoiceStatusTransition {
                realm_id: realm_id.to_string(),
                invoice_id,
                target_status: InvoiceStatus::Void,
                actor_user_id,
                actor_type,
                void_reason,
                issue_date: None,
                paid_at: None,
            })
            .await
    }

    /// Mark an invoice as paid (transition issued/overdue -> paid).
    pub async fn mark_paid(
        &self,
        realm_id: &str,
        invoice_id: Uuid,
        actor_user_id: Option<Uuid>,
        actor_type: ActorType,
    ) -> Result<Invoice, CoreError> {
        let detail = self
            .repo
            .find_with_items(realm_id, invoice_id)
            .await?
            .ok_or(CoreError::NotFound)?;

        validate_status_transition(
            detail.invoice.status,
            InvoiceStatus::Paid,
            detail.line_items.len(),
            detail.invoice.total,
            actor_type,
            false,
            None,
        )?;

        self.repo
            .transition_status(InvoiceStatusTransition {
                realm_id: realm_id.to_string(),
                invoice_id,
                target_status: InvoiceStatus::Paid,
                actor_user_id,
                actor_type,
                void_reason: None,
                issue_date: None,
                paid_at: None,
            })
            .await
    }

    /// Mark invoices as overdue by system (batch processing).
    ///
    /// Queries invoices with `status = 'issued'` and `due_date < now`,
    /// then transitions each to `overdue`.
    pub async fn mark_overdue_by_system(
        &self,
        now: chrono::DateTime<Utc>,
        limit: i64,
    ) -> Result<OverdueMarkResult, CoreError> {
        let candidates = self.repo.list_overdue_candidates(now, limit).await?;
        let total_candidates = candidates.len();
        let mut marked = 0usize;
        let mut errors = 0usize;

        for invoice in &candidates {
            let result = self
                .repo
                .transition_status(InvoiceStatusTransition {
                    realm_id: invoice.realm_id.clone(),
                    invoice_id: invoice.id,
                    target_status: InvoiceStatus::Overdue,
                    actor_user_id: None,
                    actor_type: ActorType::System,
                    void_reason: None,
                    issue_date: None,
                    paid_at: None,
                })
                .await;

            match result {
                Ok(_) => marked += 1,
                Err(_) => errors += 1,
            }
        }

        Ok(OverdueMarkResult {
            candidates: total_candidates,
            marked,
            errors,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Status machine tests
    // -------------------------------------------------------------------------

    #[test]
    fn draft_to_issued_valid() {
        let result = validate_status_transition(
            InvoiceStatus::Draft,
            InvoiceStatus::Issued,
            2,    // line items
            1000, // total
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn draft_to_issued_no_line_items_rejected() {
        let result = validate_status_transition(
            InvoiceStatus::Draft,
            InvoiceStatus::Issued,
            0,    // no line items
            1000, // total
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_err());
        match result {
            Err(CoreError::BadRequest(msg)) => {
                assert!(msg.contains("line items"));
            }
            _ => panic!("Expected BadRequest error"),
        }
    }

    #[test]
    fn draft_to_issued_zero_total_rejected() {
        let result = validate_status_transition(
            InvoiceStatus::Draft,
            InvoiceStatus::Issued,
            1, // has line items
            0, // but total is 0
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_err());
        match result {
            Err(CoreError::BadRequest(msg)) => {
                assert!(msg.contains("total"));
            }
            _ => panic!("Expected BadRequest error"),
        }
    }

    #[test]
    fn draft_to_issued_negative_total_rejected() {
        let result = validate_status_transition(
            InvoiceStatus::Draft,
            InvoiceStatus::Issued,
            1,
            -100,
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn draft_to_void_valid() {
        let result = validate_status_transition(
            InvoiceStatus::Draft,
            InvoiceStatus::Void,
            0,
            0,
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn issued_to_paid_valid() {
        let result = validate_status_transition(
            InvoiceStatus::Issued,
            InvoiceStatus::Paid,
            1,
            1000,
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn issued_to_void_with_reason_valid() {
        let result = validate_status_transition(
            InvoiceStatus::Issued,
            InvoiceStatus::Void,
            1,
            1000,
            ActorType::User,
            false,
            Some("customer request"),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn issued_to_void_without_reason_rejected() {
        let result = validate_status_transition(
            InvoiceStatus::Issued,
            InvoiceStatus::Void,
            1,
            1000,
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_err());
        match result {
            Err(CoreError::BadRequest(msg)) => {
                assert!(msg.contains("reason"));
            }
            _ => panic!("Expected BadRequest error"),
        }
    }

    #[test]
    fn issued_to_void_empty_reason_rejected() {
        let result = validate_status_transition(
            InvoiceStatus::Issued,
            InvoiceStatus::Void,
            1,
            1000,
            ActorType::User,
            false,
            Some(""),
        );
        assert!(result.is_err());
    }

    #[test]
    fn issued_to_overdue_system_allowed() {
        let result = validate_status_transition(
            InvoiceStatus::Issued,
            InvoiceStatus::Overdue,
            1,
            1000,
            ActorType::System,
            true, // due_date has passed
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn issued_to_overdue_user_rejected() {
        let result = validate_status_transition(
            InvoiceStatus::Issued,
            InvoiceStatus::Overdue,
            1,
            1000,
            ActorType::User, // not system
            true,
            None,
        );
        assert!(result.is_err());
        match result {
            Err(CoreError::Forbidden(msg)) => {
                assert!(msg.contains("system"));
            }
            _ => panic!("Expected Forbidden error"),
        }
    }

    #[test]
    fn issued_to_overdue_due_date_not_passed_rejected() {
        let result = validate_status_transition(
            InvoiceStatus::Issued,
            InvoiceStatus::Overdue,
            1,
            1000,
            ActorType::System,
            false, // due_date not passed
            None,
        );
        assert!(result.is_err());
        match result {
            Err(CoreError::BadRequest(msg)) => {
                assert!(msg.contains("due date"));
            }
            _ => panic!("Expected BadRequest error"),
        }
    }

    #[test]
    fn overdue_to_paid_valid() {
        let result = validate_status_transition(
            InvoiceStatus::Overdue,
            InvoiceStatus::Paid,
            1,
            1000,
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn overdue_to_void_valid() {
        let result = validate_status_transition(
            InvoiceStatus::Overdue,
            InvoiceStatus::Void,
            1,
            1000,
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn paid_is_terminal_rejects_any_transition() {
        for target in [
            InvoiceStatus::Draft,
            InvoiceStatus::Issued,
            InvoiceStatus::Paid,
            InvoiceStatus::Void,
            InvoiceStatus::Overdue,
        ] {
            let result = validate_status_transition(
                InvoiceStatus::Paid,
                target,
                1,
                1000,
                ActorType::User,
                false,
                None,
            );
            assert!(result.is_err(), "Paid -> {:?} should be rejected", target);
            match result {
                Err(CoreError::Conflict(msg)) => {
                    assert!(msg.contains("terminal"));
                }
                _ => panic!("Expected Conflict error for Paid -> {:?}", target),
            }
        }
    }

    #[test]
    fn void_is_terminal_rejects_any_transition() {
        for target in [
            InvoiceStatus::Draft,
            InvoiceStatus::Issued,
            InvoiceStatus::Paid,
            InvoiceStatus::Void,
            InvoiceStatus::Overdue,
        ] {
            let result = validate_status_transition(
                InvoiceStatus::Void,
                target,
                1,
                1000,
                ActorType::User,
                false,
                None,
            );
            assert!(result.is_err(), "Void -> {:?} should be rejected", target);
        }
    }

    #[test]
    fn invalid_transitions_rejected() {
        // draft -> paid is not a valid transition
        let result = validate_status_transition(
            InvoiceStatus::Draft,
            InvoiceStatus::Paid,
            1,
            1000,
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_err());
        match result {
            Err(CoreError::Conflict(msg)) => {
                assert!(msg.contains("Invalid status transition"));
            }
            _ => panic!("Expected Conflict error"),
        }

        // draft -> overdue is not valid
        let result = validate_status_transition(
            InvoiceStatus::Draft,
            InvoiceStatus::Overdue,
            1,
            1000,
            ActorType::System,
            true,
            None,
        );
        assert!(result.is_err());
    }

    // -------------------------------------------------------------------------
    // Amount calculation tests
    // -------------------------------------------------------------------------

    #[test]
    fn calculate_line_item_subtotal_integer_quantity() {
        let result = calculate_line_item_subtotal("3", 1000).unwrap();
        assert_eq!(result, 3000);
    }

    #[test]
    fn calculate_line_item_subtotal_decimal_quantity() {
        let result = calculate_line_item_subtotal("1.5", 1000).unwrap();
        assert_eq!(result, 1500);
    }

    #[test]
    fn calculate_line_item_subtotal_rounding_up() {
        // 2.667 * 1000 = 2667.0
        let result = calculate_line_item_subtotal("2.667", 1000).unwrap();
        assert_eq!(result, 2667);
    }

    #[test]
    fn calculate_line_item_subtotal_invalid_quantity() {
        let result = calculate_line_item_subtotal("abc", 1000);
        assert!(result.is_err());
    }

    #[test]
    fn calculate_invoice_amounts_basic() {
        let line_items = vec![
            NewLineItem {
                name: "Item A".to_string(),
                description: None,
                quantity: "2".to_string(),
                unit_price: 1000,
            },
            NewLineItem {
                name: "Item B".to_string(),
                description: None,
                quantity: "1".to_string(),
                unit_price: 2000,
            },
        ];

        let amounts =
            calculate_invoice_amounts(&line_items, None, None, None, None, None, None).unwrap();

        assert_eq!(amounts.subtotal, 4000); // 2*1000 + 1*2000
        assert_eq!(amounts.discount_amount, 0);
        assert_eq!(amounts.tax_amount, 0);
        assert_eq!(amounts.shipping_amount, 0);
        assert_eq!(amounts.total, 4000);
    }

    #[test]
    fn calculate_invoice_amounts_with_percent_discount() {
        let line_items = vec![NewLineItem {
            name: "Item A".to_string(),
            description: None,
            quantity: "1".to_string(),
            unit_price: 10000, // 100.00 in cents
        }];

        let amounts = calculate_invoice_amounts(
            &line_items,
            Some(AdjustmentMode::Percent),
            Some("10"), // 10%
            None,
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(amounts.subtotal, 10000);
        assert_eq!(amounts.discount_amount, 1000); // 10% of 10000 = 1000
        assert_eq!(amounts.total, 9000);
    }

    #[test]
    fn calculate_invoice_amounts_with_fixed_discount() {
        let line_items = vec![NewLineItem {
            name: "Item A".to_string(),
            description: None,
            quantity: "1".to_string(),
            unit_price: 10000,
        }];

        let amounts = calculate_invoice_amounts(
            &line_items,
            Some(AdjustmentMode::Fixed),
            Some("500"), // 5.00 in cents
            None,
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(amounts.subtotal, 10000);
        assert_eq!(amounts.discount_amount, 500);
        assert_eq!(amounts.total, 9500);
    }

    #[test]
    fn calculate_invoice_amounts_with_percent_tax() {
        let line_items = vec![NewLineItem {
            name: "Item A".to_string(),
            description: None,
            quantity: "1".to_string(),
            unit_price: 10000,
        }];

        let amounts = calculate_invoice_amounts(
            &line_items,
            None,
            None,
            Some(AdjustmentMode::Percent),
            Some("8.5"), // 8.5% tax
            None,
            None,
        )
        .unwrap();

        assert_eq!(amounts.subtotal, 10000);
        assert_eq!(amounts.tax_amount, 850); // 10000 * 8.5 / 100 = 850
        assert_eq!(amounts.total, 10850);
    }

    #[test]
    fn calculate_invoice_amounts_with_fixed_shipping() {
        let line_items = vec![NewLineItem {
            name: "Item A".to_string(),
            description: None,
            quantity: "1".to_string(),
            unit_price: 10000,
        }];

        let amounts = calculate_invoice_amounts(
            &line_items,
            None,
            None,
            None,
            None,
            Some(AdjustmentMode::Fixed),
            Some("300"), // 3.00 shipping
        )
        .unwrap();

        assert_eq!(amounts.subtotal, 10000);
        assert_eq!(amounts.shipping_amount, 300);
        assert_eq!(amounts.total, 10300);
    }

    #[test]
    fn calculate_invoice_amounts_full_scenario() {
        let line_items = vec![
            NewLineItem {
                name: "Pro Plan".to_string(),
                description: None,
                quantity: "1".to_string(),
                unit_price: 9900, // 99.00
            },
            NewLineItem {
                name: "Extra seats".to_string(),
                description: None,
                quantity: "2".to_string(),
                unit_price: 500, // 5.00 each
            },
        ];

        let amounts = calculate_invoice_amounts(
            &line_items,
            Some(AdjustmentMode::Percent),
            Some("5"), // 5% discount
            Some(AdjustmentMode::Percent),
            Some("10"), // 10% tax
            Some(AdjustmentMode::Fixed),
            Some("200"), // 2.00 shipping
        )
        .unwrap();

        // subtotal = 9900 + 1000 = 10900
        assert_eq!(amounts.subtotal, 10900);
        // discount = 10900 * 5 / 100 = 545
        assert_eq!(amounts.discount_amount, 545);
        // tax = 10900 * 10 / 100 = 1090
        assert_eq!(amounts.tax_amount, 1090);
        // shipping = 200
        assert_eq!(amounts.shipping_amount, 200);
        // total = 10900 - 545 + 1090 + 200 = 11645
        assert_eq!(amounts.total, 11645);
    }

    // -------------------------------------------------------------------------
    // Invoice number formatting tests
    // -------------------------------------------------------------------------

    #[test]
    fn format_invoice_number_basic() {
        assert_eq!(format_invoice_number(2026, 1), "INV-2026-0001");
    }

    #[test]
    fn format_invoice_number_large_seq() {
        assert_eq!(format_invoice_number(2026, 1234), "INV-2026-1234");
    }

    #[test]
    fn calculate_adjustment_invalid_fixed_value() {
        let result = calculate_adjustment(Some(AdjustmentMode::Fixed), Some("abc"), 1000);
        assert!(result.is_err());
    }

    #[test]
    fn calculate_adjustment_invalid_percent_value() {
        let result = calculate_adjustment(Some(AdjustmentMode::Percent), Some("xyz"), 1000);
        assert!(result.is_err());
    }

    #[test]
    fn calculate_adjustment_percent_rounding() {
        // 1000 * 33.333 / 100 = 333.33 -> rounds to 333
        let result =
            calculate_adjustment(Some(AdjustmentMode::Percent), Some("33.333"), 1000).unwrap();
        assert_eq!(result, 333);
    }

    #[test]
    fn calculate_adjustment_percent_rounding_up() {
        // 1000 * 66.667 / 100 = 666.67 -> rounds to 667
        let result =
            calculate_adjustment(Some(AdjustmentMode::Percent), Some("66.667"), 1000).unwrap();
        assert_eq!(result, 667);
    }

    // Fixed tax — verify AdjustmentMode::Fixed for tax adds directly
    #[test]
    fn calculate_invoice_amounts_with_fixed_tax() {
        let line_items = vec![NewLineItem {
            name: "Item A".to_string(),
            description: None,
            quantity: "1".to_string(),
            unit_price: 10000,
        }];

        let amounts = calculate_invoice_amounts(
            &line_items,
            None,
            None,
            Some(AdjustmentMode::Fixed),
            Some("750"), // 7.50 fixed tax
            None,
            None,
        )
        .unwrap();

        assert_eq!(amounts.subtotal, 10000);
        assert_eq!(amounts.tax_amount, 750);
        assert_eq!(amounts.total, 10750);
    }

    // total_cannot_be_negative — large discount can produce negative total,
    // and issue() must reject it via validate_status_transition(total <= 0).
    #[test]
    fn calculate_invoice_amounts_total_can_be_negative_and_issue_rejects() {
        let line_items = vec![NewLineItem {
            name: "Item A".to_string(),
            description: None,
            quantity: "1".to_string(),
            unit_price: 1000, // subtotal = 1000
        }];

        // discount 2000 > subtotal 1000 => total = -1000
        let amounts = calculate_invoice_amounts(
            &line_items,
            Some(AdjustmentMode::Fixed),
            Some("2000"),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(amounts.subtotal, 1000);
        assert_eq!(amounts.discount_amount, 2000);
        assert_eq!(amounts.total, -1000); // calculation allows negative

        // But issuing with a negative total must be rejected
        let result = validate_status_transition(
            InvoiceStatus::Draft,
            InvoiceStatus::Issued,
            1,
            -1000, // negative total
            ActorType::User,
            false,
            None,
        );
        assert!(result.is_err());
    }

    // -------------------------------------------------------------------------
    // Provider guard tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_validate_external_invoice_readonly_manual_ok() {
        let result = validate_external_invoice_readonly(InvoiceProvider::Manual);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_external_invoice_readonly_stripe_forbidden() {
        let result = validate_external_invoice_readonly(InvoiceProvider::Stripe);
        assert!(result.is_err());
        match result {
            Err(CoreError::Forbidden(msg)) => {
                assert!(msg.contains("payment provider"));
            }
            _ => panic!("Expected Forbidden error"),
        }
    }

    #[test]
    fn test_validate_external_invoice_readonly_creem_forbidden() {
        let result = validate_external_invoice_readonly(InvoiceProvider::Creem);
        assert!(result.is_err());
        match result {
            Err(CoreError::Forbidden(msg)) => {
                assert!(msg.contains("payment provider"));
            }
            _ => panic!("Expected Forbidden error"),
        }
    }

    #[test]
    fn test_validate_not_mor_provider_ok() {
        // None (no payment provider) should pass
        assert!(validate_not_mor_provider(None).is_ok());
        // Stripe should pass (not an MoR provider)
        assert!(validate_not_mor_provider(Some("stripe")).is_ok());
        // WeChat should pass (Herald-side merchant, manual fallback allowed)
        assert!(validate_not_mor_provider(Some("wechat")).is_ok());
        // Empty string should pass
        assert!(validate_not_mor_provider(Some("")).is_ok());
    }

    #[test]
    fn test_validate_not_mor_provider_mor_rejected() {
        // WHY: Creem, Apple App Store and Google Play are all Merchant of
        // Record — a Herald manual invoice would compete with the store's own
        // invoice/receipt, and invoice_policy must NOT override this
        // (support-iap PRD §4.1).
        for provider in ["creem", "apple", "google"] {
            let result = validate_not_mor_provider(Some(provider));
            assert!(result.is_err(), "{provider} must be rejected");
            match result {
                Err(CoreError::BadRequest(msg)) => {
                    assert!(msg.contains("Merchant of Record"));
                }
                _ => panic!("Expected BadRequest error"),
            }
        }
    }

    #[test]
    fn test_parse_invoice_policy_config_valid() {
        let json = r#"{"policy":"provider_first","provider_capabilities":{"stripe":{"external_invoice_enabled":true}}}"#;
        let config = parse_invoice_policy_config(json).unwrap();
        assert_eq!(config.policy, "provider_first");
        assert!(config.provider_capabilities.is_object());
    }

    #[test]
    fn test_parse_invoice_policy_config_invalid_json() {
        let result = parse_invoice_policy_config("not json");
        assert!(result.is_err());
        match result {
            Err(CoreError::BadRequest(msg)) => {
                assert!(msg.contains("Invalid invoice policy config JSON"));
            }
            _ => panic!("Expected BadRequest error"),
        }
    }

    #[test]
    fn test_validate_invoice_policy_allows_creation_ok() {
        // provider_first should pass
        let config_pf = InvoicePolicyConfig {
            policy: "provider_first".to_string(),
            provider_capabilities: serde_json::Value::Null,
        };
        assert!(validate_invoice_policy_allows_creation(&config_pf).is_ok());

        // manual_only should pass
        let config_mo = InvoicePolicyConfig {
            policy: "manual_only".to_string(),
            provider_capabilities: serde_json::Value::Null,
        };
        assert!(validate_invoice_policy_allows_creation(&config_mo).is_ok());
    }

    #[test]
    fn test_validate_invoice_policy_allows_creation_none_rejected() {
        let config = InvoicePolicyConfig {
            policy: "none".to_string(),
            provider_capabilities: serde_json::Value::Null,
        };
        let result = validate_invoice_policy_allows_creation(&config);
        assert!(result.is_err());
        match result {
            Err(CoreError::Forbidden(msg)) => {
                assert!(msg.contains("disabled by policy"));
            }
            _ => panic!("Expected Forbidden error"),
        }
    }

    #[test]
    fn test_map_stripe_invoice_status_mapping() {
        assert_eq!(
            map_stripe_invoice_status("draft").unwrap(),
            InvoiceStatus::Draft
        );
        assert_eq!(
            map_stripe_invoice_status("open").unwrap(),
            InvoiceStatus::Issued
        );
        assert_eq!(
            map_stripe_invoice_status("paid").unwrap(),
            InvoiceStatus::Paid
        );
        assert_eq!(
            map_stripe_invoice_status("void").unwrap(),
            InvoiceStatus::Void
        );
        assert_eq!(
            map_stripe_invoice_status("voided").unwrap(),
            InvoiceStatus::Void
        );
        assert_eq!(
            map_stripe_invoice_status("uncollectible").unwrap(),
            InvoiceStatus::Void
        );

        // Unknown status should error
        let result = map_stripe_invoice_status("unknown_status");
        assert!(result.is_err());
        match result {
            Err(CoreError::BadRequest(msg)) => {
                assert!(msg.contains("Unknown Stripe invoice status"));
            }
            _ => panic!("Expected BadRequest error"),
        }
    }
}
