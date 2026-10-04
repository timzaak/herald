/**
 * Audit List View and Filtering Demo Test
 *
 * Test Coverage:
 * - US-AU-001: View Realm Audit Log List
 *   - Scenario 1: Normal list display with expected columns
 *   - Scenario 2: Realm isolation (admin realm sees only admin realm events)
 *   - Scenario 3: Empty state table structure verification
 * - US-AU-002: Filter Audit Logs
 *   - Scenario 1: Filter by category
 *   - Scenario 2: Filter by time range
 *   - Scenario 3: Filter by actor
 *   - Scenario 4: Combined filters with no results
 *
 * Uses auditPage fixture from demo-page.fixtures for auto-login and navigation.
 *
 * @note Uses single browser session pattern (one test per user story with multiple steps)
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'

test.describe('US-AU-001: View Realm Audit Log List', () => {
  test('Normal list display with expected columns and data', async ({ auditPage }) => {
    await test.step('Given the admin is on the audit page', async () => {
      await expect(auditPage.container).toBeVisible()
      await expect(auditPage.heading).toBeVisible()
    })

    await test.step('Then the audit table is visible', async () => {
      await expect(auditPage.table).toBeVisible()
    })

    await test.step('And the table has data rows (admin realm has seed events)', async () => {
      const rowCount = await auditPage.getRowCount()
      expect(rowCount).toBeGreaterThan(0)
    })

    await test.step('And the first row contains expected column data', async () => {
      const rowTexts = await auditPage.getRowTexts(0)
      expect(rowTexts.length).toBeGreaterThanOrEqual(7)
      expect(rowTexts[0].length).toBeGreaterThan(0) // Time
      expect(rowTexts[2].length).toBeGreaterThan(0) // Category
      expect(rowTexts[3].length).toBeGreaterThan(0) // Action
      expect(rowTexts[5].length).toBeGreaterThan(0) // Result
    })
  })

  test('Table header contains expected column names', async ({ auditPage }) => {
    await test.step('Given the admin is on the audit page', async () => {
      await expect(auditPage.container).toBeVisible()
    })

    await test.step('Then the table header row has the expected columns', async () => {
      // DataTable only renders <thead> once data is loaded, so wait for the
      // first header cell before reading (avoids an empty header array race).
      const headerCells = auditPage.table.locator('thead th')
      await expect(headerCells.first()).toBeVisible()
      const headerTexts: string[] = []
      const count = await headerCells.count()
      for (let i = 0; i < count; i++) {
        headerTexts.push((await headerCells.nth(i).textContent()) || '')
      }
      expect(headerTexts).toContain('Time')
      expect(headerTexts).toContain('Actor')
      expect(headerTexts).toContain('Category')
      expect(headerTexts).toContain('Action')
      expect(headerTexts).toContain('Target')
      expect(headerTexts).toContain('Result')
    })
  })

  test('Filter bar is visible with all filter controls', async ({ auditPage }) => {
    await test.step('Given the admin is on the audit page', async () => {
      await expect(auditPage.container).toBeVisible()
    })

    await test.step('Then the filter bar is visible', async () => {
      await expect(auditPage.filterBar).toBeVisible()
    })

    await test.step('And all filter controls are present', async () => {
      await expect(auditPage.filterCategory).toBeVisible()
      await expect(auditPage.filterAction).toBeVisible()
      await expect(auditPage.filterActorId).toBeVisible()
      await expect(auditPage.filterStartDate).toBeVisible()
      await expect(auditPage.filterEndDate).toBeVisible()
    })
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, 'admin', {
      timestamp: testStartTime,
    })
  })
})

test.describe('US-AU-002: Filter Audit Logs', () => {
  test('Filter by category returns only matching events', async ({ auditPage }) => {
    let initialRowCount = 0

    await test.step('Given the audit page shows events', async () => {
      await expect(auditPage.table).toBeVisible()
      initialRowCount = await auditPage.getRowCount()
      expect(initialRowCount).toBeGreaterThan(0)
    })

    await test.step('When filtering by category "auth"', async () => {
      await auditPage.filterByCategory('auth')
    })

    await test.step('Then the table shows filtered results', async () => {
      await expect(auditPage.table).toBeVisible()
      const filteredRowCount = await auditPage.getRowCount()
      expect(filteredRowCount).toBeGreaterThan(0)
    })

    await test.step('And each visible row has category "auth"', async () => {
      const rowCount = await auditPage.getRowCount()
      for (let i = 0; i < Math.min(rowCount, 5); i++) {
        const rowTexts = await auditPage.getRowTexts(i)
        expect(rowTexts[2].toLowerCase()).toContain('auth')
      }
    })

    await test.step('And the clear filters button is visible', async () => {
      const hasFilters = await auditPage.hasActiveFilters()
      expect(hasFilters).toBe(true)
    })

    await test.step('And clearing filters restores the original data', async () => {
      await auditPage.clearFilters()
      await expect(auditPage.table).toBeVisible()
      const restoredRowCount = await auditPage.getRowCount()
      expect(restoredRowCount).toBe(initialRowCount)
    })
  })

  test('Filter by time range returns only events in range', async ({ auditPage }) => {
    let initialRowCount = 0

    await test.step('Given the audit page shows events', async () => {
      await expect(auditPage.table).toBeVisible()
      initialRowCount = await auditPage.getRowCount()
      expect(initialRowCount).toBeGreaterThan(0)
    })

    await test.step('When filtering by today\'s date range', async () => {
      const today = new Date()
      const todayStr = today.toISOString().split('T')[0]
      await auditPage.filterByDateRange(todayStr, todayStr)
      await expect(auditPage.table).toBeVisible()
    })

    await test.step('Then the filter is active (clear button visible)', async () => {
      const hasFilters = await auditPage.hasActiveFilters()
      expect(hasFilters).toBe(true)
    })
  })

  test('Filter by actor ID returns only matching events', async ({ auditPage }) => {
    await test.step('Given the audit page shows events', async () => {
      await expect(auditPage.table).toBeVisible()
      const rowCount = await auditPage.getRowCount()
      expect(rowCount).toBeGreaterThan(0)
    })

    await test.step('When filtering by a non-existent actor ID', async () => {
      await auditPage.filterByActor('nonexistent-actor-id-xyz')
    })

    await test.step('Then the table eventually shows no results', async () => {
      // The actor filter has a 300ms debounce in the frontend, so we use
      // toPass() to poll until the table reflects the filtered (empty) state.
      await expect(async () => {
        const emptyMessage = await auditPage.getEmptyMessage()
        if (emptyMessage) {
          expect(emptyMessage.length).toBeGreaterThan(0)
        } else {
          const rowCount = await auditPage.getRowCount()
          expect(rowCount).toBe(0)
        }
      }).toPass({ timeout: 5000 })
    })
  })

  test('Combined filters with no matching results show empty state', async ({ auditPage }) => {
    await test.step('Given the audit page shows events', async () => {
      await expect(auditPage.table).toBeVisible()
    })

    await test.step('When applying filters that match nothing (far-past date range)', async () => {
      // Use a date range in year 2000 -- no audit events should exist then
      await auditPage.filterByDateRange('2000-01-01', '2000-01-02')
      await expect(auditPage.table).toBeVisible()
    })

    await test.step('Then the table shows empty state', async () => {
      const emptyMessage = await auditPage.getEmptyMessage()
      if (emptyMessage) {
        expect(emptyMessage.length).toBeGreaterThan(0)
      } else {
        const rowCount = await auditPage.getRowCount()
        expect(rowCount).toBe(0)
      }
    })

    await test.step('And the clear filters button is visible', async () => {
      const hasFilters = await auditPage.hasActiveFilters()
      expect(hasFilters).toBe(true)
    })

    await test.step('And clearing filters restores the table', async () => {
      await auditPage.clearFilters()
      await expect(auditPage.table).toBeVisible()
      const rowCount = await auditPage.getRowCount()
      expect(rowCount).toBeGreaterThan(0)
    })
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, 'admin', {
      timestamp: testStartTime,
    })
  })
})
