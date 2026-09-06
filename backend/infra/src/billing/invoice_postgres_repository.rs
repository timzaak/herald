// PostgreSQL implementation for Invoice repository
//
// Uses raw sqlx queries because invoice tables do not have SeaORM entity definitions
// in herald-entity. This follows the same pattern as oauth/config_repository.rs and
// realm_config/mod.rs in this crate.

use chrono::{DateTime, Datelike, Utc};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use herald_domain::billing::invoice::{
    ActorType, AdjustmentMode, AttributionFilter, ExternalInvoiceData, Invoice, InvoiceDetail,
    InvoiceEventType, InvoiceHistory, InvoiceLineItem, InvoiceListFilters, InvoiceProvider,
    InvoiceRepository, InvoiceSellerConfig, InvoiceSource, InvoiceStatus, InvoiceStatusTransition,
    InvoiceSummary, NewInvoice, NewLineItem, PaginatedInvoices, UpdateInvoiceDraft,
};
use herald_domain::billing::invoice_service::{
    calculate_invoice_amounts, calculate_line_item_subtotal, format_invoice_number,
};
use herald_domain::common::entities::app_errors::CoreError;

pub struct PostgresInvoiceRepository {
    db: DatabaseConnection,
}

impl PostgresInvoiceRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

// ---------------------------------------------------------------------------
// Row types for sqlx query_as
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct InvoiceRow {
    id: Uuid,
    realm_id: String,
    invoice_number: String,
    source: String,
    account_id: Option<Uuid>,
    applicant_user_id: Option<Uuid>,
    subscription_id: Option<Uuid>,
    payment_attempt_id: Option<Uuid>,
    status: String,
    currency: String,
    // Provider fields
    provider: String,
    payment_provider: Option<String>,
    external_invoice_id: Option<String>,
    external_order_id: Option<String>,
    external_status: Option<String>,
    external_hosted_url: Option<String>,
    external_pdf_url: Option<String>,
    external_payload: Option<serde_json::Value>,
    tax_details: Option<serde_json::Value>,
    // Dates
    issue_date: Option<chrono::NaiveDate>,
    due_date: Option<chrono::NaiveDate>,
    issued_at: Option<DateTime<Utc>>,
    paid_at: Option<DateTime<Utc>>,
    voided_at: Option<DateTime<Utc>>,
    subtotal: i64,
    discount_amount: i64,
    tax_amount: i64,
    shipping_amount: i64,
    total: i64,
    amount_refunded: i64,
    amount_remaining: i64,
    discount_mode: Option<String>,
    discount_value: Option<String>,
    tax_mode: Option<String>,
    tax_value: Option<String>,
    shipping_mode: Option<String>,
    shipping_value: Option<String>,
    billing_name: Option<String>,
    billing_address: Option<String>,
    billing_email: Option<String>,
    billing_phone: Option<String>,
    billing_tax_id: Option<String>,
    seller_name: Option<String>,
    seller_address: Option<String>,
    seller_email: Option<String>,
    seller_phone: Option<String>,
    seller_tax_id: Option<String>,
    notes: Option<String>,
    payment_terms: Option<String>,
    void_reason: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn parse_provider_from_row(s: &str) -> Result<InvoiceProvider, CoreError> {
    s.parse::<InvoiceProvider>()
        .map_err(|e| CoreError::DatabaseError(e.to_string()))
}

fn row_to_invoice(row: InvoiceRow) -> Result<Invoice, CoreError> {
    Ok(Invoice {
        id: row.id,
        realm_id: row.realm_id,
        invoice_number: row.invoice_number,
        source: row.source.parse()?,
        account_id: row.account_id,
        applicant_user_id: row.applicant_user_id,
        subscription_id: row.subscription_id,
        payment_attempt_id: row.payment_attempt_id,
        status: row.status.parse()?,
        currency: row.currency,
        provider: parse_provider_from_row(&row.provider)?,
        payment_provider: row.payment_provider,
        external_invoice_id: row.external_invoice_id,
        external_order_id: row.external_order_id,
        external_status: row.external_status,
        external_hosted_url: row.external_hosted_url,
        external_pdf_url: row.external_pdf_url,
        external_payload: row.external_payload,
        tax_details: row.tax_details,
        issue_date: row.issue_date,
        due_date: row.due_date,
        issued_at: row.issued_at,
        paid_at: row.paid_at,
        voided_at: row.voided_at,
        subtotal: row.subtotal,
        discount_amount: row.discount_amount,
        tax_amount: row.tax_amount,
        shipping_amount: row.shipping_amount,
        total: row.total,
        amount_refunded: row.amount_refunded,
        amount_remaining: row.amount_remaining,
        discount_mode: row
            .discount_mode
            .as_deref()
            .and_then(AdjustmentMode::from_str_opt),
        discount_value: row.discount_value,
        tax_mode: row
            .tax_mode
            .as_deref()
            .and_then(AdjustmentMode::from_str_opt),
        tax_value: row.tax_value,
        shipping_mode: row
            .shipping_mode
            .as_deref()
            .and_then(AdjustmentMode::from_str_opt),
        shipping_value: row.shipping_value,
        billing_name: row.billing_name,
        billing_address: row.billing_address,
        billing_email: row.billing_email,
        billing_phone: row.billing_phone,
        billing_tax_id: row.billing_tax_id,
        seller_name: row.seller_name,
        seller_address: row.seller_address,
        seller_email: row.seller_email,
        seller_phone: row.seller_phone,
        seller_tax_id: row.seller_tax_id,
        notes: row.notes,
        payment_terms: row.payment_terms,
        void_reason: row.void_reason,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn parse_actor_type(s: &str) -> Result<ActorType, CoreError> {
    match s {
        "user" => Ok(ActorType::User),
        "system" => Ok(ActorType::System),
        _ => Err(CoreError::DatabaseError(format!(
            "Invalid actor_type: {}",
            s
        ))),
    }
}

/// Insert a single invoice_history row within an existing transaction.
/// Generates the history id and created_at timestamp internally.
async fn insert_invoice_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    invoice_id: Uuid,
    event_type: &str,
    actor_user_id: Option<Uuid>,
    actor_type: &str,
    changes: &serde_json::Value,
) -> Result<(), CoreError> {
    let history_id = Uuid::now_v7();
    let now = chrono::Utc::now();
    sqlx::query(
        "INSERT INTO invoice_history (id, invoice_id, event_type, actor_user_id, actor_type, changes, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(history_id)
    .bind(invoice_id)
    .bind(event_type)
    .bind(actor_user_id)
    .bind(actor_type)
    .bind(changes)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(|e| CoreError::DatabaseError(format!("Failed to insert history: {}", e)))?;
    Ok(())
}

fn parse_event_type(s: &str) -> Result<InvoiceEventType, CoreError> {
    match s {
        "created" => Ok(InvoiceEventType::Created),
        "updated" => Ok(InvoiceEventType::Updated),
        "issued" => Ok(InvoiceEventType::Issued),
        "paid" => Ok(InvoiceEventType::Paid),
        "voided" => Ok(InvoiceEventType::Voided),
        "overdue" => Ok(InvoiceEventType::Overdue),
        "credit_note_created" => Ok(InvoiceEventType::CreditNoteCreated),
        "credit_note_voided" => Ok(InvoiceEventType::CreditNoteVoided),
        _ => Err(CoreError::DatabaseError(format!(
            "Invalid event_type: {}",
            s
        ))),
    }
}

#[derive(Debug, sqlx::FromRow)]
struct LineItemRow {
    id: Uuid,
    invoice_id: Uuid,
    sort_order: i32,
    name: String,
    description: Option<String>,
    quantity: String,
    unit_price: i64,
    subtotal: i64,
}

fn row_to_line_item(row: LineItemRow) -> InvoiceLineItem {
    InvoiceLineItem {
        id: row.id,
        invoice_id: row.invoice_id,
        sort_order: row.sort_order,
        name: row.name,
        description: row.description,
        quantity: row.quantity,
        unit_price: row.unit_price,
        subtotal: row.subtotal,
    }
}

#[derive(Debug, sqlx::FromRow)]
struct HistoryRow {
    id: Uuid,
    invoice_id: Uuid,
    event_type: String,
    actor_user_id: Option<Uuid>,
    actor_type: String,
    changes: serde_json::Value,
    created_at: DateTime<Utc>,
}

fn row_to_history(row: HistoryRow) -> Result<InvoiceHistory, CoreError> {
    Ok(InvoiceHistory {
        id: row.id,
        invoice_id: row.invoice_id,
        event_type: parse_event_type(&row.event_type)?,
        actor_user_id: row.actor_user_id,
        actor_type: parse_actor_type(&row.actor_type)?,
        changes: row.changes,
        created_at: row.created_at,
    })
}

#[derive(Debug, sqlx::FromRow)]
struct SellerConfigRow {
    realm_id: String,
    seller_name: String,
    seller_address: String,
    seller_email: Option<String>,
    seller_phone: Option<String>,
    seller_tax_id: String,
    default_payment_terms: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn row_to_seller_config(row: SellerConfigRow) -> InvoiceSellerConfig {
    InvoiceSellerConfig {
        realm_id: row.realm_id,
        seller_name: row.seller_name,
        seller_address: row.seller_address,
        seller_email: row.seller_email,
        seller_phone: row.seller_phone,
        seller_tax_id: row.seller_tax_id,
        default_payment_terms: row.default_payment_terms,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

#[derive(Debug, sqlx::FromRow)]
struct InvoiceSummaryRow {
    id: Uuid,
    realm_id: String,
    invoice_number: String,
    source: String,
    account_id: Option<Uuid>,
    subscription_id: Option<Uuid>,
    payment_attempt_id: Option<Uuid>,
    status: String,
    currency: String,
    total: i64,
    amount_refunded: i64,
    billing_name: Option<String>,
    due_date: Option<chrono::NaiveDate>,
    created_at: DateTime<Utc>,
    provider: String,
    payment_provider: Option<String>,
    external_invoice_id: Option<String>,
    external_hosted_url: Option<String>,
    external_pdf_url: Option<String>,
}

fn row_to_summary(row: InvoiceSummaryRow) -> Result<InvoiceSummary, CoreError> {
    Ok(InvoiceSummary {
        id: row.id,
        realm_id: row.realm_id,
        invoice_number: row.invoice_number,
        source: row.source.parse()?,
        account_id: row.account_id,
        subscription_id: row.subscription_id,
        payment_attempt_id: row.payment_attempt_id,
        status: row.status.parse()?,
        currency: row.currency,
        total: row.total,
        amount_refunded: row.amount_refunded,
        billing_name: row.billing_name,
        due_date: row.due_date,
        created_at: row.created_at,
        provider: parse_provider_from_row(&row.provider)?,
        payment_provider: row.payment_provider,
        external_invoice_id: row.external_invoice_id,
        external_hosted_url: row.external_hosted_url,
        external_pdf_url: row.external_pdf_url,
    })
}

#[derive(Debug, sqlx::FromRow)]
struct CountRow {
    count: i64,
}

// ---------------------------------------------------------------------------
// SQL helpers
// ---------------------------------------------------------------------------

const INVOICE_COLUMNS: &str = r#"
    id, realm_id, invoice_number, source, account_id, applicant_user_id,
    subscription_id, payment_attempt_id, status, currency,
    provider, payment_provider, external_invoice_id, external_order_id,
    external_status, external_hosted_url, external_pdf_url,
    external_payload, tax_details,
    issue_date, due_date, issued_at, paid_at, voided_at,
    subtotal, discount_amount, tax_amount, shipping_amount, total,
    amount_refunded, amount_remaining,
    discount_mode, discount_value, tax_mode, tax_value, shipping_mode, shipping_value,
    billing_name, billing_address, billing_email, billing_phone, billing_tax_id,
    seller_name, seller_address, seller_email, seller_phone, seller_tax_id,
    notes, payment_terms, void_reason, created_at, updated_at
"#;

/// SELECT / RETURNING column list with NUMERIC columns cast to TEXT for sqlx String binding.
const INVOICE_COLUMNS_READ: &str = r#"
    id, realm_id, invoice_number, source, account_id, applicant_user_id,
    subscription_id, payment_attempt_id, status, currency,
    provider, payment_provider, external_invoice_id, external_order_id,
    external_status, external_hosted_url, external_pdf_url,
    external_payload, tax_details,
    issue_date, due_date, issued_at, paid_at, voided_at,
    subtotal, discount_amount, tax_amount, shipping_amount, total,
    amount_refunded, amount_remaining,
    discount_mode, discount_value::text, tax_mode, tax_value::text, shipping_mode, shipping_value::text,
    billing_name, billing_address, billing_email, billing_phone, billing_tax_id,
    seller_name, seller_address, seller_email, seller_phone, seller_tax_id,
    notes, payment_terms, void_reason, created_at, updated_at
"#;

const SUMMARY_COLUMNS: &str = r#"
    id, realm_id, invoice_number, source, account_id,
    subscription_id, payment_attempt_id,
    status, currency,
    total, amount_refunded, billing_name, due_date, created_at,
    provider, payment_provider, external_invoice_id, external_hosted_url, external_pdf_url
"#;

// ---------------------------------------------------------------------------
// InvoiceRepository implementation
// ---------------------------------------------------------------------------

impl InvoiceRepository for PostgresInvoiceRepository {
    async fn create_invoice(&self, input: NewInvoice) -> Result<Invoice, CoreError> {
        let now = chrono::Utc::now();
        let id = Uuid::now_v7();

        let amounts = calculate_invoice_amounts(
            &input.line_items,
            input.discount_mode,
            input.discount_value.as_deref(),
            input.tax_mode,
            input.tax_value.as_deref(),
            input.shipping_mode,
            input.shipping_value.as_deref(),
        )?;

        let mut tx = self.db.get_postgres_connection_pool().begin().await?;

        let year = now.year();
        let invoice_number =
            Self::reserve_invoice_number_tx(&mut tx, &input.realm_id, year).await?;

        let invoice_row = sqlx::query_as::<_, InvoiceRow>(&format!(
            "INSERT INTO invoice ({insert_cols}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33::numeric,$34,$35::numeric,$36,$37::numeric,$38,$39,$40,$41,$42,$43,$44,$45,$46,$47,$48,$49,$50,$51,$52) RETURNING {read_cols}",
            insert_cols = INVOICE_COLUMNS,
            read_cols = INVOICE_COLUMNS_READ
        ))
            .bind(id)
            .bind(&input.realm_id)
            .bind(&invoice_number)
            .bind(input.source.as_str())
            .bind(input.account_id)
            .bind(input.applicant_user_id)
            .bind(input.subscription_id)
            .bind(input.payment_attempt_id)
            .bind(InvoiceStatus::Draft.as_str())
            .bind(&input.currency)
            // Provider fields — manual invoices
            .bind(InvoiceProvider::Manual.as_str()) // provider
            .bind(None::<String>) // payment_provider
            .bind(None::<String>) // external_invoice_id
            .bind(None::<String>) // external_order_id
            .bind(None::<String>) // external_status
            .bind(None::<String>) // external_hosted_url
            .bind(None::<String>) // external_pdf_url
            .bind(None::<serde_json::Value>) // external_payload
            .bind(None::<serde_json::Value>) // tax_details
            // Dates
            .bind(None::<chrono::NaiveDate>) // issue_date
            .bind(input.due_date)
            .bind(None::<DateTime<Utc>>) // issued_at
            .bind(None::<DateTime<Utc>>) // paid_at
            .bind(None::<DateTime<Utc>>) // voided_at
            .bind(amounts.subtotal)
            .bind(amounts.discount_amount)
            .bind(amounts.tax_amount)
            .bind(amounts.shipping_amount)
            .bind(amounts.total)
            // Refund aggregates — new manual invoices start with no refunds.
            .bind(0i64) // amount_refunded
            .bind(amounts.total) // amount_remaining
            .bind(input.discount_mode.map(|m| m.as_str()))
            .bind(&input.discount_value)
            .bind(input.tax_mode.map(|m| m.as_str()))
            .bind(&input.tax_value)
            .bind(input.shipping_mode.map(|m| m.as_str()))
            .bind(&input.shipping_value)
            .bind(&input.billing_name)
            .bind(&input.billing_address)
            .bind(&input.billing_email)
            .bind(&input.billing_phone)
            .bind(&input.billing_tax_id)
            .bind(&input.seller_name)
            .bind(&input.seller_address)
            .bind(&input.seller_email)
            .bind(&input.seller_phone)
            .bind(&input.seller_tax_id)
            .bind(&input.notes)
            .bind(&input.payment_terms)
            .bind(None::<String>) // void_reason
            .bind(now)
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to insert invoice: {}", e)))?;

        for (i, item) in input.line_items.iter().enumerate() {
            let item_subtotal = calculate_line_item_subtotal(&item.quantity, item.unit_price)?;

            let item_id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO invoice_line_item (id, invoice_id, sort_order, name, description, quantity, unit_price, subtotal) VALUES ($1,$2,$3,$4,$5,$6::numeric,$7,$8)"
            )
                .bind(item_id)
                .bind(id)
                .bind(i as i32)
                .bind(&item.name)
                .bind(&item.description)
                .bind(&item.quantity)
                .bind(item.unit_price)
                .bind(item_subtotal)
                .execute(&mut *tx)
                .await
                .map_err(|e| CoreError::DatabaseError(format!("Failed to insert line item: {}", e)))?;
        }

        let changes = serde_json::json!({"status": "draft"});
        insert_invoice_history(
            &mut tx,
            id,
            InvoiceEventType::Created.as_str(),
            input.actor_user_id,
            ActorType::User.as_str(),
            &changes,
        )
        .await?;

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit create_invoice: {}", e))
        })?;

        row_to_invoice(invoice_row)
    }

    async fn update_draft(&self, input: UpdateInvoiceDraft) -> Result<Invoice, CoreError> {
        let now = chrono::Utc::now();
        let mut tx = self.db.get_postgres_connection_pool().begin().await?;

        // Lock the invoice row and verify status is draft
        let existing = sqlx::query_as::<_, InvoiceRow>(&format!(
            "SELECT {cols} FROM invoice WHERE id = $1 AND realm_id = $2 FOR UPDATE",
            cols = INVOICE_COLUMNS_READ
        ))
        .bind(input.invoice_id)
        .bind(&input.realm_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to lock invoice: {}", e)))?
        .ok_or(CoreError::NotFound)?;

        let existing = row_to_invoice(existing)?;
        if existing.status != InvoiceStatus::Draft {
            return Err(CoreError::Conflict(format!(
                "Cannot update invoice in '{}' status",
                existing.status.as_str()
            )));
        }

        let billing_name = input.billing_name.or(existing.billing_name);
        let billing_address = input.billing_address.or(existing.billing_address);
        let billing_email = input.billing_email.or(existing.billing_email);
        let billing_phone = input.billing_phone.or(existing.billing_phone);
        let billing_tax_id = input.billing_tax_id.or(existing.billing_tax_id);
        let seller_name = input.seller_name.or(existing.seller_name);
        let seller_address = input.seller_address.or(existing.seller_address);
        let seller_email = input.seller_email.or(existing.seller_email);
        let seller_phone = input.seller_phone.or(existing.seller_phone);
        let seller_tax_id = input.seller_tax_id.or(existing.seller_tax_id);
        let due_date = input.due_date.or(existing.due_date);
        let payment_terms = input.payment_terms.or(existing.payment_terms);
        let notes = input.notes.or(existing.notes);

        let discount_mode = input.discount_mode.or(existing.discount_mode);
        let discount_value = input.discount_value.or(existing.discount_value);
        let tax_mode = input.tax_mode.or(existing.tax_mode);
        let tax_value = input.tax_value.or(existing.tax_value);
        let shipping_mode = input.shipping_mode.or(existing.shipping_mode);
        let shipping_value = input.shipping_value.or(existing.shipping_value);

        let (subtotal, discount_amount, tax_amount, shipping_amount, total) = if let Some(
            ref items,
        ) =
            input.line_items
        {
            sqlx::query("DELETE FROM invoice_line_item WHERE invoice_id = $1")
                .bind(input.invoice_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    CoreError::DatabaseError(format!("Failed to delete line items: {}", e))
                })?;

            for (i, item) in items.iter().enumerate() {
                let item_subtotal = calculate_invoice_amounts(
                    std::slice::from_ref(item),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )?
                .subtotal;

                let item_id = Uuid::now_v7();
                sqlx::query(
                        "INSERT INTO invoice_line_item (id, invoice_id, sort_order, name, description, quantity, unit_price, subtotal) VALUES ($1,$2,$3,$4,$5,$6::numeric,$7,$8)"
                    )
                        .bind(item_id)
                        .bind(input.invoice_id)
                        .bind(i as i32)
                        .bind(&item.name)
                        .bind(&item.description)
                        .bind(&item.quantity)
                        .bind(item.unit_price)
                        .bind(item_subtotal)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| CoreError::DatabaseError(format!("Failed to insert line item: {}", e)))?;
            }

            // Recalculate amounts from new line items
            let amounts = calculate_invoice_amounts(
                items,
                discount_mode,
                discount_value.as_deref(),
                tax_mode,
                tax_value.as_deref(),
                shipping_mode,
                shipping_value.as_deref(),
            )?;
            (
                amounts.subtotal,
                amounts.discount_amount,
                amounts.tax_amount,
                amounts.shipping_amount,
                amounts.total,
            )
        } else {
            // Recalculate amounts from existing line items if adjustment inputs changed
            let existing_items = sqlx::query_as::<_, LineItemRow>(
                    "SELECT id, invoice_id, sort_order, name, description, quantity::text, unit_price, subtotal FROM invoice_line_item WHERE invoice_id = $1 ORDER BY sort_order"
                )
                    .bind(input.invoice_id)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(|e| CoreError::DatabaseError(format!("Failed to fetch line items: {}", e)))?;

            let new_items: Vec<NewLineItem> = existing_items
                .iter()
                .map(|li| NewLineItem {
                    name: li.name.clone(),
                    description: li.description.clone(),
                    quantity: li.quantity.clone(),
                    unit_price: li.unit_price,
                })
                .collect();

            let amounts = calculate_invoice_amounts(
                &new_items,
                discount_mode,
                discount_value.as_deref(),
                tax_mode,
                tax_value.as_deref(),
                shipping_mode,
                shipping_value.as_deref(),
            )?;
            (
                amounts.subtotal,
                amounts.discount_amount,
                amounts.tax_amount,
                amounts.shipping_amount,
                amounts.total,
            )
        };

        // Recompute amount_remaining so the cached column tracks the new total.
        // (Invariant: amount_remaining = total - amount_refunded.)
        let amount_remaining = total - existing.amount_refunded;

        // Update invoice row
        let updated = sqlx::query_as::<_, InvoiceRow>(&format!(
            "UPDATE invoice SET
                billing_name = $1, billing_address = $2, billing_email = $3, billing_phone = $4,
                billing_tax_id = $5,
                seller_name = $6, seller_address = $7, seller_email = $8, seller_phone = $9,
                seller_tax_id = $10,
                due_date = $11, payment_terms = $12, notes = $13,
                discount_mode = $14, discount_value = $15::numeric,
                tax_mode = $16, tax_value = $17::numeric,
                shipping_mode = $18, shipping_value = $19::numeric,
                subtotal = $20, discount_amount = $21, tax_amount = $22, shipping_amount = $23,
                total = $24, amount_remaining = $25, updated_at = $26
             WHERE id = $27
             RETURNING {cols}",
            cols = INVOICE_COLUMNS_READ
        ))
        .bind(&billing_name)
        .bind(&billing_address)
        .bind(&billing_email)
        .bind(&billing_phone)
        .bind(&billing_tax_id)
        .bind(&seller_name)
        .bind(&seller_address)
        .bind(&seller_email)
        .bind(&seller_phone)
        .bind(&seller_tax_id)
        .bind(due_date)
        .bind(&payment_terms)
        .bind(&notes)
        .bind(discount_mode.map(|m| m.as_str()))
        .bind(&discount_value)
        .bind(tax_mode.map(|m| m.as_str()))
        .bind(&tax_value)
        .bind(shipping_mode.map(|m| m.as_str()))
        .bind(&shipping_value)
        .bind(subtotal)
        .bind(discount_amount)
        .bind(tax_amount)
        .bind(shipping_amount)
        .bind(total)
        .bind(amount_remaining)
        .bind(now)
        .bind(input.invoice_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to update invoice: {}", e)))?;

        // Record history
        let changes = serde_json::json!({"action": "updated"});
        insert_invoice_history(
            &mut tx,
            input.invoice_id,
            InvoiceEventType::Updated.as_str(),
            input.actor_user_id,
            ActorType::User.as_str(),
            &changes,
        )
        .await?;

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit update_draft: {}", e))
        })?;

        row_to_invoice(updated)
    }

    async fn find_with_items(
        &self,
        realm_id: &str,
        invoice_id: Uuid,
    ) -> Result<Option<InvoiceDetail>, CoreError> {
        let pool = self.db.get_postgres_connection_pool();

        let invoice_row = sqlx::query_as::<_, InvoiceRow>(&format!(
            "SELECT {cols} FROM invoice WHERE id = $1 AND realm_id = $2",
            cols = INVOICE_COLUMNS_READ
        ))
        .bind(invoice_id)
        .bind(realm_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to find invoice: {}", e)))?;

        let invoice_row = match invoice_row {
            Some(r) => r,
            None => return Ok(None),
        };

        let invoice = row_to_invoice(invoice_row)?;

        let line_item_rows = sqlx::query_as::<_, LineItemRow>(
            "SELECT id, invoice_id, sort_order, name, description, quantity::text, unit_price, subtotal FROM invoice_line_item WHERE invoice_id = $1 ORDER BY sort_order"
        )
            .bind(invoice_id)
            .fetch_all(pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to find line items: {}", e)))?;

        let line_items: Vec<InvoiceLineItem> =
            line_item_rows.into_iter().map(row_to_line_item).collect();

        let history_rows = sqlx::query_as::<_, HistoryRow>(
            "SELECT id, invoice_id, event_type, actor_user_id, actor_type, changes, created_at FROM invoice_history WHERE invoice_id = $1 ORDER BY created_at"
        )
            .bind(invoice_id)
            .fetch_all(pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to find history: {}", e)))?;

        let history: Vec<InvoiceHistory> = history_rows
            .into_iter()
            .map(row_to_history)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(InvoiceDetail {
            invoice,
            line_items,
            history,
        }))
    }

    async fn list_admin(
        &self,
        realm_id: &str,
        filters: InvoiceListFilters,
    ) -> Result<PaginatedInvoices<InvoiceSummary>, CoreError> {
        Self::list_invoices(
            self.db.get_postgres_connection_pool(),
            realm_id,
            None,
            filters,
        )
        .await
    }

    async fn list_user(
        &self,
        realm_id: &str,
        user_id: Uuid,
        filters: InvoiceListFilters,
    ) -> Result<PaginatedInvoices<InvoiceSummary>, CoreError> {
        Self::list_invoices(
            self.db.get_postgres_connection_pool(),
            realm_id,
            Some(user_id),
            filters,
        )
        .await
    }

    async fn transition_status(
        &self,
        input: InvoiceStatusTransition,
    ) -> Result<Invoice, CoreError> {
        let now = chrono::Utc::now();
        let mut tx = self.db.get_postgres_connection_pool().begin().await?;

        let current = sqlx::query_as::<_, InvoiceRow>(&format!(
            "SELECT {cols} FROM invoice WHERE id = $1 AND realm_id = $2 FOR UPDATE",
            cols = INVOICE_COLUMNS_READ
        ))
        .bind(input.invoice_id)
        .bind(&input.realm_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to lock invoice: {}", e)))?
        .ok_or(CoreError::NotFound)?;

        let current = row_to_invoice(current)?;

        let (
            status_str,
            issued_at_set,
            paid_at_set,
            voided_at_set,
            issue_date_set,
            void_reason_set,
        ) = match input.target_status {
            InvoiceStatus::Issued => (
                InvoiceStatus::Issued.as_str(),
                Some(now),
                None,
                None,
                Some(input.issue_date.unwrap_or(now.date_naive())),
                None,
            ),
            InvoiceStatus::Paid => (
                InvoiceStatus::Paid.as_str(),
                None,
                Some(input.paid_at.unwrap_or(now)),
                None,
                None,
                None,
            ),
            InvoiceStatus::Void => (
                InvoiceStatus::Void.as_str(),
                None,
                None,
                Some(now),
                None,
                input.void_reason,
            ),
            InvoiceStatus::Overdue => (
                InvoiceStatus::Overdue.as_str(),
                None,
                None,
                None,
                None,
                None,
            ),
            InvoiceStatus::Draft => {
                return Err(CoreError::Conflict(
                    "Cannot transition to draft".to_string(),
                ));
            }
        };

        let updated = sqlx::query_as::<_, InvoiceRow>(&format!(
            "UPDATE invoice SET
                status = $1,
                issued_at = COALESCE($2, issued_at),
                paid_at = COALESCE($3, paid_at),
                voided_at = COALESCE($4, voided_at),
                issue_date = COALESCE($5, issue_date),
                void_reason = COALESCE($6, void_reason),
                updated_at = $7
             WHERE id = $8
             RETURNING {cols}",
            cols = INVOICE_COLUMNS_READ
        ))
        .bind(status_str)
        .bind(issued_at_set)
        .bind(paid_at_set)
        .bind(voided_at_set)
        .bind(issue_date_set)
        .bind(&void_reason_set)
        .bind(now)
        .bind(input.invoice_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to update status: {}", e)))?;

        // Record history event
        let event_type = match input.target_status {
            InvoiceStatus::Issued => InvoiceEventType::Issued,
            InvoiceStatus::Paid => InvoiceEventType::Paid,
            InvoiceStatus::Void => InvoiceEventType::Voided,
            InvoiceStatus::Overdue => InvoiceEventType::Overdue,
            InvoiceStatus::Draft => unreachable!(),
        };

        let changes = serde_json::json!({
            "field": "status",
            "from": current.status.as_str(),
            "to": input.target_status.as_str()
        });

        insert_invoice_history(
            &mut tx,
            input.invoice_id,
            event_type.as_str(),
            input.actor_user_id,
            input.actor_type.as_str(),
            &changes,
        )
        .await?;

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit transition_status: {}", e))
        })?;

        row_to_invoice(updated)
    }

    async fn find_seller_config(
        &self,
        realm_id: &str,
    ) -> Result<Option<InvoiceSellerConfig>, CoreError> {
        let row = sqlx::query_as::<_, SellerConfigRow>(
            "SELECT realm_id, seller_name, seller_address, seller_email, seller_phone, seller_tax_id, default_payment_terms, created_at, updated_at FROM invoice_seller_config WHERE realm_id = $1"
        )
            .bind(realm_id)
            .fetch_optional(self.db.get_postgres_connection_pool())
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to find seller config: {}", e)))?;

        Ok(row.map(row_to_seller_config))
    }

    async fn upsert_seller_config(
        &self,
        config: InvoiceSellerConfig,
    ) -> Result<InvoiceSellerConfig, CoreError> {
        let now = chrono::Utc::now();

        let row = sqlx::query_as::<_, SellerConfigRow>(
            "INSERT INTO invoice_seller_config (realm_id, seller_name, seller_address, seller_email, seller_phone, seller_tax_id, default_payment_terms, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (realm_id) DO UPDATE SET
                 seller_name = EXCLUDED.seller_name,
                 seller_address = EXCLUDED.seller_address,
                 seller_email = EXCLUDED.seller_email,
                 seller_phone = EXCLUDED.seller_phone,
                 seller_tax_id = EXCLUDED.seller_tax_id,
                 default_payment_terms = EXCLUDED.default_payment_terms,
                 updated_at = EXCLUDED.updated_at
             RETURNING realm_id, seller_name, seller_address, seller_email, seller_phone, seller_tax_id, default_payment_terms, created_at, updated_at"
        )
            .bind(&config.realm_id)
            .bind(&config.seller_name)
            .bind(&config.seller_address)
            .bind(&config.seller_email)
            .bind(&config.seller_phone)
            .bind(&config.seller_tax_id)
            .bind(&config.default_payment_terms)
            .bind(now)
            .bind(now)
            .fetch_one(self.db.get_postgres_connection_pool())
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to upsert seller config: {}", e)))?;

        Ok(row_to_seller_config(row))
    }

    async fn list_overdue_candidates(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<Invoice>, CoreError> {
        let today = now.date_naive();

        let rows = sqlx::query_as::<_, InvoiceRow>(&format!(
            "SELECT {cols} FROM invoice
             WHERE status = 'issued'
               AND provider = 'manual'
               AND due_date < $1
             ORDER BY due_date ASC
             LIMIT $2",
            cols = INVOICE_COLUMNS_READ
        ))
        .bind(today)
        .bind(limit)
        .fetch_all(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to list overdue candidates: {}", e))
        })?;

        rows.into_iter().map(row_to_invoice).collect()
    }

    async fn next_invoice_number(&self, realm_id: &str, year: i32) -> Result<String, CoreError> {
        let mut tx = self.db.get_postgres_connection_pool().begin().await?;

        let invoice_number = Self::reserve_invoice_number_tx(&mut tx, realm_id, year).await?;

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit next_invoice_number: {}", e))
        })?;

        Ok(invoice_number)
    }

    async fn upsert_external_invoice(
        &self,
        data: ExternalInvoiceData,
    ) -> Result<Invoice, CoreError> {
        let now = chrono::Utc::now();
        let id = Uuid::now_v7();

        let invoice_number = format!(
            "EXT-{}-{}",
            data.provider.as_str().to_uppercase(),
            data.external_invoice_id
                .as_deref()
                .unwrap_or(data.external_order_id.as_deref().unwrap_or("unknown"))
        );

        let source = InvoiceSource::ExternalSync.as_str();
        let provider = data.provider.as_str();
        let payment_provider = data
            .payment_provider
            .as_deref()
            .unwrap_or(data.provider.as_str());
        let status = data.status.as_str();

        // The two branches share the same INSERT + VALUES + bind sequence.
        // Only the ON CONFLICT clause differs.
        //
        // The buyer attribution/snapshot fields (account_id, applicant_user_id,
        // billing_name/email/phone/address) are COALESCE-preserved on update so
        // the first event to resolve them wins. This matters for one-time
        // Checkout purchases: `checkout.session.completed` lands first and
        // resolves account_id from session metadata, but the buyer snapshot
        // (customer_name/email/...) only arrives with the subsequent
        // `invoice.*` events — both must be merged onto the same row.
        let on_conflict = if data.external_invoice_id.is_some() {
            // Branch A: match on (realm_id, external_invoice_id)
            //
            // `external_hosted_url` / `external_pdf_url` use COALESCE(EXCLUDED, invoice)
            // for the same reason as the attribution fields: a re-upsert from the
            // renewal path (`invoice.payment_succeeded`) carries `None` for these when
            // its payload omits `hosted_invoice_url`/`invoice_pdf` (the common case for
            // renewals). Without COALESCE the NULL would overwrite the URL written by
            // the earlier `invoice.finalized` sync event.
            // When EXCLUDED is non-NULL (sync path) it still overrides, preserving the
            // sync semantics.
            "ON CONFLICT (realm_id, external_invoice_id) WHERE external_invoice_id IS NOT NULL
             DO UPDATE SET
                external_status = EXCLUDED.external_status,
                external_hosted_url = COALESCE(EXCLUDED.external_hosted_url, invoice.external_hosted_url),
                external_pdf_url = COALESCE(EXCLUDED.external_pdf_url, invoice.external_pdf_url),
                external_order_id = COALESCE(EXCLUDED.external_order_id, invoice.external_order_id),
                external_payload = EXCLUDED.external_payload,
                tax_details = EXCLUDED.tax_details,
                subscription_id = COALESCE(EXCLUDED.subscription_id, invoice.subscription_id),
                payment_attempt_id = COALESCE(EXCLUDED.payment_attempt_id, invoice.payment_attempt_id),
                account_id = COALESCE(EXCLUDED.account_id, invoice.account_id),
                applicant_user_id = COALESCE(EXCLUDED.applicant_user_id, invoice.applicant_user_id),
                billing_name = COALESCE(EXCLUDED.billing_name, invoice.billing_name),
                billing_email = COALESCE(EXCLUDED.billing_email, invoice.billing_email),
                billing_phone = COALESCE(EXCLUDED.billing_phone, invoice.billing_phone),
                billing_address = COALESCE(EXCLUDED.billing_address, invoice.billing_address),
                status = EXCLUDED.status,
                updated_at = NOW()"
        } else {
            // Branch B: match on (realm_id, external_order_id)
            //
            // This branch matches by external_order_id and is not exercised by the
            // Stripe invoice.finalized/payment_succeeded sync paths (which always
            // carry external_invoice_id). The two URL columns are left out of the
            // UPDATE here because the carrier value (EXCLUDED.external_hosted_url /
            // external_pdf_url) is the same as on the freshly-inserted row, and the
            // existing row's URL set must be preserved verbatim.
            "ON CONFLICT (realm_id, external_order_id) WHERE external_order_id IS NOT NULL
             DO UPDATE SET
                external_status = EXCLUDED.external_status,
                external_payload = EXCLUDED.external_payload,
                tax_details = EXCLUDED.tax_details,
                subscription_id = COALESCE(EXCLUDED.subscription_id, invoice.subscription_id),
                payment_attempt_id = COALESCE(EXCLUDED.payment_attempt_id, invoice.payment_attempt_id),
                account_id = COALESCE(EXCLUDED.account_id, invoice.account_id),
                applicant_user_id = COALESCE(EXCLUDED.applicant_user_id, invoice.applicant_user_id),
                billing_name = COALESCE(EXCLUDED.billing_name, invoice.billing_name),
                billing_email = COALESCE(EXCLUDED.billing_email, invoice.billing_email),
                billing_phone = COALESCE(EXCLUDED.billing_phone, invoice.billing_phone),
                billing_address = COALESCE(EXCLUDED.billing_address, invoice.billing_address),
                status = EXCLUDED.status,
                updated_at = NOW()"
        };

        // Bind order follows the column order exactly ($1..$26). Literal
        // amount columns (subtotal/discount/tax/shipping/amount_refunded = 0,
        // total = amount_remaining = $22) are inlined to keep the parameter
        // list linear and avoid the prior out-of-order placeholder trick.
        let sql = format!(
            "INSERT INTO invoice (
                id, realm_id, invoice_number, source,
                account_id, applicant_user_id,
                billing_name, billing_email, billing_phone, billing_address,
                status, currency,
                provider, payment_provider,
                external_invoice_id, external_order_id,
                external_status, external_hosted_url, external_pdf_url,
                external_payload, tax_details,
                subtotal, discount_amount, tax_amount, shipping_amount, total,
                amount_refunded, amount_remaining,
                subscription_id, payment_attempt_id,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6,
                $7, $8, $9, $10,
                $11, $12,
                $13, $14,
                $15, $16,
                $17, $18, $19,
                $20, $21,
                0, 0, 0, 0, $22,
                0, $22,
                $23, $24,
                $25, $26
            )
            {on_conflict}
            RETURNING {cols}",
            cols = INVOICE_COLUMNS_READ
        );

        let row = sqlx::query_as::<_, InvoiceRow>(&sql)
            .bind(id) // $1  id
            .bind(&data.realm_id) // $2  realm_id
            .bind(&invoice_number) // $3  invoice_number
            .bind(source) // $4  source
            .bind(data.account_id) // $5  account_id
            .bind(data.applicant_user_id) // $6  applicant_user_id
            .bind(&data.billing_name) // $7  billing_name
            .bind(&data.billing_email) // $8  billing_email
            .bind(&data.billing_phone) // $9  billing_phone
            .bind(&data.billing_address) // $10 billing_address
            .bind(status) // $11 status
            .bind(&data.currency) // $12 currency
            .bind(provider) // $13 provider
            .bind(payment_provider) // $14 payment_provider
            .bind(&data.external_invoice_id) // $15 external_invoice_id
            .bind(&data.external_order_id) // $16 external_order_id
            .bind(&data.external_status) // $17 external_status
            .bind(&data.external_hosted_url) // $18 external_hosted_url
            .bind(&data.external_pdf_url) // $19 external_pdf_url
            .bind(&data.external_payload) // $20 external_payload
            .bind(&data.tax_details) // $21 tax_details
            .bind(data.total) // $22 total (+ amount_remaining)
            .bind(data.subscription_id) // $23 subscription_id
            .bind(data.payment_attempt_id) // $24 payment_attempt_id
            .bind(now) // $25 created_at
            .bind(now) // $26 updated_at
            .fetch_one(self.db.get_postgres_connection_pool())
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to upsert external invoice: {}", e))
            })?;

        row_to_invoice(row)
    }

    async fn find_by_external_invoice_id(
        &self,
        realm_id: &str,
        external_invoice_id: &str,
    ) -> Result<Option<Invoice>, CoreError> {
        let row = sqlx::query_as::<_, InvoiceRow>(&format!(
            "SELECT {cols} FROM invoice WHERE realm_id = $1 AND external_invoice_id = $2",
            cols = INVOICE_COLUMNS_READ
        ))
        .bind(realm_id)
        .bind(external_invoice_id)
        .fetch_optional(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to find invoice by external id: {}", e))
        })?;

        row.map(row_to_invoice).transpose()
    }
}

