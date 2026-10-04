/**
 * Audit Detail Sheet Demo Test
 *
 * Test Coverage:
 * - US-AU-003: View Audit Log Detail
 *   - Scenario 1: Click a table row to open detail sheet, verify all fields present
 *   - Scenario 2: Details JSON section displays formatted content
 *   - Close behavior: Close button dismisses the sheet
 *
 * Uses auditPage fixture from demo-page.fixtures for auto-login and navigation.
 *
 * @note Uses single browser session pattern (one test with multiple steps)
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'

test.describe('US-AU-003: View Audit Log Detail', () => {
  test('Open detail sheet, verify fields and JSON details, then close', async ({ auditPage }) => {
    await test.step('Load audit page and verify table has rows', async () => {
      await expect(auditPage.container).toBeVisible()
      await expect(auditPage.heading).toBeVisible()
      await expect(auditPage.table).toBeVisible()

      const rowCount = await auditPage.getRowCount()
      expect(rowCount).toBeGreaterThan(0)
    })

    await test.step('Click first row to open detail sheet', async () => {
      await auditPage.openDetailSheet(0)
    })

    await test.step('Verify detail sheet is visible', async () => {
      const isOpen = await auditPage.isDetailSheetOpen()
      expect(isOpen).toBe(true)
    })

    await test.step('Verify key fields are present (Time, Actor, Category, Action, Target, Result)', async () => {
      // Time field should contain date-like content
      const timeText = await auditPage.getDetailFieldText('Time')
      expect(timeText.length).toBeGreaterThan(0)

      // Actor field should show a name or "Unknown"
      const actorText = await auditPage.getDetailFieldText('Actor')
      expect(actorText.length).toBeGreaterThan(0)

      // Category field should have a value
      const categoryText = await auditPage.getDetailFieldText('Category')
      expect(categoryText.length).toBeGreaterThan(0)

      // Action field should have a value
      const actionText = await auditPage.getDetailFieldText('Action')
      expect(actionText.length).toBeGreaterThan(0)

      // Target field should show target info
      const targetText = await auditPage.getDetailFieldText('Target')
      expect(targetText.length).toBeGreaterThan(0)

      // Result badge should be visible with a value
      await expect(auditPage.detailResult).toBeVisible()
      const resultText = (await auditPage.detailResult.textContent()) || ''
      expect(resultText.length).toBeGreaterThan(0)
    })

    await test.step('Verify JSON details section is present when available', async () => {
      // The JSON details section is only rendered when the event has a details object.
      // Seed events (like login) typically include details, so we check visibility.
      const jsonVisible = await auditPage.isVisible(auditPage.detailJson)
      if (jsonVisible) {
        const jsonText = await auditPage.getDetailJson()
        // JSON content should be non-empty and parseable
        expect(jsonText.length).toBeGreaterThan(0)
        // Verify it is valid JSON
        const parsed = JSON.parse(jsonText)
        expect(typeof parsed).toBe('object')
      }
    })

    await test.step('Close detail sheet', async () => {
      await auditPage.closeDetailSheet()
    })

    await test.step('Verify sheet is closed and table still visible', async () => {
      const isOpen = await auditPage.isDetailSheetOpen()
      expect(isOpen).toBe(false)

      await expect(auditPage.table).toBeVisible()
    })
  })

  test.afterEach(async ({ page, testStartTime }) => {
    await cleanupTestData(page, 'admin', {
      timestamp: testStartTime,
    })
  })
})
