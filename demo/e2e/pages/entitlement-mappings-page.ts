import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from '../helpers/unified-logger'

/**
 * Page Object for the master-detail Entitlement Mappings page.
 *
 * Route: /{realmId}/manage/billing/entitlement-mappings
 *
 * Frontend source:
 * frontend/src/components/billing/entitlement-mappings-page.tsx
 * + entitlement-mapping-detail-dialog.tsx (ProtectedPriceConfirmDialog — Cancel-only)
 * + provider-sync-button.tsx (wrapper `<div data-testid="provider-sync-button">`).
 *
 * User stories:
 * - US-EM-001: View provider entitlement mappings (list-pane view)
 * - US-EM-002: Trigger provider product sync
 * - US-EM-007: Multi-price master-detail configuration (shared key, per-price policy)
 *
 * LOUD NOTE — priceKey suffix:
 * `price-edit-row-${externalPriceId ?? mappingId}` and the toggle share the same
 * suffix. For Stripe rows (non-NULL external_price_id) the suffix is the price id;
 * for Creem rows (NULL external_price_id — price-less provider) the
 * suffix falls back to the mapping id. Callers MUST pass the correct key for the
 * provider under test.
 *
 * LOUD NOTE — ProtectedPriceConfirmDialog:
 * The 409 dialog renders ONLY `protected-price-active-subs` +
 * `protected-price-confirm-cancel`. There is NO proceed button: the active-
 * subscription lock is enforced authoritatively by the backend 409 (batch rolls
 * back); the client offers no force path. Tests assert the dialog surfaces the
 * active-sub count, then dismiss it.
 */
export class EntitlementMappingsPage extends BasePage {
  // Page shell
  readonly container: Locator
  readonly heading: Locator

  // Banner regions
  readonly readonlyPermBanner: Locator
  readonly webhookPriceUnresolvedBanner: Locator
  readonly emptyState: Locator

  // Master list (left pane)
  readonly mappingProductList: Locator

  // Detail panel (right pane)
  readonly mappingDetailPanel: Locator
  readonly detailHead: Locator
  readonly saveMappingButton: Locator

  // Provider sync controls (wrapper div + inner Button)
  readonly providerSyncButton: Locator
  readonly syncButton: Locator
  readonly syncResultProducts: Locator
  readonly syncResultPrices: Locator

  // Protected-price 409 dialog (Cancel-only)
  readonly protectedPriceConfirmDialog: Locator
  readonly protectedPriceActiveSubs: Locator
  readonly protectedPriceConfirmCancel: Locator

  // Create-mapping form entry. The form is rendered by
  // CreateEntitlementMappingPage (frontend/src/components/billing/
  // create-entitlement-mapping-page.tsx), reached from this page's toolbar.
  readonly createMappingButton: Locator
  readonly createMappingForm: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.container = page.locator(SELECTORS.multiPriceMapping.page)
    this.heading = page.locator('[data-testid="entitlement-mappings-heading"]')

    this.readonlyPermBanner = page.locator(SELECTORS.multiPriceMapping.readonlyPermBanner)
    this.webhookPriceUnresolvedBanner = page.locator(
      SELECTORS.multiPriceMapping.webhookPriceUnresolvedBanner,
    )
    this.emptyState = page.locator(SELECTORS.multiPriceMapping.emptyState)

    this.mappingProductList = page.locator(SELECTORS.multiPriceMapping.mappingProductList)

    this.mappingDetailPanel = page.locator(SELECTORS.multiPriceMapping.mappingDetailPanel)
    this.detailHead = page.locator(SELECTORS.multiPriceMapping.detailHead)
    this.saveMappingButton = page.locator(SELECTORS.multiPriceMapping.saveMappingButton)

    // `provider-sync-button` is a wrapper `<div>`; the actionable controls live
    // inside it. One sync Button is rendered per configured provider, each
    // carrying `data-testid="sync-button"` + `data-provider="<platform>"`.
    this.providerSyncButton = page.locator(SELECTORS.multiPriceMapping.providerSyncButton)
    this.syncButton = this.providerSyncButton.locator(SELECTORS.multiPriceMapping.syncButton)
    this.syncResultProducts = this.providerSyncButton.locator(
      SELECTORS.multiPriceMapping.syncResultProducts,
    )
    this.syncResultPrices = this.providerSyncButton.locator(
      SELECTORS.multiPriceMapping.syncResultPrices,
    )

