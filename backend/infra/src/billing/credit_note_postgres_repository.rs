// PostgreSQL implementation for CreditNote repository
//
// Uses raw sqlx queries because the credit_note table does not have a SeaORM
// entity definition in herald-entity. This follows the same pattern as
// invoice_postgres_repository.rs.

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use herald_domain::billing::credit_note::{
    CreditNote, CreditNoteRepository, CreditNoteSource, CreditNoteStatus, NewCreditNote,
};
use herald_domain::billing::invoice::InvoiceEventType;
use herald_domain::common::entities::app_errors::CoreError;

pub struct PostgresCreditNoteRepository {
    db: DatabaseConnection,
}

impl PostgresCreditNoteRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

// ---------------------------------------------------------------------------
// Row type for sqlx query_as
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct CreditNoteRow {
    id: Uuid,
    invoice_id: Uuid,
    realm_id: String,
    amount: i64,
    currency: String,
    source: String,
    status: String,
    external_credit_note_id: Option<String>,
    memo: Option<String>,
    created_by_user_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

fn parse_source(s: &str) -> Result<CreditNoteSource, CoreError> {
    CreditNoteSource::from_str_opt(s)
        .ok_or_else(|| CoreError::DatabaseError(format!("Invalid credit note source: {}", s)))
}

fn parse_status(s: &str) -> Result<CreditNoteStatus, CoreError> {
    CreditNoteStatus::from_str_opt(s)
        .ok_or_else(|| CoreError::DatabaseError(format!("Invalid credit note status: {}", s)))
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

fn row_to_credit_note(row: CreditNoteRow) -> Result<CreditNote, CoreError> {
    Ok(CreditNote {
        id: row.id,
        invoice_id: row.invoice_id,
        realm_id: row.realm_id,
        amount: row.amount,
        currency: row.currency,
        source: parse_source(&row.source)?,
        status: parse_status(&row.status)?,
        external_credit_note_id: row.external_credit_note_id,
        memo: row.memo,
        created_by_user_id: row.created_by_user_id,
        created_at: row.created_at,
    })
}

/// Column list for credit_note SELECT / RETURNING.
/// Mirrors the field order of CreditNoteRow.
const CREDIT_NOTE_COLUMNS: &str = r#"
    id, invoice_id, realm_id, amount, currency, source, status,
    external_credit_note_id, memo, created_by_user_id, created_at
"#;

// ---------------------------------------------------------------------------
// CreditNoteRepository implementation
// ---------------------------------------------------------------------------

impl CreditNoteRepository for PostgresCreditNoteRepository {
    async fn update_external_memo(
        &self,
        realm_id: &str,
        external_credit_note_id: &str,
        memo: Option<String>,
    ) -> Result<(), CoreError> {
        let result = sqlx::query(
            "UPDATE credit_note SET memo = $3 WHERE realm_id = $1 AND external_credit_note_id = $2 AND source = 'stripe'",
        )
        .bind(realm_id).bind(external_credit_note_id).bind(memo)
        .execute(self.db.get_postgres_connection_pool()).await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }

    async fn find_by_invoice_id(
        &self,
        realm_id: &str,
        invoice_id: Uuid,
    ) -> Result<Vec<CreditNote>, CoreError> {
        let rows = sqlx::query_as::<_, CreditNoteRow>(&format!(
            "SELECT {cols} FROM credit_note WHERE realm_id = $1 AND invoice_id = $2 ORDER BY created_at",
            cols = CREDIT_NOTE_COLUMNS
        ))
        .bind(realm_id)
        .bind(invoice_id)
        .fetch_all(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to list credit notes: {}", e))
        })?;

        rows.into_iter().map(row_to_credit_note).collect()
    }

    async fn find_by_external_id(
        &self,
        external_credit_note_id: &str,
    ) -> Result<Option<CreditNote>, CoreError> {
        let row = sqlx::query_as::<_, CreditNoteRow>(&format!(
            "SELECT {cols} FROM credit_note WHERE external_credit_note_id = $1",
            cols = CREDIT_NOTE_COLUMNS
        ))
        .bind(external_credit_note_id)
        .fetch_optional(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to find credit note by external id: {}", e))
        })?;

        row.map(row_to_credit_note).transpose()
    }

    async fn create_credit_note_and_update_invoice(
        &self,
        input: NewCreditNote,
    ) -> Result<CreditNote, CoreError> {
        // Amount validation — applies to both Stripe and Manual sources.
        if input.amount <= 0 {
            return Err(CoreError::BadRequest(
                "Credit note amount must be positive".to_string(),
            ));
        }

        let mut tx = self
            .db
            .get_postgres_connection_pool()
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to begin transaction: {}", e)))?;

        // Lock the invoice row first. Serializes concurrent credit_note writes for the
        // same invoice, so by the time Tx2 acquires the lock, Tx1 has committed and the
        // idempotency SELECT below will observe the new row. (Previously the idempotency
        // SELECT ran before the lock, racing on INSERT.)
        let invoice_lock: Option<(i64, i64, String)> = sqlx::query_as(
            "SELECT amount_refunded, amount_remaining, currency FROM invoice \
             WHERE id = $1 AND realm_id = $2 FOR UPDATE",
        )
        .bind(input.invoice_id)
        .bind(&input.realm_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to lock invoice for credit note: {}", e))
        })?;

        let (_current_refunded, amount_remaining, invoice_currency) =
            invoice_lock.ok_or(CoreError::NotFound)?;

        // Currency must match the invoice's currency. Defense-in-depth: the manual
        // handler copies invoice.currency today, but a future caller must not be
        // able to violate this invariant silently.
        if input.currency.to_uppercase() != invoice_currency.to_uppercase() {
            return Err(CoreError::BadRequest(format!(
                "Credit note currency {} does not match invoice currency {}",
                input.currency, invoice_currency
            )));
        }

        // For Stripe source, use external_credit_note_id as idempotency guard:
        // if a row with the same external id already exists, return it without
        // re-applying the refund. This makes webhook retries safe.
        // (Runs AFTER the FOR UPDATE lock — see comment above.)
        if input.source == CreditNoteSource::Stripe
            && let Some(ext_id) = &input.external_credit_note_id
        {
            let existing = sqlx::query_as::<_, CreditNoteRow>(&format!(
                "SELECT {cols} FROM credit_note WHERE external_credit_note_id = $1",
                cols = CREDIT_NOTE_COLUMNS
            ))
            .bind(ext_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to check existing credit note: {}", e))
            })?;

            if let Some(existing) = existing {
                tx.commit().await.map_err(|e| {
                    CoreError::DatabaseError(format!(
                        "Failed to commit idempotent credit note lookup: {}",
                        e
                    ))
                })?;
                return row_to_credit_note(existing);
            }
        }

        if input.amount > amount_remaining {
            return Err(CoreError::BadRequest(format!(
                "Credit note amount {} exceeds invoice remaining amount {}",
                input.amount, amount_remaining
            )));
        }

        let id = Uuid::now_v7();
        let now = chrono::Utc::now();

        let row = sqlx::query_as::<_, CreditNoteRow>(&format!(
            "INSERT INTO credit_note (id, invoice_id, realm_id, amount, currency, source, status, external_credit_note_id, memo, created_by_user_id, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             RETURNING {cols}",
            cols = CREDIT_NOTE_COLUMNS
        ))
            .bind(id)
            .bind(input.invoice_id)
            .bind(&input.realm_id)
            .bind(input.amount)
            .bind(&input.currency)
            .bind(input.source.as_str())
            .bind(CreditNoteStatus::Active.as_str())
            .bind(&input.external_credit_note_id)
            .bind(&input.memo)
            .bind(input.created_by_user_id)
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to insert credit note: {}", e))
            })?;

        sqlx::query(
            "UPDATE invoice \
             SET amount_refunded = amount_refunded + $1, \
                 amount_remaining = amount_remaining - $1, \
                 updated_at = NOW() \
             WHERE id = $2",
        )
        .bind(input.amount)
        .bind(input.invoice_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to update invoice refund totals: {}", e))
        })?;

        // Record an invoice_history row so the audit trail reflects the refund.
        // Manual credit notes attribute the action to the admin user; Stripe credit
        // notes attribute it to the system (originated from a webhook).
        let actor_type = input.source.default_actor_type();
        let actor_user_id = match input.source {
            CreditNoteSource::Manual => input.created_by_user_id,
            CreditNoteSource::Stripe => None,
        };
        let changes = serde_json::json!({
            "action": "credit_note_created",
            "amount": input.amount,
            "source": input.source.as_str(),
        });
        insert_invoice_history(
            &mut tx,
            input.invoice_id,
            InvoiceEventType::CreditNoteCreated.as_str(),
            actor_user_id,
            actor_type.as_str(),
            &changes,
        )
        .await?;

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!(
                "Failed to commit credit note + invoice update: {}",
                e
            ))
        })?;

        row_to_credit_note(row)
    }

    async fn void_credit_note_by_external_id(
        &self,
        realm_id: &str,
        external_credit_note_id: &str,
    ) -> Result<CreditNote, CoreError> {
        let mut tx = self
            .db
            .get_postgres_connection_pool()
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to begin transaction: {}", e)))?;

        // SELECT FOR UPDATE the credit note. The unique index on external_credit_note_id
        // guarantees global uniqueness, but we still constrain by realm_id defensively
        // to avoid leaking cross-realm update semantics.
        let existing: Option<CreditNoteRow> = sqlx::query_as::<_, CreditNoteRow>(&format!(
            "SELECT {cols} FROM credit_note \
             WHERE external_credit_note_id = $1 AND realm_id = $2 FOR UPDATE",
            cols = CREDIT_NOTE_COLUMNS
        ))
        .bind(external_credit_note_id)
        .bind(realm_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to lock credit note for void: {}", e))
        })?;

        let existing = existing.ok_or_else(|| {
            CoreError::DatabaseError(format!(
                "Credit note with external id {} not found in realm {}",
                external_credit_note_id, realm_id
            ))
        })?;

        // Idempotency: already voided — return the existing row without re-reversing.
        if existing.status == CreditNoteStatus::Voided.as_str() {
            tx.commit().await.map_err(|e| {
                CoreError::DatabaseError(format!("Failed to commit void idempotency: {}", e))
            })?;
            return row_to_credit_note(existing);
        }

        // Reverse the credit note's amount on the parent invoice. Locks the invoice
        // via the credit_note row above (same transaction).
        sqlx::query(
            "UPDATE invoice \
             SET amount_refunded = amount_refunded - $1, \
                 amount_remaining = amount_remaining + $1, \
                 updated_at = NOW() \
             WHERE id = $2",
        )
        .bind(existing.amount)
        .bind(existing.invoice_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to reverse invoice refund totals: {}", e))
        })?;

        // Mark the credit note as voided.
        let voided_row = sqlx::query_as::<_, CreditNoteRow>(&format!(
            "UPDATE credit_note SET status = $1 WHERE id = $2 RETURNING {cols}",
            cols = CREDIT_NOTE_COLUMNS
        ))
        .bind(CreditNoteStatus::Voided.as_str())
        .bind(existing.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to void credit note: {}", e)))?;

        // Record an invoice_history row for the reversal. Stripe voids come from webhooks,
        // so the actor is the system.
        let source = parse_source(&existing.source)?; // validated above
        let actor_type = source.default_actor_type();
        let changes = serde_json::json!({
            "action": "credit_note_voided",
            "amount": existing.amount,
            "source": existing.source,
            "external_credit_note_id": external_credit_note_id,
        });
        insert_invoice_history(
            &mut tx,
            existing.invoice_id,
            InvoiceEventType::CreditNoteVoided.as_str(),
            existing.created_by_user_id,
            actor_type.as_str(),
            &changes,
        )
        .await?;

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit void credit note: {}", e))
        })?;

        row_to_credit_note(voided_row)
    }
}
