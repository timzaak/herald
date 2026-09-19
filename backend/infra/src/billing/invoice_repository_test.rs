// =============================================================================
// PostgresInvoiceRepository Integration Tests
// =============================================================================
//
// Integration tests for PostgreSQL invoice repository operations running against
// a real database. Tests verify CRUD operations, status transitions, invoice
// number generation, seller config, and overdue candidate queries.
//
// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: Invoice CRUD, status transitions, number generation, seller config, overdue detection

use super::*;
use chrono::{Datelike, Duration, Utc};
use futures::FutureExt;
use herald_domain::billing::invoice::{
    ActorType, InvoiceEventType, InvoiceListFilters, InvoiceRepository, InvoiceStatus,
    InvoiceStatusTransition, NewInvoice, NewLineItem,
};
use herald_test_db::{SharedTestDatabaseHandle, create_isolated_schema_database};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use std::future::Future;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

// =============================================================================
// Test Database Setup
// =============================================================================

struct InvoiceTestDb {
    db: DatabaseConnection,
    pool: sqlx::PgPool,
    schema: SharedTestDatabaseHandle,
}

impl InvoiceTestDb {
    async fn teardown(self) {
        let InvoiceTestDb { db, pool, schema } = self;
        drop(db);
        drop(pool);
        schema.teardown().await;
    }
}

async fn setup_test_db() -> InvoiceTestDb {
    let (schema, pool, db) = create_isolated_schema_database(3).await;
    create_test_realms(&db).await;
    create_test_accounts(&db).await;
    InvoiceTestDb { db, pool, schema }
}

async fn create_test_realms(db: &DatabaseConnection) {
    let backend = db.get_database_backend();
    for realm_id in test_realm_ids() {
        let _ = db
            .execute(Statement::from_string(
                backend,
                format!(
                    "INSERT INTO realm (id, name) VALUES ('{}', 'Test Realm {}') ON CONFLICT (id) DO NOTHING",
                    realm_id, realm_id
                ),
            ))
            .await;
    }
}

/// Create test account records. The invoice table references account(id) for
/// account_id and applicant_user_id.
async fn create_test_accounts(db: &DatabaseConnection) {
    let backend = db.get_database_backend();
    for realm_id in test_realm_ids() {
        let account_id = test_account_id(realm_id);
        let _ = db
            .execute(Statement::from_string(
                backend,
                format!(
                    "INSERT INTO account (id, realm_id, email, status) VALUES ('{}', '{}', 'test@{}', 1) ON CONFLICT (id) DO NOTHING",
                    account_id, realm_id, realm_id
                ),
            ))
            .await;

        let user_id = test_user_id(realm_id);
        let _ = db
            .execute(Statement::from_string(
                backend,
                format!(
                    "INSERT INTO account (id, realm_id, email, status) VALUES ('{}', '{}', 'user@{}', 1) ON CONFLICT (id) DO NOTHING",
                    user_id, realm_id, realm_id
                ),
            ))
            .await;
    }
}

fn test_account_id(realm_id: &str) -> Uuid {
    deterministic_uuid(&format!("account:{realm_id}"))
}

fn test_user_id(realm_id: &str) -> Uuid {
    deterministic_uuid(&format!("user:{realm_id}"))
}

