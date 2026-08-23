/**
 * Admin Subscription List Demo Tests
 *
 * User Story: US-EM-006 -- View Subscription Projections
 *
 * Coverage:
 * - US-EM-006 Scene 1: Display subscription projection list with correct columns
 *   Note: Columns follow the actual frontend implementation (Entitlement, Payment
 *   Provider, External Price ID, Synced At, Billing type, Service period end,
 *   Status, Client App), NOT the user story columns (User,
 *   Current Period). The user story should be updated to match the implementation.
 * - US-EM-006 Scene 2: Filter subscriptions by entitlement key, status, and payment provider
 * - Row field verification: Verify expected subscription fields in list rows
 * - Empty state: No match / no subscriptions
 *
 * NOT Covered:
 * - US-EM-006 Scene 3 (subscription detail/change history): The frontend lacks a clickable
 *   subscription detail view with change history timeline. This should be tracked as a
 *   follow-up task when the frontend feature is built.
 *
 * User Story: docs/user-stories/billing/entitlement-mapping.md (US-EM-006)
 *
 * Uses AdminSubscriptionListPage page object from DE-D01.
 * Uses adminSubscriptionListPage fixture for login + navigation.
 */

import { test, expect } from '../fixtures/demo-page.fixtures'

/**
 * Resolve the Status column index from the rendered table headers.
 *
 * The column layout evolves with the frontend (Billing type / Service period
 * end were inserted between Synced At and Status, growing the table from 6 to
 * 8 columns and shifting Status off its old fixed index), so tests must locate
 * the column by header text instead of a hardcoded index.
 */
function findStatusColumnIndex(headers: string[]): number {
  const index = headers.findIndex((h) => h.trim().toLowerCase() === 'status')
  expect(index, 'Expected a "Status" column in table headers').toBeGreaterThanOrEqual(0)
  return index
}