    this.protectedPriceConfirmDialog = page.locator(
      SELECTORS.multiPriceMapping.protectedPriceConfirmDialog,
    )
    this.protectedPriceActiveSubs = page.locator(
      SELECTORS.multiPriceMapping.protectedPriceActiveSubs,
    )
    this.protectedPriceConfirmCancel = page.locator(
      SELECTORS.multiPriceMapping.protectedPriceConfirmCancel,
    )

    this.createMappingButton = page.locator(SELECTORS.iap.createMappingButton)
    this.createMappingForm = page.locator(SELECTORS.iap.createMappingPage)
  }

  /**
   * Navigate to the entitlement mappings page for a given realm by route.
   *
   * The sidebar entry testid is i18n-derived and must NOT be relied on; always
   * navigate by route.
   */
  async goto(realmId: string = 'admin'): Promise<void> {
    await super.goto(`/manage/billing/entitlement-mappings`)
    await this.waitForReady()
  }

  /**
   * Wait for the page container and heading to be visible.
   */
  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible()
    await expect(this.heading).toBeVisible()
  }

  /**
   * Wait for data to finish loading (master list or empty state becomes visible).
   *
   * The frontend renders a loading skeleton while the API call is in flight;
   * neither the product list nor the empty state has its testid during loading.
   */
  async waitForDataLoaded(timeout: number = 10000): Promise<void> {
    await this.page
      .locator(
        `${SELECTORS.multiPriceMapping.mappingProductList}, ${SELECTORS.multiPriceMapping.emptyState}`,
      )
      .first()
      .waitFor({ state: 'visible', timeout })
  }

  /**
   * Check if the empty state card is visible (no mappings).
   */
  async isListEmpty(): Promise<boolean> {
    return await this.isVisible(this.emptyState)
  }

  // ==================== Master list ====================

  /**
   * Select a product in the master list (left pane) by its external product id.
   * The product row testid is `mapping-product-row-${externalProductId}`.
   */
  async selectProduct(productId: string): Promise<void> {
    const row = this.page.locator(SELECTORS.multiPriceMapping.mappingProductRow(productId))
    await this.smartClick(row)
    // The detail panel mounts/remounts on selection change.
    await expect(this.mappingDetailPanel).toBeVisible({ timeout: 5000 })
  }

  /**
   * Click the first product row (helper for tests that don't know the seeded id).
   */
  async selectFirstProduct(): Promise<void> {
    const row = this.page.locator(SELECTORS.multiPriceMapping.firstMappingProductRow()).first()
    await this.smartClick(row)
    await expect(this.mappingDetailPanel).toBeVisible({ timeout: 5000 })
  }

  /**
   * Check if a product row is rendered as selected (aria-current="true").
   */
  async isProductSelected(productId: string): Promise<boolean> {
    const row = this.page.locator(SELECTORS.multiPriceMapping.mappingProductRow(productId))
    const current = await row.getAttribute('aria-current')
    return current === 'true'
  }

  // ==================== Detail panel ====================

  /**
   * Get the price-edit-row locator for a single price.
   *
   * `priceKey` is `externalPriceId` for Stripe rows and `mappingId` for Creem
   * (NULL price) rows — see the loud note on the class.
   */
  getPriceEditRow(priceKey: string): Locator {
    return this.mappingDetailPanel.locator(SELECTORS.multiPriceMapping.priceEditRow(priceKey))
  }

  getMetadataBlock(priceKey: string): Locator {
    return this.mappingDetailPanel.locator(
      SELECTORS.multiPriceMapping.priceMetadataBlock(priceKey),
    )
  }

  getMetadataEntry(scope: 'product' | 'price', key: string): Locator {
    return this.mappingDetailPanel.locator(
      SELECTORS.multiPriceMapping.metadataEntry(scope, key),
    )
  }

  async getMetadataEntryValue(scope: 'product' | 'price', key: string): Promise<string> {
    return (await this.getMetadataEntry(scope, key).textContent())?.trim() ?? ''
  }

  async getProductRowLabel(productId: string): Promise<string> {
    return (
      await this.page
        .locator(SELECTORS.multiPriceMapping.mappingProductRow(productId))
        .textContent()
    )?.trim() ?? ''
  }

  async getDetailHeadLabel(): Promise<string> {
    return (await this.detailHead.textContent())?.trim() ?? ''
  }

  getBillingTypeInput(priceKey: string): Locator {
    return this.getPriceEditRow(priceKey).locator(
      SELECTORS.multiPriceMapping.priceBillingType(priceKey),
    )
  }

  getBillingPeriodInput(priceKey: string): Locator {
    return this.getReadonlyFieldInput(priceKey, 'Period')
  }

  async getPriceDisplayValue(priceKey: string): Promise<string> {
    return this.getReadonlyFieldValue(priceKey, 'Price')
  }

  async getBillingPeriodValue(priceKey: string): Promise<string> {
    return this.getReadonlyFieldValue(priceKey, 'Period')
  }

  async getEntitlementKeyValue(priceKey: string): Promise<string> {
    return this.getReadonlyFieldValue(priceKey, 'Entitlement Key')
  }

  /**
   * Configure exactly one enabled fixed point-distribution rule for a price.
   * Existing fixed rules are reused so repeated demo runs do not accumulate
   * rules; any other enabled rules are disabled before the chosen rule is saved.
   */
  async configureFixedPointRule(priceKey: string, pointsAmount: number): Promise<void> {
    const editor = this.getPriceEditRow(priceKey).locator(SELECTORS.pointRule.list)
    await expect(editor).toBeVisible()

    const enabledSwitches = editor.locator('[data-testid^="point-rule-enabled-"]')
    const amountInputs = editor.locator('[data-testid^="point-rule-amount-"]')
    let targetAmount = amountInputs.first()

    if ((await amountInputs.count()) === 0) {
      for (let i = 0; i < (await enabledSwitches.count()); i++) {
        const enabledSwitch = enabledSwitches.nth(i)
        if (await enabledSwitch.isChecked()) await this.smartClick(enabledSwitch)
      }
      await this.smartClick(editor.locator(SELECTORS.pointRule.addButton))
      targetAmount = editor.locator('[data-testid^="point-rule-amount-"]').last()
    }

    const targetRule = targetAmount.locator(
      'xpath=ancestor::div[starts-with(@data-testid,"point-rule-")][1]',
    )
    const targetEnabled = targetRule.locator('[data-testid^="point-rule-enabled-"]')
    const targetRuleId = await targetRule.getAttribute('data-testid')
    for (let i = 0; i < (await enabledSwitches.count()); i++) {
      const enabledSwitch = enabledSwitches.nth(i)
      const ownerRuleId = await enabledSwitch.evaluate(
        (node) =>
          node.parentElement
            ?.closest('div[data-testid^="point-rule-"]')
            ?.getAttribute('data-testid') ?? null,
      )
      if ((await enabledSwitch.isChecked()) && ownerRuleId !== targetRuleId) {
        await this.smartClick(enabledSwitch)
      }
    }
    if (!(await targetEnabled.isChecked())) await this.smartClick(targetEnabled)

    const bucketSelect = targetRule.locator(SELECTORS.pointRule.bucketSelect)
    await this.smartClick(bucketSelect)
    await this.smartClick(this.page.locator('[role="option"]:not([data-disabled])').first())
    await this.fillField(targetAmount, String(pointsAmount))
  }

  /** Read the amount of the enabled fixed rule after save/reload. */
  async getFixedPointRuleAmount(priceKey: string): Promise<string> {
    const editor = this.getPriceEditRow(priceKey).locator(SELECTORS.pointRule.list)
    const amountInputs = editor.locator('[data-testid^="point-rule-amount-"]')
    for (let i = 0; i < (await amountInputs.count()); i++) {
      const amount = amountInputs.nth(i)
      const rule = amount.locator(
        'xpath=ancestor::div[starts-with(@data-testid,"point-rule-")][1]',
      )
      if (await rule.locator('[data-testid^="point-rule-enabled-"]').isChecked()) {
        return await amount.inputValue()
      }
    }
    throw new Error(`No enabled fixed point rule found for price ${priceKey}`)
  }

  /** Disable persisted point rules (or remove unsaved ones) through the editor. */
  async clearPointRules(priceKey: string): Promise<void> {
    const editor = this.getPriceEditRow(priceKey).locator(SELECTORS.pointRule.list)
    const enabledSwitches = editor.locator('[data-testid^="point-rule-enabled-"]')
    for (let i = (await enabledSwitches.count()) - 1; i >= 0; i--) {
      const enabledSwitch = enabledSwitches.nth(i)
      if (!(await enabledSwitch.isChecked())) continue
      const rule = enabledSwitch.locator(
        'xpath=ancestor::div[starts-with(@data-testid,"point-rule-")][1]',
      )
      await this.smartClick(rule.locator('[data-testid^="point-rule-remove-"]'))
    }
  }

  /** Count enabled point rules after save/reload. */
  async getEnabledPointRuleCount(priceKey: string): Promise<number> {
    const editor = this.getPriceEditRow(priceKey).locator(SELECTORS.pointRule.list)
    const enabledSwitches = editor.locator('[data-testid^="point-rule-enabled-"]')
    let enabledCount = 0
    for (let i = 0; i < (await enabledSwitches.count()); i++) {
      if (await enabledSwitches.nth(i).isChecked()) enabledCount += 1
    }
    return enabledCount
  }

  /**
   * Get the enabled-toggle locator for a single price.
   */
  getPriceEnabledToggle(priceKey: string): Locator {
    return this.mappingDetailPanel.locator(
      SELECTORS.multiPriceMapping.priceEnabledToggle(priceKey),
    )
  }

  /**
   * Get the shared-key chip locator for an entitlement key (renders once per
   * shared key inside the detail panel).
   */
  getSharedKeyChip(entitlementKey: string): Locator {
    return this.mappingDetailPanel.locator(
      SELECTORS.multiPriceMapping.sharedKeyChip(entitlementKey),
    )
  }

  // ==================== Granted-roles dimension ====================
  //
  // The granted-roles field is a `<div className="sm:col-span-2"
  // data-testid="price-granted-roles-${priceKey}">` wrapper (priceKey =
  // `price.externalPriceId ?? price.id`) containing a `<Field label>` +
  // `<RoleSelector>`.
  //
  // RoleSelector (frontend/src/components/shared/role-selector.tsx) is a Radix
  // Popover + cmdk Command. IMPORTANT rendering facts (verified against source):
  //  - Trigger testid `role-selector-trigger` (a `<Button role="combobox">`).
  //  - Collapsed trigger shows selected roles as `<Badge>` chips carrying the
  //    role NAME only — there is NO `data-role-id` anywhere, so the closed
  //    trigger CANNOT be used to read back role ids.
  //  - Item testid `role-selector-item-${role.id}`. The `<Check>` lucide icon is
  //    ALWAYS rendered; selected = `svg.opacity-100`, unselected =
  //    `svg.opacity-0`.
  //  - Clicking an item toggles it (same click selects/deselects); the popover
  //    STAYS OPEN after selecting. You MUST press Escape (or click outside)
  //    before interacting with the save button — Radix retains focus and would
  //    otherwise intercept the save click.
  //  - The popover CONTENT is portaled to `document.body`, so item locators are
  //    scoped to `this.page` (NOT to the field div). Only the trigger lives
  //    inside the field.

  /**
   * Get the granted-roles Field wrapper locator for a single price.
   *
   * `priceKey` is `externalPriceId` for Stripe rows and `mappingId` for Creem
   * (NULL price) rows — same suffix rule as the price-edit-row.
   */
  getGrantedRolesField(priceKey: string): Locator {
    return this.mappingDetailPanel.locator(`[data-testid="price-granted-roles-${priceKey}"]`)
  }

  /**
   * Read back the currently granted role ids for a price.
   *
   * Because there is no `data-role-id` on the closed trigger, this OPENS the
   * popover, scans all `role-selector-item-*` for those whose `<Check>` icon has
   * `opacity-100`, parses the role id from the testid suffix, then closes the
   * popover with Escape. The save button is NOT clicked — callers persist
   * changes explicitly via `saveChanges()`.
   */
  async getGrantedRoles(priceKey: string): Promise<string[]> {
    const field = this.getGrantedRolesField(priceKey)
    await expect(field).toBeVisible()

    // Open the popover (trigger lives inside the field).
    const trigger = field.locator(SELECTORS.apiKeyRoles.roleSelectorTrigger)
    await this.smartClick(trigger)

    // Wait for the portaled popover content to mount. Items are page-scoped
    // (portaled to document.body), so query `this.page`, NOT `field`.
    await expect(this.page.getByTestId('role-selector-search')).toBeVisible({
      timeout: 5000,
    })

    const items = this.page.locator(`[data-testid^="role-selector-item-"]`)
    const count = await items.count()
    const selectedIds: string[] = []
    for (let i = 0; i < count; i++) {
      const item = items.nth(i)
      const testid = (await item.getAttribute('data-testid')) ?? ''
      const roleId = testid.replace(/^role-selector-item-/, '')
      // Selected ⇔ the Check svg is opaque (opacity-100).
      const check = item.locator('svg').first()
      const cls = (await check.getAttribute('class')) ?? ''
      if (cls.includes('opacity-100')) {
        selectedIds.push(roleId)
      }
    }

    // Close the popover so the page is in a stable, non-portal-focused state.
    await this.page.keyboard.press('Escape')
    return selectedIds
  }

  /**
   * Select (grant) roles for a price via the RoleSelector. Each requested role
   * is toggled ON; already-selected roles are left as-is. The popover stays open
   * between selections (Radix cmdk behavior), and Escape is pressed once at the
   * end to close it before any save click.
   *
   * Does NOT call `saveChanges()` — callers persist the whole row explicitly.
   */
  async selectGrantedRoles(priceKey: string, roleIds: string[]): Promise<void> {
    const field = this.getGrantedRolesField(priceKey)
    await expect(field).toBeVisible()

    const trigger = field.locator(SELECTORS.apiKeyRoles.roleSelectorTrigger)
    await this.smartClick(trigger)
    await expect(this.page.getByTestId('role-selector-search')).toBeVisible({
      timeout: 5000,
    })

    for (const roleId of roleIds) {
      // Items are page-scoped (portaled content). Re-query each time — the list
      // does not remount between toggles, but resolving freshly avoids stale
      // element-handle issues.
      const item = this.page.getByTestId(`role-selector-item-${roleId}`)
      await expect(item).toBeVisible({ timeout: 5000 })
      // Only click when not already selected (toggle semantics: a second click
      // would deselect). Read the Check icon opacity to decide.
      const check = item.locator('svg').first()
      const cls = (await check.getAttribute('class')) ?? ''
      if (!cls.includes('opacity-100')) {
        await this.smartClick(item)
      }
    }

    // Close the popover before returning (see class note: Radix retains focus).
    await this.page.keyboard.press('Escape')
  }

  /**
   * Clear all granted roles for a price. Opens the popover, finds every
   * selected item (Check svg opacity-100), clicks each to deselect, then closes
   * the popover with Escape.
   *
   * Does NOT call `saveChanges()`.
   */
  async clearGrantedRoles(priceKey: string): Promise<void> {
    const field = this.getGrantedRolesField(priceKey)
    await expect(field).toBeVisible()

    const trigger = field.locator(SELECTORS.apiKeyRoles.roleSelectorTrigger)
    await this.smartClick(trigger)
    await expect(this.page.getByTestId('role-selector-search')).toBeVisible({
      timeout: 5000,
    })

    // Repeatedly deselect until no item is opaque, since deselection does not
    // reorder the list (ids are stable; only the opacity toggles).
    const items = this.page.locator(`[data-testid^="role-selector-item-"]`)
    const count = await items.count()
    for (let i = 0; i < count; i++) {
      const item = items.nth(i)
      const check = item.locator('svg').first()
      const cls = (await check.getAttribute('class')) ?? ''
      if (cls.includes('opacity-100')) {
        await this.smartClick(item)
      }
    }

    await this.page.keyboard.press('Escape')
  }

  // fillPriceRow was REMOVED — dead path under the current contract
  // (frontend commit 2ef33cc8 made the entitlement key provider-owned): the
  // detail panel's Entitlement Key, Period, and legacy "Points per period"
  // inputs are read-only or gone, so none of its fill branches had a valid
  // target. Points policy is configured via `configureFixedPointRule`;
  // readonly values are read via `getEntitlementKeyValue` /
  // `getBillingPeriodValue`. The create-mapping page
  // (`fillCreateMappingForm`) still accepts a writable entitlement key and is
  // unaffected.

  private async getReadonlyFieldValue(priceKey: string, label: string): Promise<string> {
    const input = this.getReadonlyFieldInput(priceKey, label)
    await expect(input).toBeVisible()
    return await input.inputValue()
  }

  private getReadonlyFieldInput(priceKey: string, label: string): Locator {
    const row = this.getPriceEditRow(priceKey)
    const field = row
      .locator(`xpath=./div[1]//label[normalize-space()='${label}']`)
      .locator('xpath=ancestor::div[starts-with(@class,"space-y-1")][1]')
    return field.locator('input').first()
  }

  /**
   * Toggle the enabled switch on a single price row.
   *
   * NOTE: When the price protects active subscriptions, the backend rejects the
   * disable with a 409 AFTER save (the toggle itself is not pre-disabled on the
   * client). Callers expecting the 409 path should call saveChanges() next and
   * then expectProtectedPriceDialog().
   */
  async togglePriceEnabled(priceKey: string): Promise<void> {
    const toggle = this.getPriceEnabledToggle(priceKey)
    await this.smartClick(toggle)
  }

  /**
   * Click the Save Changes button (batch PUT). Does not wait for the response —
   * callers that need to assert the result should follow with the appropriate
   * expect* call (banner / dialog / panel re-render).
   */
  async saveChanges(): Promise<void> {
    await expect(this.saveMappingButton).toBeVisible()
    await this.saveMappingButton.click()
  }

  // ==================== Protected-price 409 dialog ====================

  /**
   * Assert the protected-price 409 dialog is visible (surfaces after a save that
   * the backend rejected because the toggled price protects active subscriptions).
   */
  async expectProtectedPriceDialog(): Promise<void> {
    await expect(this.protectedPriceConfirmDialog).toBeVisible({ timeout: 5000 })
    await expect(this.protectedPriceActiveSubs).toBeVisible()
  }

  /**
   * Read the active-subscription count surfaced by the 409 dialog.
   */
  async getProtectedPriceActiveSubs(): Promise<number> {
    await expect(this.protectedPriceActiveSubs).toBeVisible()
    const text = (await this.protectedPriceActiveSubs.textContent()) || ''
    const match = text.match(/\d+/)
    return match ? Number(match[0]) : 0
  }

  /**
   * Dismiss the protected-price dialog via its Cancel button (the only action;
   * there is NO proceed button — the lock is backend-enforced).
   */
  async cancelProtectedPrice(): Promise<void> {
    await expect(this.protectedPriceConfirmCancel).toBeVisible()
    await this.protectedPriceConfirmCancel.click()
    await expect(this.protectedPriceConfirmDialog).toBeHidden({ timeout: 3000 })
  }

  // ==================== Webhook-unresolved banner ====================

  /**
   * Assert the webhook-price-unresolved banner is visible (rendered when at
   * least one loaded mapping has an unresolved webhook price).
   */
  async expectWebhookUnresolvedBanner(): Promise<void> {
    await expect(this.webhookPriceUnresolvedBanner).toBeVisible()
  }

  // ==================== Provider sync ====================

  /**
   * Trigger a provider product sync via the toolbar sync button.
   *
   * One Button is rendered per configured provider (e.g. "Sync Stripe"). This
   * clicks the button matching the requested provider via its `data-provider`
   * attribute, then waits for the result spans to surface. Returns the parsed
   * {productsSynced, pricesSynced} counts from the result spans.
   *
   * @param provider 'stripe' | 'creem'
   */
  async sync(
    provider: 'stripe' | 'creem',
  ): Promise<{ productsSynced: number; pricesSynced: number }> {
    await expect(this.providerSyncButton).toBeVisible()
    // One sync button per configured provider; scope by data-provider so the
    // correct provider's button is clicked when both are configured.
    const providerSyncButton = this.providerSyncButton.locator(
      `[data-testid="sync-button"][data-provider="${provider}"]`
    )

    // Click the provider's sync button, then wait for either the result spans
    // or a toast (sync may fail with test credentials). Resolve counts if present.
    await this.smartClick(providerSyncButton)

    // Best-effort: wait for result spans (completed/partial sync renders them).
    const resultVisible = await this.syncResultProducts
      .waitFor({ state: 'visible', timeout: 10000 })
      .then(() => true)
      .catch(() => false)

    if (!resultVisible) {
      return { productsSynced: 0, pricesSynced: 0 }
    }

    const productsText = (await this.syncResultProducts.textContent()) || ''
    const pricesText = (await this.syncResultPrices.textContent()) || ''
    const productsMatch = productsText.match(/\d+/)
    const pricesMatch = pricesText.match(/\d+/)
    return {
      productsSynced: productsMatch ? Number(productsMatch[0]) : 0,
      pricesSynced: pricesMatch ? Number(pricesMatch[0]) : 0,
    }
  }

  // ==================== Create-mapping page ====================

  /**
   * Navigate to the Create Entitlement Mapping page from the toolbar.
   *
   * Clicks the `create-mapping-button` (rendered only when `billing.manage`
   * is held; navigates to /manage/billing/entitlement-mappings/new) and waits
   * for `create-entitlement-mapping-page` to be visible. The timeout covers
   * the route's lazy chunk load.
   *
   * Shared open entry for US-IAP-002. The full field-fill helpers below
   * (provider/bucket/billing-type selects, product/price/entitlement-key
   * inputs, points strategy) consume `SELECTORS.iap.createMapping*`.
   */
  async openCreateMappingPage(): Promise<void> {
    await expect(this.createMappingButton).toBeVisible()
    await this.smartClick(this.createMappingButton)
    await expect(this.createMappingForm).toBeVisible({ timeout: 10000 })
  }

  /**
   * Pick an option from a create-mapping Radix Select by its visible option name.
   *
   * Radix renders SelectItem options with role="option"; the accessible name is
   * the item's child text (e.g. "App Store", "Recurring", "Month", "Primary
   * Pool"). Precedent: `PaymentProvidersPage.selectAppleEnvironment` +
   * `admin-subscription-history-demo.e2e.ts:226`.
   *
   * @param triggerTestId The `data-testid` on the SelectTrigger.
   * @param optionName   The visible name of the option to click (exact).
   */
  private async pickCreateMappingSelectOption(
    triggerTestId: string,
    optionName: string,
  ): Promise<void> {
    // `triggerTestId` is a full attribute selector (e.g.
    // '[data-testid="create-mapping-provider-select"]') stored in SELECTORS.iap,
    // matching the codebase convention of `page.locator(SELECTORS...)`. Route it
    // through `locator()` (NOT `getByTestId`, which expects a bare testid and
    // would double-wrap the attribute, matching nothing).
    await this.smartClick(this.page.locator(triggerTestId))
    // Radix Select content is portaled; the option carries role="option". Match
    // the visible name exactly so "Month" does not also match "Year"-adjacent
    // text.
    const option = this.page.getByRole('option', { name: optionName, exact: true })
    await expect(option).toBeVisible({ timeout: 5000 })
    await option.click()
  }

  /**
   * Resolve + select the credit bucket by its display NAME (not UUID — bucket ids
   * are dynamic). If `bucketName` is supplied it is selected directly; otherwise
   * the FIRST available option is chosen (fallback when the seed bucket display
   * name is not stable across envs — recorded gap).
   *
   * @returns the display name of the bucket actually selected (for caller logs).
   */
  private async selectCreateMappingBucket(bucketName?: string): Promise<string> {
    await this.smartClick(
      this.createMappingForm.locator(SELECTORS.pointRule.bucketSelect),
    )
    if (bucketName) {
      const named = this.page.getByRole('option', { name: bucketName, exact: true })
      await expect(named).toBeVisible({ timeout: 5000 })
      await named.click()
      return bucketName
    }
    // Fallback: pick the first option. `getByRole('option')` resolves lazily, so
    // wait for at least one to be visible first.
    const firstOption = this.page.getByRole('option').first()
    await expect(firstOption).toBeVisible({ timeout: 5000 })
    const name = (await firstOption.textContent())?.trim() ?? '<first-bucket>'
    await firstOption.click()
    return name
  }

  /**
   * Fill the create-mapping form (US-IAP-002). Opens the page via
   * `openCreateMappingPage()`, drives the Radix selects by visible option
   * name, fills the text inputs by canonical testid, and configures one points
   * distribution rule. Does NOT click submit — the caller drives the
   * outcome assertion (success → list row / returned to the list, or duplicate →
   * `create-mapping-submit-error`).
   *
   * Select option names (en locale, the admin realm default):
   *   - provider:   'App Store' (apple) | 'Google Play' (google)
   *   - billingType: 'Recurring' | 'One-time'
   *   - billingPeriod (recurring only): 'Month' (monthly) | 'Year' (yearly)
   *
   * Bucket is resolved by display name (the seeded registration pool display
   * name is "Primary Pool" — `scripts/lib/demo_seed.py::CREDIT_BUCKET_NAME_PRIMARY`,
   * mirrored in `helpers/bucket-seed-ids.ts::CREDIT_BUCKET_NAMES.PRIMARY_POOL`).
   * If `bucketName` is omitted, the first bucket option is selected as a
   * documented fallback.
   *
   * The points editor renders only after billing type is selected. A newly
   * added rule starts with the billing type's first legal trigger selected;
   * callers explicitly provide every trigger the scenario needs.
   */
  async fillCreateMappingForm(values: {
    /** 'apple' → 'App Store', 'google' → 'Google Play'. */
    provider: 'apple' | 'google'
    externalProductId: string
    entitlementKey: string
    /** Display name to match against the bucket option (e.g. 'Primary Pool'). */
    bucketName?: string
    billingType: 'recurring' | 'one_time'
    /** 'monthly' → 'Month', 'yearly' → 'Year'. Required for recurring. */
    billingPeriod?: 'monthly' | 'yearly'
    /** Trigger sources to enable on the single points distribution rule. */
    pointRuleTriggers: Array<
      'topup' | 'subscription_initial' | 'subscription_renewal' | 'subscription_upgrade'
    >
    /** Fixed points amount granted by the rule. */
    pointsAmount: number
    /** Fixed-rule validity days. */
    validityDays?: number
    /** Optional Stripe price id. IAP/Creem leave it empty. */
    externalPriceId?: string
  }): Promise<void> {
    await this.openCreateMappingPage()

    const providerOptionName =
      values.provider === 'apple' ? 'App Store' : 'Google Play'
    await this.pickCreateMappingSelectOption(
      SELECTORS.iap.createMappingProviderSelect,
      providerOptionName,
    )

    await this.fillField(
      this.page.locator(SELECTORS.iap.createMappingExternalProductIdInput),
      values.externalProductId,
    )

    if (values.externalPriceId !== undefined) {
      await this.fillField(
        this.page.locator(SELECTORS.iap.createMappingExternalPriceIdInput),
        values.externalPriceId,
      )
    }

    await this.fillField(
      this.page.locator(SELECTORS.iap.createMappingEntitlementKeyInput),
      values.entitlementKey,
    )

    const billingTypeOptionName =
      values.billingType === 'recurring' ? 'Recurring' : 'One-time'
    await this.pickCreateMappingSelectOption(
      SELECTORS.iap.createMappingBillingTypeSelect,
      billingTypeOptionName,
    )

    // Billing Period — recurring only. 'monthly' → 'Month', 'yearly' → 'Year'
    // (SelectItem text from billing.billing_period_month / _year).
    if (values.billingType === 'recurring' && values.billingPeriod) {
      const periodOptionName = values.billingPeriod === 'monthly' ? 'Month' : 'Year'
      await this.pickCreateMappingSelectOption(
        SELECTORS.iap.createMappingBillingPeriodSelect,
        periodOptionName,
      )
    }

    const addRule = this.createMappingForm.locator(SELECTORS.pointRule.addButton)
    await expect(addRule).toBeVisible({ timeout: 5000 })
    await this.smartClick(addRule)
    await this.selectCreateMappingBucket(values.bucketName)

    for (const trigger of values.pointRuleTriggers) {
      const checkbox = this.createMappingForm.locator(
        SELECTORS.pointRule.trigger(trigger),
      )
      await expect(checkbox).toBeVisible({ timeout: 5000 })
      if ((await checkbox.getAttribute('data-state')) !== 'checked') {
        await this.smartClick(checkbox)
      }
    }

    await this.fillField(
      this.createMappingForm.locator(SELECTORS.pointRule.amountInput('new-0')),
      String(values.pointsAmount),
    )
    if (values.validityDays !== undefined) {
      await this.fillField(
        this.createMappingForm.locator(SELECTORS.pointRule.validityInput('new-0')),
        String(values.validityDays),
      )
    }

    // Submit is intentionally NOT clicked here — the caller drives the outcome.
  }

  /**
   * Click the create-mapping submit button. The caller then asserts the outcome
   * via `expectCreateMappingFormClosed()` (success) or
   * `expectCreateMappingDuplicateError()` (409).
   */
  async submitCreateMapping(): Promise<void> {
    const submit = this.page.locator(SELECTORS.iap.createMappingSubmitButton)
    await expect(submit).toBeVisible()
    await submit.click()
  }

  /**
   * Assert the 409 duplicate inline error region is visible. The backend rejects
   * a create whose (provider, externalProductId) collides with an existing row
   * (the pair is unique); the form surfaces
   * `create-mapping-submit-error` inline (NOT a toast — toasts are auto-dismissed
   * and must not be the primary assertion). The form remains on the page on failure.
   */
  async expectCreateMappingDuplicateError(): Promise<void> {
    await expect(
      this.page.locator(SELECTORS.iap.createMappingSubmitError),
    ).toBeVisible({ timeout: 10000 })
  }

  /**
   * Assert the create-mapping page has been left (success path). On a successful
   * create the mutation invalidates `['entitlement-mappings']` and the form
   * navigates back to the list; the master list refreshes with the new product row.
   */
  async expectCreateMappingFormClosed(): Promise<void> {
    await expect(
      this.page.locator(SELECTORS.iap.createMappingPage),
    ).toBeHidden({ timeout: 10000 })
  }

  /**
   * Assert the create-mapping form REMAINS on the page (failed submit does not
   * navigate away). Used alongside `expectCreateMappingDuplicateError` to pin
   * the "stays put on error" UX contract.
   */
  async expectCreateMappingFormOpen(): Promise<void> {
    await expect(
      this.page.locator(SELECTORS.iap.createMappingPage),
    ).toBeVisible()
  }
}