fn deterministic_uuid(seed: &str) -> Uuid {
    use sha2::{Digest, Sha256};
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&Sha256::digest(seed.as_bytes())[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // UUID v4 variant
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn test_realm_ids() -> Vec<&'static str> {
    vec![
        "inv_find",
        "inv_pagination",
        "inv_seq_realm_a",
        "inv_seq_realm_b",
        "inv_seller_notfound",
        "inv_overdue",
        "inv_overdue_excl",
        "inv_transition_hist",
        "inv_transition_ts",
    ]
}

async fn run_with_repo<F, Fut>(test_fn: F)
where
    F: FnOnce(PostgresInvoiceRepository) -> Fut,
    Fut: Future<Output = ()>,
{
    let test_db = setup_test_db().await;
    let repo = PostgresInvoiceRepository::new(test_db.db.clone());
    let result = AssertUnwindSafe(test_fn(repo)).catch_unwind().await;
    test_db.teardown().await;
    if let Err(panic_payload) = result {
        resume_unwind(panic_payload);
    }
}

// =============================================================================
// Test helper constructors
// =============================================================================

fn new_invoice(realm_id: &str, line_items: Vec<NewLineItem>) -> NewInvoice {
    let account_id = test_account_id(realm_id);
    NewInvoice {
        realm_id: realm_id.to_string(),
        source: herald_domain::billing::invoice::InvoiceSource::AdminManual,
        account_id,
        applicant_user_id: None,
        subscription_id: None,
        payment_attempt_id: None,
        currency: "USD".to_string(),
        line_items,
        actor_user_id: None,
        billing_name: "Test Buyer".to_string(),
        billing_address: "123 Main St".to_string(),
        billing_email: Some("buyer@example.com".to_string()),
        billing_phone: None,
        billing_tax_id: String::new(),
        seller_name: "Test Seller".to_string(),
        seller_address: "456 Corp Ave".to_string(),
        seller_email: Some("seller@example.com".to_string()),
        seller_phone: None,
        seller_tax_id: String::new(),
        discount_mode: None,
        discount_value: None,
        tax_mode: None,
        tax_value: None,
        shipping_mode: None,
        shipping_value: None,
        due_date: (Utc::now() + chrono::Duration::days(30)).date_naive(),
        payment_terms: None,
        notes: None,
    }
}

fn line_item(name: &str, quantity: &str, unit_price: i64) -> NewLineItem {
    NewLineItem {
        name: name.to_string(),
        description: Some(format!("Desc for {}", name)),
        quantity: quantity.to_string(),
        unit_price,
    }
}

// =============================================================================
// Macro for test declarations
// =============================================================================

macro_rules! invoice_repo_test {
    ($name:ident, |$repo:ident| $body:block) => {
        #[tokio::test]
        async fn $name() {
            run_with_repo(|$repo| async move $body).await;
        }
    };
}

// =============================================================================
// 1. CRUD Tests
// =============================================================================

// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: US-01 Create Invoice, US-02 View Invoice

// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: US-02 View Invoice Detail (with line items and history)

invoice_repo_test!(test_find_with_items, |repo| {
    let input = new_invoice("inv_find", vec![line_item("Item X", "3", 500)]);
    let created = repo.create_invoice(input).await.unwrap();

    let result = repo.find_with_items("inv_find", created.id).await;
    assert!(result.is_ok());

    let detail = result.unwrap();
    assert!(detail.is_some());
    let detail = detail.unwrap();

    // Verify invoice fields
    assert_eq!(detail.invoice.id, created.id);
    assert_eq!(detail.invoice.invoice_number, created.invoice_number);
    assert_eq!(detail.invoice.realm_id, "inv_find");

    // Verify line items
    assert_eq!(detail.line_items.len(), 1);
    assert_eq!(detail.line_items[0].name, "Item X");
    assert_eq!(detail.line_items[0].quantity, "3.000");
    assert_eq!(detail.line_items[0].unit_price, 500);
    // subtotal = 3 * 500 = 1500
    assert_eq!(detail.line_items[0].subtotal, 1500);

    // Verify history
    assert!(
        !detail.history.is_empty(),
        "History should contain created event"
    );
    let created_event = &detail.history[0];
    assert_eq!(created_event.event_type, InvoiceEventType::Created);
});

invoice_repo_test!(test_find_with_items_not_found, |repo| {
    let fake_id = Uuid::now_v7();
    let result = repo.find_with_items("inv_find", fake_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
});

invoice_repo_test!(test_pagination, |repo| {
    let realm = "inv_pagination";

    // Create 5 invoices
    for i in 0..5 {
        let input = new_invoice(realm, vec![line_item(&format!("Item {}", i), "1", 1000)]);
        repo.create_invoice(input).await.unwrap();
    }

    // Page 1 with page_size=2
    let filters = InvoiceListFilters {
        page: Some(1),
        page_size: Some(2),
        ..Default::default()
    };
    let result = repo.list_admin(realm, filters).await.unwrap();
    assert_eq!(result.total, 5, "Total should be 5");
    assert_eq!(result.page, 1);
    assert_eq!(result.page_size, 2);
    assert_eq!(result.data.len(), 2, "Page 1 should have 2 items");

    // Page 2
    let filters = InvoiceListFilters {
        page: Some(2),
        page_size: Some(2),
        ..Default::default()
    };
    let result = repo.list_admin(realm, filters).await.unwrap();
    assert_eq!(result.data.len(), 2, "Page 2 should have 2 items");

    // Page 3 (last page, only 1 item)
    let filters = InvoiceListFilters {
        page: Some(3),
        page_size: Some(2),
        ..Default::default()
    };
    let result = repo.list_admin(realm, filters).await.unwrap();
    assert_eq!(result.data.len(), 1, "Page 3 should have 1 item");
});

// =============================================================================
// 2. Invoice Number Counter Tests
// =============================================================================

// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: INV-01 Sequential number generation

invoice_repo_test!(test_sequential_number_generation, |repo| {
    let realm = "inv_seq_realm_a";

    let inv1 = repo
        .create_invoice(new_invoice(realm, vec![line_item("A", "1", 100)]))
        .await
        .unwrap();
    let inv2 = repo
        .create_invoice(new_invoice(realm, vec![line_item("B", "1", 200)]))
        .await
        .unwrap();
    let inv3 = repo
        .create_invoice(new_invoice(realm, vec![line_item("C", "1", 300)]))
        .await
        .unwrap();

    // All should have different invoice numbers within the same year
    assert_ne!(inv1.invoice_number, inv2.invoice_number);
    assert_ne!(inv2.invoice_number, inv3.invoice_number);

    // Numbers should follow INV-{YEAR}-{SEQ:04} pattern
    let year = Utc::now().year();
    assert!(inv1.invoice_number.starts_with(&format!("INV-{}-", year)));
    assert!(inv2.invoice_number.starts_with(&format!("INV-{}-", year)));
    assert!(inv3.invoice_number.starts_with(&format!("INV-{}-", year)));

    // Extract sequence numbers and verify they increment
    let seq1: i64 = inv1
        .invoice_number
        .rsplit('-')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let seq2: i64 = inv2
        .invoice_number
        .rsplit('-')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let seq3: i64 = inv3
        .invoice_number
        .rsplit('-')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(seq1 < seq2, "seq1 ({}) should be < seq2 ({})", seq1, seq2);
    assert!(seq2 < seq3, "seq2 ({}) should be < seq3 ({})", seq2, seq3);
});

// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: INV-01 Number independent across realms

invoice_repo_test!(test_number_independent_across_realms, |repo| {
    let realm_a = "inv_seq_realm_a";
    let realm_b = "inv_seq_realm_b";

    // Create one invoice in each realm
    let inv_a = repo
        .create_invoice(new_invoice(realm_a, vec![line_item("RA", "1", 100)]))
        .await
        .unwrap();
    let inv_b = repo
        .create_invoice(new_invoice(realm_b, vec![line_item("RB", "1", 200)]))
        .await
        .unwrap();

    // Both should have invoice number starting with seq 0001 since each realm
    // has its own counter
    let year = Utc::now().year();
    let expected_a = format!("INV-{}-0001", year);
    let expected_b = format!("INV-{}-0001", year);
    assert_eq!(
        inv_a.invoice_number, expected_a,
        "First invoice in realm_a should be 0001"
    );
    assert_eq!(
        inv_b.invoice_number, expected_b,
        "First invoice in realm_b should be 0001"
    );
});

// =============================================================================
// 3. Seller Config Tests
// =============================================================================

invoice_repo_test!(test_find_seller_config_not_found, |repo| {
    let result = repo.find_seller_config("inv_seller_notfound").await;
    assert!(result.is_ok());
    assert!(
        result.unwrap().is_none(),
        "Should return None for unconfigured realm"
    );
});

// =============================================================================
// 4. Overdue Candidate Tests
// =============================================================================

// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: US-06 Overdue detection

invoice_repo_test!(test_list_overdue_candidates, |repo| {
    let realm = "inv_overdue";
    let past_date = (Utc::now() - Duration::days(5)).date_naive();

    // Create and issue an invoice with a past due_date
    let mut input = new_invoice(realm, vec![line_item("Overdue Item", "1", 1000)]);
    input.due_date = past_date;
    let created = repo.create_invoice(input).await.unwrap();

    repo.transition_status(InvoiceStatusTransition {
        realm_id: realm.to_string(),
        invoice_id: created.id,
        target_status: InvoiceStatus::Issued,
        expected_current_status: InvoiceStatus::Draft,
        actor_user_id: None,
        actor_type: ActorType::User,
        void_reason: None,
        issue_date: None,
        paid_at: None,
    })
    .await
    .unwrap();

    let candidates = repo.list_overdue_candidates(Utc::now(), 10).await.unwrap();
    assert!(
        !candidates.is_empty(),
        "Should find at least one overdue candidate"
    );
    assert!(
        candidates.iter().any(|c| c.id == created.id),
        "The issued invoice with past due_date should be a candidate"
    );
});

invoice_repo_test!(test_excludes_non_issued_status, |repo| {
    let realm = "inv_overdue_excl";
    let past_date = (Utc::now() - Duration::days(5)).date_naive();

    // Create a draft invoice with past due_date -- should NOT be in overdue candidates
    let mut input = new_invoice(realm, vec![line_item("Draft Item", "1", 1000)]);
    input.due_date = past_date;
    let draft_inv = repo.create_invoice(input).await.unwrap();

    // Create and issue, then pay an invoice with past due_date -- should NOT be in overdue candidates
    let mut input2 = new_invoice(realm, vec![line_item("Paid Item", "1", 2000)]);
    input2.due_date = past_date;
    let paid_inv = repo.create_invoice(input2).await.unwrap();
    repo.transition_status(InvoiceStatusTransition {
        realm_id: realm.to_string(),
        invoice_id: paid_inv.id,
        target_status: InvoiceStatus::Issued,
        expected_current_status: InvoiceStatus::Draft,
        actor_user_id: None,
        actor_type: ActorType::User,
        void_reason: None,
        issue_date: None,
        paid_at: None,
    })
    .await
    .unwrap();
    repo.transition_status(InvoiceStatusTransition {
        realm_id: realm.to_string(),
        invoice_id: paid_inv.id,
        target_status: InvoiceStatus::Paid,
        expected_current_status: InvoiceStatus::Issued,
        actor_user_id: None,
        actor_type: ActorType::User,
        void_reason: None,
        issue_date: None,
        paid_at: None,
    })
    .await
    .unwrap();

    let candidates = repo.list_overdue_candidates(Utc::now(), 100).await.unwrap();
    assert!(
        !candidates.iter().any(|c| c.id == draft_inv.id),
        "Draft invoice should not be an overdue candidate"
    );
    assert!(
        !candidates.iter().any(|c| c.id == paid_inv.id),
        "Paid invoice should not be an overdue candidate"
    );
});

// =============================================================================
// 5. Transition Status Tests
// =============================================================================

// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: US-05 Status transitions with history

invoice_repo_test!(test_transition_records_history, |repo| {
    let realm = "inv_transition_hist";
    let input = new_invoice(realm, vec![line_item("Hist Item", "1", 1000)]);
    let created = repo.create_invoice(input).await.unwrap();

    // Transition draft -> issued
    let actor_user_id = Uuid::now_v7();
    let issued = repo
        .transition_status(InvoiceStatusTransition {
            realm_id: realm.to_string(),
            invoice_id: created.id,
            target_status: InvoiceStatus::Issued,
            expected_current_status: InvoiceStatus::Draft,
            actor_user_id: Some(actor_user_id),
            actor_type: ActorType::User,
            void_reason: None,
            issue_date: None,
            paid_at: None,
        })
        .await
        .unwrap();

    assert_eq!(issued.status, InvoiceStatus::Issued);

    // Verify history records
    let detail = repo
        .find_with_items(realm, created.id)
        .await
        .unwrap()
        .unwrap();
    let history_events: Vec<_> = detail.history.iter().map(|h| h.event_type).collect();
    assert!(
        history_events.contains(&InvoiceEventType::Created),
        "Should have 'created' event"
    );
    assert!(
        history_events.contains(&InvoiceEventType::Issued),
        "Should have 'issued' event"
    );

    // Verify the issued event has correct actor info
    let issued_event = detail
        .history
        .iter()
        .find(|h| h.event_type == InvoiceEventType::Issued)
        .unwrap();
    assert_eq!(issued_event.actor_user_id, Some(actor_user_id));
    assert_eq!(issued_event.actor_type, ActorType::User);

    // Verify changes field contains status transition info
    let changes = &issued_event.changes;
    assert_eq!(changes["from"], "draft");
    assert_eq!(changes["to"], "issued");
});

invoice_repo_test!(test_transition_updates_timestamps, |repo| {
    let realm = "inv_transition_ts";
    let input = new_invoice(realm, vec![line_item("TS Item", "1", 1000)]);
    let created = repo.create_invoice(input).await.unwrap();

    // Before issuing: issued_at should be None
    assert!(created.issued_at.is_none());
    assert!(created.issue_date.is_none());

    // Transition draft -> issued
    let issued = repo
        .transition_status(InvoiceStatusTransition {
            realm_id: realm.to_string(),
            invoice_id: created.id,
            target_status: InvoiceStatus::Issued,
            expected_current_status: InvoiceStatus::Draft,
            actor_user_id: None,
            actor_type: ActorType::User,
            void_reason: None,
            issue_date: None,
            paid_at: None,
        })
        .await
        .unwrap();

    // After issuing: issued_at and issue_date should be set
    assert!(
        issued.issued_at.is_some(),
        "issued_at should be set after issuing"
    );
    assert!(
        issued.issue_date.is_some(),
        "issue_date should be set after issuing"
    );
    assert!(issued.paid_at.is_none(), "paid_at should still be None");
    assert!(issued.voided_at.is_none(), "voided_at should still be None");

    // Transition issued -> paid
    let paid = repo
        .transition_status(InvoiceStatusTransition {
            realm_id: realm.to_string(),
            invoice_id: created.id,
            target_status: InvoiceStatus::Paid,
            expected_current_status: InvoiceStatus::Issued,
            actor_user_id: None,
            actor_type: ActorType::User,
            void_reason: None,
            issue_date: None,
            paid_at: None,
        })
        .await
        .unwrap();

    assert_eq!(paid.status, InvoiceStatus::Paid);
    assert!(
        paid.paid_at.is_some(),
        "paid_at should be set after payment"
    );
    // issued_at should remain
    assert!(
        paid.issued_at.is_some(),
        "issued_at should persist after payment"
    );
});

invoice_repo_test!(test_transition_void_sets_reason, |repo| {
    let realm = "inv_transition_ts";
    let input = new_invoice(realm, vec![line_item("Void Item", "1", 1000)]);
    let created = repo.create_invoice(input).await.unwrap();

    // Transition draft -> void
    let voided = repo
        .transition_status(InvoiceStatusTransition {
            realm_id: realm.to_string(),
            invoice_id: created.id,
            target_status: InvoiceStatus::Void,
            expected_current_status: InvoiceStatus::Draft,
            actor_user_id: None,
            actor_type: ActorType::User,
            void_reason: Some("Customer cancelled".to_string()),
            issue_date: None,
            paid_at: None,
        })
        .await
        .unwrap();

    assert_eq!(voided.status, InvoiceStatus::Void);
    assert!(voided.voided_at.is_some(), "voided_at should be set");
    assert_eq!(voided.void_reason, Some("Customer cancelled".to_string()));
});