// ---------------------------------------------------------------------------
// Shared helper for invoice number counter
// ---------------------------------------------------------------------------

impl PostgresInvoiceRepository {
    /// Reserve the next invoice number within an existing transaction.
    /// Uses SELECT FOR UPDATE for row-level locking to prevent concurrent counter collisions.
    async fn reserve_invoice_number_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        realm_id: &str,
        year: i32,
    ) -> Result<String, CoreError> {
        // Try to lock existing counter row
        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT next_seq FROM invoice_number_counter WHERE realm_id = $1 AND year = $2 FOR UPDATE"
        )
            .bind(realm_id)
            .bind(year)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to lock counter: {}", e)))?;

        let seq = match existing {
            Some((current_seq,)) => {
                // Update existing counter
                sqlx::query(
                    "UPDATE invoice_number_counter SET next_seq = next_seq + 1, updated_at = NOW() WHERE realm_id = $1 AND year = $2"
                )
                    .bind(realm_id)
                    .bind(year)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| CoreError::DatabaseError(format!("Failed to update counter: {}", e)))?;
                current_seq
            }
            None => {
                // First invoice for this realm+year: insert with next_seq=2, return seq=1
                sqlx::query(
                    "INSERT INTO invoice_number_counter (realm_id, year, next_seq, updated_at) VALUES ($1, $2, 2, NOW())"
                )
                    .bind(realm_id)
                    .bind(year)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| CoreError::DatabaseError(format!("Failed to insert counter: {}", e)))?;
                1
            }
        };

        Ok(format_invoice_number(year, seq))
    }

    /// Shared implementation for list_admin and list_user queries.
    async fn list_invoices(
        pool: &sqlx::PgPool,
        realm_id: &str,
        user_id: Option<Uuid>,
        filters: InvoiceListFilters,
    ) -> Result<PaginatedInvoices<InvoiceSummary>, CoreError> {
        let page = filters.page.unwrap_or(1).max(1);
        let page_size = filters.page_size.unwrap_or(20).clamp(1, 100);
        let offset = (page - 1) * page_size;

        // Build WHERE conditions
        let mut conditions = vec!["realm_id = $1".to_string()];
        let mut param_idx = 2u32;

        if let Some(_user_id) = user_id {
            conditions.push(format!(
                "(applicant_user_id = ${0} OR account_id = ${0})",
                param_idx
            ));
            param_idx += 1;
        }

        if filters.status.is_some() {
            conditions.push(format!("status = ${}", param_idx));
            param_idx += 1;
        }

        if filters.source.is_some() {
            conditions.push(format!("source = ${}", param_idx));
            param_idx += 1;
        }

        if filters.provider.is_some() {
            conditions.push(format!("provider = ${}", param_idx));
            param_idx += 1;
        }

        // external_only — read-path filter for invoice policy `none` (list
        // only externally-synced invoices). Static condition (no bind), so it
        // does not consume a param index — same pattern as the attribution
        // filter below.
        if filters.external_only {
            conditions.push("provider <> 'manual'".to_string());
        }

        if filters.date_from.is_some() {
            conditions.push(format!("created_at >= ${}", param_idx));
            param_idx += 1;
        }

        if filters.date_to.is_some() {
            conditions.push(format!(
                "created_at < (${param_idx}::date + interval '1 day')"
            ));
            param_idx += 1;
        }

        if filters.search.is_some() {
            conditions.push(format!(
                "(invoice_number ILIKE ${} OR billing_name ILIKE ${})",
                param_idx, param_idx
            ));
            param_idx += 1;
        }

        // attribution=missing — externally-synced invoice with no local
        // attribution. Static condition (no bind), so it does not consume a
        // param index. See domain::billing::invoice::AttributionFilter.
        if matches!(filters.attribution, Some(AttributionFilter::Missing)) {
            conditions.push(
                "(provider <> 'manual' AND subscription_id IS NULL AND payment_attempt_id IS NULL)"
                    .to_string(),
            );
        }

        let where_clause = conditions.join(" AND ");

        // Count query
        let count_sql = format!(
            "SELECT COUNT(*) as count FROM invoice WHERE {}",
            where_clause
        );
        let mut count_query = sqlx::query_as::<_, CountRow>(&count_sql);
        count_query = count_query.bind(realm_id);
        if let Some(uid) = user_id {
            count_query = count_query.bind(uid);
        }
        if let Some(ref s) = filters.status {
            count_query = count_query.bind(s.as_str());
        }
        if let Some(ref s) = filters.source {
            count_query = count_query.bind(s.as_str());
        }
        if let Some(ref p) = filters.provider {
            count_query = count_query.bind(p.as_str());
        }
        if let Some(d) = filters.date_from {
            count_query = count_query.bind(d);
        }
        if let Some(d) = filters.date_to {
            count_query = count_query.bind(d);
        }
        if let Some(ref s) = filters.search {
            count_query = count_query.bind(format!("%{}%", s));
        }

        let total = count_query
            .fetch_one(pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to count invoices: {}", e)))?
            .count as u64;

        // Data query
        let data_sql = format!(
            "SELECT {summary_cols} FROM invoice WHERE {where} ORDER BY created_at DESC LIMIT ${limit_idx} OFFSET ${offset_idx}",
            summary_cols = SUMMARY_COLUMNS,
            where = where_clause,
            limit_idx = param_idx,
            offset_idx = param_idx + 1,
        );

        let mut data_query = sqlx::query_as::<_, InvoiceSummaryRow>(&data_sql);
        data_query = data_query.bind(realm_id);
        if let Some(uid) = user_id {
            data_query = data_query.bind(uid);
        }
        if let Some(ref s) = filters.status {
            data_query = data_query.bind(s.as_str());
        }
        if let Some(ref s) = filters.source {
            data_query = data_query.bind(s.as_str());
        }
        if let Some(ref p) = filters.provider {
            data_query = data_query.bind(p.as_str());
        }
        if let Some(d) = filters.date_from {
            data_query = data_query.bind(d);
        }
        if let Some(d) = filters.date_to {
            data_query = data_query.bind(d);
        }
        if let Some(ref s) = filters.search {
            data_query = data_query.bind(format!("%{}%", s));
        }
        data_query = data_query.bind(page_size as i64);
        data_query = data_query.bind(offset as i64);

        let rows = data_query
            .fetch_all(pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to list invoices: {}", e)))?;

        let data: Vec<InvoiceSummary> = rows
            .into_iter()
            .map(row_to_summary)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(PaginatedInvoices {
            total,
            page,
            page_size,
            data,
        })
    }
}