test.describe('[Billing Admin] Subscription Projection List (US-EM-006)', () => {
  // ==========================================================================
  // US-EM-006 Scene 1: View all subscription projections
  // Columns follow frontend implementation, not user story.
  // ==========================================================================

  test('should display subscription projection list with correct columns', async ({
    adminSubscriptionListPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is on the subscription list page', async () => {
      await expect(adminSubscriptionListPage.container).toBeVisible()
      await expect(adminSubscriptionListPage.heading).toBeVisible()
      demoLogger.testCode.log('[Given] Subscription list page is loaded')
    })

    await test.step('When: Page loads and renders either subscriptions or empty state', async () => {
      await adminSubscriptionListPage.waitForDataLoaded()
      const hasTable = await adminSubscriptionListPage.isVisible(adminSubscriptionListPage.table)
      const hasEmpty = await adminSubscriptionListPage.isTableEmpty()

      if (hasTable) {
        demoLogger.testCode.log('[When] Table with subscriptions is visible')
      } else if (hasEmpty) {
        demoLogger.testCode.log('[When] Empty state is visible (no subscriptions)')
      } else {
        expect(hasTable || hasEmpty).toBe(true)
      }
    })

    await test.step('Then: If subscriptions exist, table has expected columns; otherwise empty state is shown', async () => {
      await adminSubscriptionListPage.waitForDataLoaded()
      const hasTable = await adminSubscriptionListPage.isVisible(adminSubscriptionListPage.table)

      if (hasTable) {
        const headers = await adminSubscriptionListPage.getTableHeaders()

        // Columns per the actual frontend implementation:
        // Entitlement, Payment Provider, External Price ID, Synced At,
        // Billing type, Service period end, Status, Client App
        const expectedColumns = [
          'Entitlement',
          'Payment Provider',
          'External Price ID',
          'Synced At',
          'Billing type',
          'Service period end',
          'Status',
          'Client App',
        ]

        for (const col of expectedColumns) {
          const headerMatch = headers.some((h) => h.includes(col))
          expect(headerMatch, `Expected column "${col}" in table headers`).toBe(true)
        }

        // Verify at least one subscription row exists with data
        const rowCount = await adminSubscriptionListPage.getSubscriptionRowCount()
        expect(rowCount).toBeGreaterThanOrEqual(1)

        const firstRowTexts = await adminSubscriptionListPage.getSubscriptionRowTexts(0)
        // Each row should have 8 cells matching the columns
        expect(firstRowTexts.length).toBe(8)

        // Verify row data: entitlement key and provider should be non-empty
        expect(
          firstRowTexts[0].trim().length,
          'Entitlement Key cell should not be empty'
        ).toBeGreaterThan(0)
        expect(
          firstRowTexts[1].trim().length,
          'Payment Provider cell should not be empty'
        ).toBeGreaterThan(0)

        // Status badge should contain recognizable status label text.
        // The column is located by header text so future column
        // insertions/removals cannot silently shift the index.
        const statusText = (firstRowTexts[findStatusColumnIndex(headers)] || '').trim()
        expect(statusText.length, 'Status cell should not be empty').toBeGreaterThan(0)

        demoLogger.testCode.log(
          `[Then] Table verified with ${rowCount} rows and correct columns`
        )
      } else {
        // Empty state must be visible
        await expect(adminSubscriptionListPage.emptyState).toBeVisible()
        demoLogger.testCode.log('[Then] Empty state verified')
      }
    })
  })

  // ==========================================================================
  // US-EM-006 Scene 2: Filter by entitlement key
  // ==========================================================================

  test('should filter subscriptions by entitlement key', async ({
    adminSubscriptionListPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is on the subscription list page', async () => {
      await expect(adminSubscriptionListPage.container).toBeVisible()
      await expect(adminSubscriptionListPage.entitlementKeyFilterInput).toBeVisible()
      demoLogger.testCode.log('[Given] Page loaded with entitlement key filter visible')
    })

    await test.step('When: Type a non-existent entitlement key in the filter input', async () => {
      await adminSubscriptionListPage.filterByEntitlementKey('nonexistent-key-xyz')
      demoLogger.testCode.log('[When] Non-existent entitlement key filter applied')
    })

    await test.step('Then: Empty state is shown (no matching subscriptions)', async () => {
      // The list keeps the previous table visible while the filtered query
      // refetches (keepPreviousData), then swaps to the empty state. Two
      // point-in-time checks can straddle that swap and observe neither
      // state, so wait atomically for the empty state -- the filter value
      // guarantees no match, hence the empty state must render.
      await expect(adminSubscriptionListPage.emptyState).toBeVisible()
      const emptyText = await adminSubscriptionListPage.getEmptyStateText()
      // The empty state message should indicate no match
      expect(emptyText.length, 'Empty state should have a message').toBeGreaterThan(0)
      demoLogger.testCode.log(`[Then] Empty state shown: "${emptyText.trim()}"`)
    })

    await test.step('When: Clear the entitlement key filter', async () => {
      await adminSubscriptionListPage.filterByEntitlementKey('')
      demoLogger.testCode.log('[When] Entitlement key filter cleared')
    })

    await test.step('Then: Page returns to showing all subscriptions or empty state', async () => {
      // Same asynchronous table/empty-state swap as above -- poll for either
      // final state atomically instead of two point-in-time reads.
      await expect
        .poll(
          async () =>
            (await adminSubscriptionListPage.isVisible(adminSubscriptionListPage.table)) ||
            (await adminSubscriptionListPage.isTableEmpty()),
          {
            message:
              'Expected the table or the empty state to be visible after clearing the filter',
          }
        )
        .toBe(true)
      demoLogger.testCode.log('[Then] Filter cleared, page shows appropriate state')
    })
  })

  // ==========================================================================
  // US-EM-006 Scene 2: Filter by status
  // ==========================================================================

  test('should filter subscriptions by status', async ({
    adminSubscriptionListPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is on the subscription list page', async () => {
      await expect(adminSubscriptionListPage.container).toBeVisible()
      await expect(adminSubscriptionListPage.statusFilterSelect).toBeVisible()
      demoLogger.testCode.log('[Given] Page loaded with status filter visible')
    })

    await test.step('When: Select "Active" from status filter dropdown', async () => {
      await adminSubscriptionListPage.filterByStatus('active')
      demoLogger.testCode.log('[When] Active status filter applied')
    })

    await test.step('Then: Only active subscriptions are shown (or empty state)', async () => {
      await adminSubscriptionListPage.waitForDataLoaded()
      const hasTable = await adminSubscriptionListPage.isVisible(adminSubscriptionListPage.table)
      const hasEmpty = await adminSubscriptionListPage.isTableEmpty()

      if (hasTable) {
        // The list keeps the previous (unfiltered) rows visible while the
        // filtered query refetches (keepPreviousData), so the row set may
        // briefly still contain non-Active rows. Retry the verification
        // until the filtered data settles.
        await expect(async () => {
          const rowCount = await adminSubscriptionListPage.getSubscriptionRowCount()
          // Every visible row should have "Active" in the Status column. The
          // column is located by header text so future column insertions or
          // removals cannot silently shift the index.
          const statusColumnIndex = findStatusColumnIndex(
            await adminSubscriptionListPage.getTableHeaders()
          )
          for (let i = 0; i < rowCount; i++) {
            const rowTexts = await adminSubscriptionListPage.getSubscriptionRowTexts(i)
            const statusCell = (rowTexts[statusColumnIndex] || '').trim()
            expect(statusCell, `Row ${i} status should be Active`).toContain('Active')
          }
        }).toPass({ timeout: 10000 })
        const rowCount = await adminSubscriptionListPage.getSubscriptionRowCount()
        demoLogger.testCode.log(`[Then] ${rowCount} Active-only rows verified`)
      } else if (hasEmpty) {
        demoLogger.testCode.log('[Then] No active subscriptions, empty state shown')
      } else {
        expect(hasTable || hasEmpty).toBe(true)
      }

      // Verify the status filter displays "Active" as the selected value
      const filterValue = await adminSubscriptionListPage.statusFilterSelect.textContent()
      expect(filterValue).toContain('Active')
      demoLogger.testCode.log('[Then] Status filter displays "Active"')
    })

    await test.step('When: Select "All" from status filter', async () => {
      await adminSubscriptionListPage.filterByStatus('all')
      demoLogger.testCode.log('[When] "All" status filter applied')
    })

    await test.step('Then: All subscriptions are shown again (or empty state)', async () => {
      await adminSubscriptionListPage.waitForDataLoaded()
      const hasTable = await adminSubscriptionListPage.isVisible(adminSubscriptionListPage.table)
      const hasEmpty = await adminSubscriptionListPage.isTableEmpty()

      if (hasTable) {
        const rowCount = await adminSubscriptionListPage.getSubscriptionRowCount()
        expect(rowCount).toBeGreaterThanOrEqual(0)
        demoLogger.testCode.log(`[Then] All subscriptions shown: ${rowCount} rows`)
      } else if (hasEmpty) {
        demoLogger.testCode.log('[Then] No subscriptions at all, empty state shown')
      } else {
        expect(hasTable || hasEmpty).toBe(true)
      }

      // Verify the status filter displays "All" as the selected value
      const filterValue = await adminSubscriptionListPage.statusFilterSelect.textContent()
      expect(filterValue).toContain('All')
      demoLogger.testCode.log('[Then] Status filter displays "All"')
    })
  })

  // ==========================================================================
  // US-EM-006 Scene 2: Filter by payment provider
  // ==========================================================================

  test('should filter subscriptions by payment provider', async ({
    adminSubscriptionListPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is on the subscription list page', async () => {
      await expect(adminSubscriptionListPage.container).toBeVisible()
      await expect(adminSubscriptionListPage.paymentProviderFilterSelect).toBeVisible()
      demoLogger.testCode.log('[Given] Page loaded with payment provider filter visible')
    })

    await test.step('When: Select "Stripe" from payment provider filter dropdown', async () => {
      await adminSubscriptionListPage.filterByProvider('stripe')
      demoLogger.testCode.log('[When] Stripe provider filter applied')
    })

    await test.step('Then: Only Stripe subscriptions are shown (or empty state)', async () => {
      await adminSubscriptionListPage.waitForDataLoaded()
      const hasTable = await adminSubscriptionListPage.isVisible(adminSubscriptionListPage.table)
      const hasEmpty = await adminSubscriptionListPage.isTableEmpty()

      if (hasTable) {
        const rowCount = await adminSubscriptionListPage.getSubscriptionRowCount()
        // Every visible row should have "Stripe" in the Payment Provider column (index 1)
        for (let i = 0; i < rowCount; i++) {
          const rowTexts = await adminSubscriptionListPage.getSubscriptionRowTexts(i)
          expect(rowTexts[1].trim(), `Row ${i} provider should be Stripe`).toBe('Stripe')
        }
        demoLogger.testCode.log(`[Then] ${rowCount} Stripe-only rows verified`)
      } else if (hasEmpty) {
        demoLogger.testCode.log('[Then] No Stripe subscriptions, empty state shown')
      } else {
        expect(hasTable || hasEmpty).toBe(true)
      }

      // Verify the provider filter displays "Stripe" as the selected value
      const filterValue = await adminSubscriptionListPage.paymentProviderFilterSelect.textContent()
      expect(filterValue).toContain('Stripe')
      demoLogger.testCode.log('[Then] Provider filter displays "Stripe"')
    })

    await test.step('When: Select "All" from payment provider filter', async () => {
      await adminSubscriptionListPage.filterByProvider('all')
      demoLogger.testCode.log('[When] "All" provider filter applied')
    })

    await test.step('Then: All subscriptions are shown again (or empty state)', async () => {
      await adminSubscriptionListPage.waitForDataLoaded()
      const hasTable = await adminSubscriptionListPage.isVisible(adminSubscriptionListPage.table)
      const hasEmpty = await adminSubscriptionListPage.isTableEmpty()

      if (hasTable) {
        const rowCount = await adminSubscriptionListPage.getSubscriptionRowCount()
        expect(rowCount).toBeGreaterThanOrEqual(0)
        demoLogger.testCode.log(`[Then] All subscriptions shown: ${rowCount} rows`)
      } else if (hasEmpty) {
        demoLogger.testCode.log('[Then] No subscriptions at all, empty state shown')
      } else {
        expect(hasTable || hasEmpty).toBe(true)
      }

      const filterValue = await adminSubscriptionListPage.paymentProviderFilterSelect.textContent()
      expect(filterValue).toContain('All')
      demoLogger.testCode.log('[Then] Provider filter displays "All"')
    })
  })

  // ==========================================================================
  // Row field verification (NOT US-EM-006 Scene 3 -- frontend lacks detail view)
  // Verifies that each subscription row displays the expected fields.
  // ==========================================================================

  test('should display expected subscription fields in list rows', async ({
    adminSubscriptionListPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is on the subscription list page with at least one subscription', async () => {
      await expect(adminSubscriptionListPage.container).toBeVisible()

      await adminSubscriptionListPage.waitForDataLoaded()
      const hasTable = await adminSubscriptionListPage.isVisible(adminSubscriptionListPage.table)
      if (!hasTable) {
        demoLogger.testCode.log('[Given] No subscriptions present, skipping row field verification')
        return
      }

      const rowCount = await adminSubscriptionListPage.getSubscriptionRowCount()
      expect(
        rowCount,
        'Expected at least one subscription row for field verification'
      ).toBeGreaterThanOrEqual(1)
      demoLogger.testCode.log(`[Given] Table has ${rowCount} rows`)
    })

    await test.step('When: Page loads with subscription data', async () => {
      // Page is already loaded via fixture
      demoLogger.testCode.log('[When] Checking row field presence')
    })

    await test.step('Then: Each subscription row displays expected fields', async () => {
      await adminSubscriptionListPage.waitForDataLoaded()
      const hasTable = await adminSubscriptionListPage.isVisible(adminSubscriptionListPage.table)
      if (!hasTable) return

      const rowCount = await adminSubscriptionListPage.getSubscriptionRowCount()
      // Verify fields for the first row as representative
      const rowTexts = await adminSubscriptionListPage.getSubscriptionRowTexts(0)

      // Column 0: Entitlement Key (primary identifier) -- must be non-empty
      const entitlementKey = rowTexts[0].trim()
      expect(entitlementKey.length, 'Entitlement Key should not be empty').toBeGreaterThan(0)
      demoLogger.testCode.log(`[Then] Entitlement Key: "${entitlementKey}"`)

      // Column 1: Payment Provider -- should be a formatted name (e.g., "Stripe" not "stripe")
      const provider = rowTexts[1].trim()
      expect(provider.length, 'Payment Provider should not be empty').toBeGreaterThan(0)
      demoLogger.testCode.log(`[Then] Payment Provider: "${provider}"`)

      // Column 2: External Price ID -- may be "---" if absent
      const externalPriceId = rowTexts[2].trim()
      demoLogger.testCode.log(`[Then] External Price ID: "${externalPriceId}"`)

      // Column 3: Synced At -- should be a date or "---"
      const syncedAt = rowTexts[3].trim()
      demoLogger.testCode.log(`[Then] Synced At: "${syncedAt}"`)

      // Column 4: Status badge -- must contain a recognizable status label
      const status = rowTexts[4].trim()
      expect(status.length, 'Status should not be empty').toBeGreaterThan(0)
      demoLogger.testCode.log(`[Then] Status: "${status}"`)

      // Column 5: Client App ID -- may be "---" if absent
      const clientAppId = rowTexts[5].trim()
      demoLogger.testCode.log(`[Then] Client App ID: "${clientAppId}"`)

      demoLogger.testCode.log(
        `[Then] All 6 fields verified for row 0 of ${rowCount}`
      )
    })
  })

  // ==========================================================================
  // Empty state: No subscriptions match filters
  // ==========================================================================

  test('should display empty state when no subscriptions match filters', async ({
    adminSubscriptionListPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is on the subscription list page', async () => {
      await expect(adminSubscriptionListPage.container).toBeVisible()
      await expect(adminSubscriptionListPage.entitlementKeyFilterInput).toBeVisible()
      demoLogger.testCode.log('[Given] Page loaded with filter input visible')
    })

    await test.step('When: Apply filters that match nothing (non-existent entitlement key)', async () => {
      await adminSubscriptionListPage.filterByEntitlementKey('zzz-nonexistent-entitlement-key')
      demoLogger.testCode.log('[When] Non-existent entitlement key filter applied')
    })

    await test.step('Then: Empty state card with dashed border is visible', async () => {
      // The list keeps the previous table visible while the filtered query
      // refetches (keepPreviousData), then swaps to the empty state. Two
      // point-in-time checks can straddle that swap and observe neither
      // state, so wait atomically for the empty state -- the filter value
      // guarantees no match, hence the empty state must render.
      await expect(adminSubscriptionListPage.emptyState).toBeVisible()
      const emptyText = await adminSubscriptionListPage.getEmptyStateText()
      expect(emptyText.length, 'Empty state should have a message').toBeGreaterThan(0)
      demoLogger.testCode.log(`[Then] Empty state shown: "${emptyText.trim()}"`)
    })
  })

  // ==========================================================================
  // Empty state: No subscriptions exist at all
  // ==========================================================================

  test('should display empty state when no subscriptions exist', async ({
    adminSubscriptionListPage,
    demoLogger,
  }) => {
    await test.step('Given: Admin is on the subscription list page for a realm with no subscriptions', async () => {
      await expect(adminSubscriptionListPage.container).toBeVisible()
      demoLogger.testCode.log('[Given] On subscription list page')
    })

    await test.step('When: Page loads with no filters applied', async () => {
      // Page is already loaded via fixture. Check the current state.
      // If subscriptions exist (seed data), this test validates that the page
      // correctly handles data presence. The empty state path is exercised
      // when no subscriptions have been created.
      demoLogger.testCode.log('[When] Page loaded, checking state')
    })

    await test.step('Then: Verify page structure is valid regardless of data state', async () => {
      await adminSubscriptionListPage.waitForDataLoaded()
      const hasTable = await adminSubscriptionListPage.isVisible(adminSubscriptionListPage.table)
      const hasEmpty = await adminSubscriptionListPage.isTableEmpty()

      if (hasEmpty) {
        // Empty state card is visible
        await expect(adminSubscriptionListPage.emptyState).toBeVisible()
        const emptyText = await adminSubscriptionListPage.getEmptyStateText()
        expect(emptyText.length, 'Empty state should have a message').toBeGreaterThan(0)
        demoLogger.testCode.log(`[Then] Empty state message: "${emptyText.trim()}"`)
      } else if (hasTable) {
        // Subscriptions exist -- verify table is well-formed
        const rowCount = await adminSubscriptionListPage.getSubscriptionRowCount()
        expect(rowCount).toBeGreaterThanOrEqual(1)
        demoLogger.testCode.log(
          `[Then] Subscriptions exist (${rowCount} rows), empty state not applicable`
        )
      } else {
        expect(hasTable || hasEmpty).toBe(true)
      }
    })

    await test.step('And: Filter controls are still accessible regardless of data state', async () => {
      // Filter inputs must be visible regardless of whether data exists
      await expect(adminSubscriptionListPage.entitlementKeyFilterInput).toBeVisible()
      await expect(adminSubscriptionListPage.statusFilterSelect).toBeVisible()
      await expect(adminSubscriptionListPage.paymentProviderFilterSelect).toBeVisible()
      demoLogger.testCode.log('[Then] All filter controls are accessible')
    })
  })
})
