// =============================================================================
// Invoice Overdue Job Scenario Tests
// =============================================================================
//
// Tests for the InvoiceOverdueJob worker: marking past-due issued invoices as
// overdue, skipping non-eligible invoices, recording history with
// actor_type = system, batch processing, and idempotency.
//
// User Story: docs/user-stories/13-invoice-user-stories.md
// Covers: US-IV-009 (System marks overdue invoices)
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::schema_test_context::SchemaTestContext;
    use chrono::{Duration, Utc};
    use herald_core::domain::billing::invoice::{
        ActorType, InvoiceEventType, InvoiceRepository, InvoiceSource, InvoiceStatus,
        InvoiceStatusTransition, NewInvoice, NewLineItem,
    };
    use test_context::test_context;
    use uuid::Uuid;

    use SchemaTestContext as InvoiceTestContext;

    // ---------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------

    /// Create a test account row in the current test schema.
    async fn ensure_test_account(ctx: &InvoiceTestContext, realm_id: &str) -> Uuid {
        let account_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(account_id)
        .bind(realm_id)
        .bind(format!("overdue-test-{}@example.com", account_id))
        .bind("$2a$12$dummy_hash")
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        account_id
    }

    /// Build a NewInvoice with sensible defaults for overdue testing.
    fn build_new_invoice(
        realm_id: &str,
        account_id: Uuid,
        due_date: chrono::NaiveDate,
    ) -> NewInvoice {
        NewInvoice {
            realm_id: realm_id.to_string(),
            source: InvoiceSource::AdminManual,
            account_id,
            applicant_user_id: None,
            subscription_id: None,
            payment_attempt_id: None,
            currency: "USD".to_string(),
            line_items: vec![NewLineItem {
                name: "Service Fee".to_string(),
                description: None,
                quantity: "1".to_string(),
                unit_price: 10000,
            }],
            actor_user_id: None,
            billing_name: "Overdue Test Client".to_string(),
            billing_address: "123 Test St".to_string(),
            billing_email: None,
            billing_phone: None,
            billing_tax_id: String::new(),
            seller_name: "Test Seller".to_string(),
            seller_address: "456 Seller Ave".to_string(),
            seller_email: None,
            seller_phone: None,
            seller_tax_id: String::new(),
            discount_mode: None,
            discount_value: None,
            tax_mode: None,
            tax_value: None,
            shipping_mode: None,
            shipping_value: None,
            due_date,
            payment_terms: None,
            notes: None,
        }
    }

    // =========================================================================
    // Test 1: Overdue job marks past-due issued invoices as overdue
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 Scenario 1
    //
    // Given: An issued invoice with a past due_date
    // When:  The overdue job runs
    // Then:  The invoice status becomes "overdue"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_overdue_job_marks_past_due_issued_invoices(ctx: &mut InvoiceTestContext) {
        let realm_id = ctx._realm_id.clone();
        let account_id = ensure_test_account(ctx, &realm_id).await;
        let repo = ctx.app_state.invoice_repository.clone();

        // Create and issue an invoice with a past due_date (5 days ago)
        let past_date = (Utc::now() - Duration::days(5)).date_naive();
        let input = build_new_invoice(&realm_id, account_id, past_date);
        let created = repo.create_invoice(input).await.unwrap();

        // Issue the invoice
        repo.transition_status(InvoiceStatusTransition {
            realm_id: realm_id.clone(),
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

        // Run the overdue job
        let job = herald_worker::InvoiceOverdueJob::new(repo.clone());
        let result = job.run().await.unwrap();

        // The job should have found and processed our invoice
        assert!(
            result.candidates >= 1,
            "Expected at least 1 overdue candidate, got {}",
            result.candidates,
        );
        assert!(
            result.marked >= 1,
            "Expected at least 1 invoice marked overdue, got {}",
            result.marked,
        );

        // Verify the invoice status is now overdue
        let detail = repo
            .find_with_items(&realm_id, created.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            detail.invoice.status,
            InvoiceStatus::Overdue,
            "Invoice should be overdue after job runs"
        );
    }

    // =========================================================================
    // Test 2: Overdue job skips non-eligible invoices
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 Scenario 2 (skip non-eligible)
    //
    // Verifies that the overdue job does NOT change invoices that are:
    // - paid (past due_date, but already paid)
    // - void (past due_date, but voided)
    // - draft (past due_date, but never issued)
    // - issued with a future due_date (not yet past due)

    struct SkipCase {
        label: &'static str,
        initial_status: InvoiceStatus,
        due_date_offset_days: i64,
        void_reason: Option<&'static str>,
    }

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_overdue_job_skips_non_eligible_invoices(ctx: &mut InvoiceTestContext) {
        let cases = [
            SkipCase {
                label: "paid",
                initial_status: InvoiceStatus::Paid,
                due_date_offset_days: -5,
                void_reason: None,
            },
            SkipCase {
                label: "void",
                initial_status: InvoiceStatus::Void,
                due_date_offset_days: -5,
                void_reason: Some("Cancelled"),
            },
            SkipCase {
                label: "draft",
                initial_status: InvoiceStatus::Draft,
                due_date_offset_days: -5,
                void_reason: None,
            },
            SkipCase {
                label: "future due_date",
                initial_status: InvoiceStatus::Issued,
                due_date_offset_days: 30,
                void_reason: None,
            },
        ];

        let realm_id = ctx._realm_id.clone();
        let repo = ctx.app_state.invoice_repository.clone();

        for case in &cases {
            let account_id = ensure_test_account(ctx, &realm_id).await;
            let due_date = (Utc::now() + Duration::days(case.due_date_offset_days)).date_naive();
            let input = build_new_invoice(&realm_id, account_id, due_date);
            let created = repo.create_invoice(input).await.unwrap();

            // Transition to the required status (skip draft -- already draft)
            if case.initial_status != InvoiceStatus::Draft {
                repo.transition_status(InvoiceStatusTransition {
                    realm_id: realm_id.clone(),
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

                if case.initial_status == InvoiceStatus::Paid {
                    repo.transition_status(InvoiceStatusTransition {
                        realm_id: realm_id.clone(),
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
                } else if case.initial_status == InvoiceStatus::Void {
                    repo.transition_status(InvoiceStatusTransition {
                        realm_id: realm_id.clone(),
                        invoice_id: created.id,
                        target_status: InvoiceStatus::Void,
                        expected_current_status: InvoiceStatus::Issued,
                        actor_user_id: None,
                        actor_type: ActorType::User,
                        void_reason: case.void_reason.map(|s| s.to_string()),
                        issue_date: None,
                        paid_at: None,
                    })
                    .await
                    .unwrap();
                }
            }

            // Run the overdue job
            let job = herald_worker::InvoiceOverdueJob::new(repo.clone());
            let _result = job.run().await.unwrap();

            // Verify the invoice status is unchanged
            let detail = repo
                .find_with_items(&realm_id, created.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                detail.invoice.status, case.initial_status,
                "{} invoice should not be changed by overdue job",
                case.label,
            );
        }
    }

    // =========================================================================
    // Test 6: Overdue job records history with actor_type = system
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 Scenario 1 (history entry with actor_type = system)
    //
    // Given: An issued invoice with a past due_date
    // When:  The overdue job runs
    // Then:  A history entry is recorded with event_type = "overdue" and
    //        actor_type = "system"

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_overdue_job_records_history(ctx: &mut InvoiceTestContext) {
        let realm_id = ctx._realm_id.clone();
        let account_id = ensure_test_account(ctx, &realm_id).await;
        let repo = ctx.app_state.invoice_repository.clone();

        // Create and issue an invoice with a past due_date
        let past_date = (Utc::now() - Duration::days(3)).date_naive();
        let input = build_new_invoice(&realm_id, account_id, past_date);
        let created = repo.create_invoice(input).await.unwrap();

        repo.transition_status(InvoiceStatusTransition {
            realm_id: realm_id.clone(),
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

        // Run the overdue job
        let job = herald_worker::InvoiceOverdueJob::new(repo.clone());
        let _result = job.run().await.unwrap();

        // Verify history contains an "overdue" event with actor_type = system
        let detail = repo
            .find_with_items(&realm_id, created.id)
            .await
            .unwrap()
            .unwrap();

        let overdue_event = detail
            .history
            .iter()
            .find(|h| h.event_type == InvoiceEventType::Overdue);
        assert!(
            overdue_event.is_some(),
            "Expected an 'overdue' event in history"
        );

        let event = overdue_event.unwrap();
        assert_eq!(
            event.actor_type,
            ActorType::System,
            "Overdue event should have actor_type = System"
        );
        assert!(
            event.actor_user_id.is_none(),
            "System overdue event should have no actor_user_id"
        );

        // Verify the changes field records the status transition
        let changes = &event.changes;
        assert_eq!(changes["from"], "issued");
        assert_eq!(changes["to"], "overdue");
    }

    // =========================================================================
    // Test 7: Overdue job batch processing
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 (batch processing: multiple invoices processed in one run)
    //
    // Given: 3 issued invoices with past due_dates and 1 issued with future date
    // When:  The overdue job runs
    // Then:  Exactly the 3 past-due invoices are marked overdue;
    //        the future-dated one remains issued

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_overdue_job_batch_processing(ctx: &mut InvoiceTestContext) {
        let realm_id = ctx._realm_id.clone();
        let account_id = ensure_test_account(ctx, &realm_id).await;
        let repo = ctx.app_state.invoice_repository.clone();

        let past_date = (Utc::now() - Duration::days(5)).date_naive();
        let future_date = (Utc::now() + Duration::days(30)).date_naive();

        let mut past_due_ids: Vec<Uuid> = Vec::new();

        // Create 3 issued invoices with past due_dates
        for _ in 0..3 {
            let input = build_new_invoice(&realm_id, account_id, past_date);
            let created = repo.create_invoice(input).await.unwrap();

            repo.transition_status(InvoiceStatusTransition {
                realm_id: realm_id.clone(),
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

            past_due_ids.push(created.id);
        }

        // Create 1 issued invoice with a future due_date (should be skipped)
        let input = build_new_invoice(&realm_id, account_id, future_date);
        let future_inv = repo.create_invoice(input).await.unwrap();

        repo.transition_status(InvoiceStatusTransition {
            realm_id: realm_id.clone(),
            invoice_id: future_inv.id,
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

        let future_due_id = future_inv.id;

        // Run the overdue job
        let job = herald_worker::InvoiceOverdueJob::new(repo.clone());
        let result = job.run().await.unwrap();

        // Verify batch result
        assert!(
            result.candidates >= 3,
            "Expected at least 3 candidates, got {}",
            result.candidates,
        );
        assert!(
            result.marked >= 3,
            "Expected at least 3 marked, got {}",
            result.marked,
        );
        assert_eq!(result.errors, 0, "Expected 0 errors in batch processing");

        // Verify each past-due invoice is now overdue
        for id in &past_due_ids {
            let detail = repo.find_with_items(&realm_id, *id).await.unwrap().unwrap();
            assert_eq!(
                detail.invoice.status,
                InvoiceStatus::Overdue,
                "Past-due invoice {} should be overdue",
                id
            );
        }

        // Verify the future-dated invoice is still issued
        let future_detail = repo
            .find_with_items(&realm_id, future_due_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            future_detail.invoice.status,
            InvoiceStatus::Issued,
            "Future-dated invoice should remain issued"
        );
    }

    // =========================================================================
    // Test 8: Overdue job is idempotent
    // =========================================================================
    // User Story: docs/user-stories/13-invoice-user-stories.md
    // Covers: US-IV-009 (idempotency: running twice does not cause errors or
    //         double transitions)
    //
    // Given: An issued invoice with a past due_date
    // When:  The overdue job runs twice
    // Then:  The first run marks it overdue; the second run finds 0 candidates
    //        for this invoice (it is no longer status=issued)

    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_overdue_job_idempotent(ctx: &mut InvoiceTestContext) {
        let realm_id = ctx._realm_id.clone();
        let account_id = ensure_test_account(ctx, &realm_id).await;
        let repo = ctx.app_state.invoice_repository.clone();

        // Create and issue an invoice with a past due_date
        let past_date = (Utc::now() - Duration::days(7)).date_naive();
        let input = build_new_invoice(&realm_id, account_id, past_date);
        let created = repo.create_invoice(input).await.unwrap();

        repo.transition_status(InvoiceStatusTransition {
            realm_id: realm_id.clone(),
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

        // First run: should mark the invoice as overdue
        let job = herald_worker::InvoiceOverdueJob::new(repo.clone());
        let result1 = job.run().await.unwrap();
        assert!(
            result1.marked >= 1,
            "First run should mark at least 1 invoice"
        );

        // Verify it is overdue after first run
        let detail1 = repo
            .find_with_items(&realm_id, created.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(detail1.invoice.status, InvoiceStatus::Overdue);

        // Count overdue history events after first run
        let overdue_events_after_run1 = detail1
            .history
            .iter()
            .filter(|h| h.event_type == InvoiceEventType::Overdue)
            .count();

        // Second run: should find 0 candidates for this invoice (already overdue)
        let result2 = job.run().await.unwrap();
        // The second run should succeed without errors
        assert_eq!(result2.errors, 0, "Second run should not produce errors");

        // Verify the invoice is still overdue and no duplicate history entries
        let detail2 = repo
            .find_with_items(&realm_id, created.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            detail2.invoice.status,
            InvoiceStatus::Overdue,
            "Invoice should still be overdue after second run"
        );

        let overdue_events_after_run2 = detail2
            .history
            .iter()
            .filter(|h| h.event_type == InvoiceEventType::Overdue)
            .count();
        assert_eq!(
            overdue_events_after_run1, overdue_events_after_run2,
            "Second run should not add duplicate overdue history entries"
        );
    }

    // =========================================================================
    // Regression (audit run-1: mark-overdue-by-system-missing-status-precondition)
    // =========================================================================

    /// Paid/Void 是发票终态：overdue 扫描列出候选（status='issued'）与逐行
    /// UPDATE 之间的 check-then-act 窗口里，一张已并发提交为 paid 的发票
    /// 绝不能被系统改写成 overdue。回归：以扫描的期望状态（issued）对一张
    /// 已 paid 的发票执行 overdue 写入必须 Conflict（旧代码直接覆盖成功），
    /// 且终态之后 void 也必须被拒。
    #[test_context(InvoiceTestContext)]
    #[tokio::test]
    async fn test_overdue_write_rejected_after_paid_terminal_state(ctx: &mut InvoiceTestContext) {
        let realm_id = ctx._realm_id.clone();
        let account_id = ensure_test_account(ctx, &realm_id).await;
        let repo = ctx.app_state.invoice_repository.clone();

        // Create + issue an invoice with a past due_date (sweep candidate).
        let past_date = (Utc::now() - Duration::days(5)).date_naive();
        let input = build_new_invoice(&realm_id, account_id, past_date);
        let created = repo.create_invoice(input).await.unwrap();
        repo.transition_status(InvoiceStatusTransition {
            realm_id: realm_id.clone(),
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

        // The candidate gets paid between the sweep listing and its write.
        repo.transition_status(InvoiceStatusTransition {
            realm_id: realm_id.clone(),
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

        // The sweep's write carries the stale expectation (listed as issued)
        // and must be refused, not overwrite the terminal paid state.
        let stale = repo
            .transition_status(InvoiceStatusTransition {
                realm_id: realm_id.clone(),
                invoice_id: created.id,
                target_status: InvoiceStatus::Overdue,
                expected_current_status: InvoiceStatus::Issued,
                actor_user_id: None,
                actor_type: ActorType::System,
                void_reason: None,
                issue_date: None,
                paid_at: None,
            })
            .await;
        assert!(
            matches!(
                stale,
                Err(herald_core::domain::common::entities::app_errors::CoreError::Conflict(_))
            ),
            "overdue write against a paid invoice must Conflict, got {stale:?}"
        );

        // Terminal state holds: the invoice is still paid, and a later void
        // with the stale issued expectation is refused too.
        let detail = repo
            .find_with_items(&realm_id, created.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(detail.invoice.status, InvoiceStatus::Paid);

        let void_attempt = repo
            .transition_status(InvoiceStatusTransition {
                realm_id: realm_id.clone(),
                invoice_id: created.id,
                target_status: InvoiceStatus::Void,
                expected_current_status: InvoiceStatus::Issued,
                actor_user_id: None,
                actor_type: ActorType::User,
                void_reason: Some("late void".to_string()),
                issue_date: None,
                paid_at: None,
            })
            .await;
        assert!(
            matches!(
                void_attempt,
                Err(herald_core::domain::common::entities::app_errors::CoreError::Conflict(_))
            ),
            "void write against a paid invoice must Conflict, got {void_attempt:?}"
        );
    }
}
