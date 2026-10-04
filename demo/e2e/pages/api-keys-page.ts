/**
 * API Keys Page Object
 *
 * Encapsulates API Key management page operations: list, create, edit, reveal, and delete.
 *
 */

import { Page, Locator, expect } from "@playwright/test";
import { SELECTORS } from "../selectors";
import { BasePage } from "./base-page";
import type { UnifiedLogger } from "../helpers/unified-logger";

/**
 * API Keys Page Object
 *
 * Represents API Key management pages:
 * - List: /{realmId}/manage/api-keys
 * - Create: /{realmId}/manage/api-keys/new
 * - Edit: /{realmId}/manage/api-keys/{apiKeyId}/edit
 * - Reveal: /{realmId}/manage/api-keys/reveal
 *
 * @example
 * ```typescript
 * const apiKeyPage = new ApiKeysPage(page, logger)
 * await apiKeyPage.goto()
 * await apiKeyPage.gotoCreatePage()
 * await apiKeyPage.fillCreateForm({ name: 'my-key' })
 * await apiKeyPage.submitForm()
 * await apiKeyPage.waitForRevealPage()
 * ```
 */
export class ApiKeysPage extends BasePage {
  // List page locators
  readonly container: Locator;
  readonly heading: Locator;
  readonly table: Locator;
  readonly addButton: Locator;

  // Form page locators (create/edit)
  readonly formPage: Locator;
  readonly pageTitle: Locator;
  readonly formBackButton: Locator;
  readonly nameInput: Locator;
  readonly clientAppSelectorTrigger: Locator;
  readonly clientAppSelectorSearch: Locator;
  readonly enabledSwitch: Locator;
  readonly expiresAtInput: Locator;
  readonly expiresAtClearButton: Locator;
  readonly cancelButton: Locator;
  readonly submitButton: Locator;

  // Reveal page locators
  readonly revealPage: Locator;
  readonly revealPageTitle: Locator;
  readonly revealBackButton: Locator;
  readonly keyValue: Locator;
  readonly copyButton: Locator;
  readonly doneButton: Locator;

  // Delete dialog locators
  readonly deleteDialog: Locator;
  readonly deleteCancelButton: Locator;
  readonly deleteConfirmButton: Locator;

  // Roles dialog locators
  readonly rolesDialog: Locator;
  readonly rolesDialogTitle: Locator;
  readonly rolesDialogClose: Locator;
  readonly roleSelectorTrigger: Locator;
  readonly roleSelectorSearch: Locator;

  // Success toast
  readonly successMessage: Locator;

  // Current realm ID (stored for navigation)
  private currentRealmId: string = "admin";

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger);

    // List page
    this.container = this.page.locator(SELECTORS.apiKeys.page);
    this.heading = this.page.locator(SELECTORS.apiKeys.heading);
    this.table = this.page.locator(SELECTORS.apiKeys.table);
    this.addButton = this.page.locator(SELECTORS.apiKeys.addButton);

    // Form page
    this.formPage = this.page.locator(SELECTORS.apiKeyForm.page);
    this.pageTitle = this.page.locator(SELECTORS.apiKeyForm.pageTitle);
    this.formBackButton = this.page.locator(SELECTORS.apiKeyForm.backButton);
    this.nameInput = this.page.locator(SELECTORS.apiKeyForm.nameInput);
    this.clientAppSelectorTrigger = this.page.locator(
      SELECTORS.apiKeyForm.clientAppSelectorTrigger,
    );
    this.clientAppSelectorSearch = this.page.locator(
      SELECTORS.apiKeyForm.clientAppSelectorSearch,
    );
    this.enabledSwitch = this.page.locator(SELECTORS.apiKeyForm.enabledSwitch);
    this.expiresAtInput = this.page.locator(
      SELECTORS.apiKeyForm.expiresAtInput,
    );
    this.expiresAtClearButton = this.page.locator(
      SELECTORS.apiKeyForm.expiresAtClearButton,
    );
    this.cancelButton = this.page.locator(SELECTORS.apiKeyForm.cancelButton);
    this.submitButton = this.page.locator(SELECTORS.apiKeyForm.submitButton);

    // Reveal page
    this.revealPage = this.page.locator(SELECTORS.apiKeyReveal.page);
    this.revealPageTitle = this.page.locator(SELECTORS.apiKeyReveal.pageTitle);
    this.revealBackButton = this.page.locator(
      SELECTORS.apiKeyReveal.backButton,
    );
    this.keyValue = this.page.locator(SELECTORS.apiKeyReveal.keyValue);
    this.copyButton = this.page.locator(SELECTORS.apiKeyReveal.copyButton);
    this.doneButton = this.page.locator(SELECTORS.apiKeyReveal.doneButton);

    // Delete dialog
    this.deleteDialog = this.page.locator(SELECTORS.apiKeyDelete.dialog);
    this.deleteCancelButton = this.page.locator(
      SELECTORS.apiKeyDelete.cancelButton,
    );
    this.deleteConfirmButton = this.page.locator(
      SELECTORS.apiKeyDelete.confirmButton,
    );

    // Roles dialog
    this.rolesDialog = this.page.locator(SELECTORS.apiKeyRoles.dialogContent);
    this.rolesDialogTitle = this.page.locator(
      SELECTORS.apiKeyRoles.dialogTitle,
    );
    this.rolesDialogClose = this.page.locator(
      SELECTORS.apiKeyRoles.dialogClose,
    );
    this.roleSelectorTrigger = this.page.locator(
      SELECTORS.apiKeyRoles.roleSelectorTrigger,
    );
    this.roleSelectorSearch = this.page.locator(
      SELECTORS.apiKeyRoles.roleSelectorSearch,
    );

    // Success message (toast)
    this.successMessage = this.page.locator(
      '[data-sonner-toast][data-type="success"]',
    );
  }

  // ============================================================================
  // Page Navigation
  // ============================================================================

  /**
   * Navigate to API Keys page
   *
   * @param realmId Realm ID (defaults to 'admin')
   */
  async goto(realmId: string = "admin"): Promise<void> {
    this.currentRealmId = realmId;
    const url = `/manage/api-keys`;
    await this.page.goto(url);
    await this.waitForReady();
  }

  /**
   * Wait for API Keys list page to be visible
   */
  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible();
    await expect(this.heading).toBeVisible();
  }

  // ============================================================================
  // Form Page Navigation
  // ============================================================================

  /**
   * Navigate to the Create API Key page
   *
   * Navigates to /{realmId}/manage/api-keys/new and waits for the form page.
   */
  async gotoCreatePage(): Promise<void> {
    const url = `/${this.currentRealmId}/manage/api-keys/new`;
    await this.page.goto(url);
    await this.waitForFormPage();
    this.logger?.testCode.log("Create API Key form page opened");
  }

  /**
   * Navigate to the Edit API Key page by finding the row by name
   *
   * Finds the key in the table, clicks the edit button in that row, and waits for form page.
   *
   * @param apiKeyName Name of the API key to edit
   */
  async gotoEditPage(apiKeyName: string): Promise<void> {
    // Ensure we are on the list page to find the key
    if (!(await this.container.isVisible().catch(() => false))) {
      await this.goto(this.currentRealmId);
    }

    const row = await this.findRowByName(apiKeyName);
    if (!row) {
      throw new Error(`API Key "${apiKeyName}" not found in table`);
    }

    const editBtn = row.locator(SELECTORS.apiKeys.editButton);
    await this.smartClick(editBtn);
    await this.waitForFormPage();
    this.logger?.testCode.log(`Edit form page opened for "${apiKeyName}"`);
  }

  /**
   * Wait for the form page to be visible
   */
  async waitForFormPage(): Promise<void> {
    await expect(this.formPage).toBeVisible();
    await expect(this.pageTitle).toBeVisible();
  }

  // ============================================================================
  // List Helpers
  // ============================================================================

  /**
   * Find a table row by API key name
   *
   * Uses `expect(...).toBeVisible({ timeout })` so Playwright auto-waits for the
   * row to settle (e.g. React Query resolution). `locator.isVisible({ timeout })`
   * is deprecated and returns immediately, which caused races right after
   * `goto()` — see `waitForApiKeyByName` for the same auto-wait pattern.
   *
   * @param name API key name to find
   * @returns Locator for the table row, or null if not found after waiting
   */
  async findRowByName(name: string): Promise<Locator | null> {
    const nameCell = this.page.getByText(name, { exact: true }).first();
    const isVisible = await expect(nameCell, `API Key "${name}" to be visible`)
      .toBeVisible({ timeout: 5000 })
      .then(() => true)
      .catch(() => false);

    if (!isVisible) {
      return null;
    }

    return nameCell.locator("xpath=ancestor::tr[1]");
  }

  /**
   * Check if an API key exists in the table
   *
   * Auto-waits via `expect(...).toBeVisible({ timeout })` before deciding
   * presence/absence. `locator.isVisible({ timeout })` is deprecated and does
   * not auto-wait — see `waitForApiKeyByName` for the same pattern.
   *
   * @param name API key name
   */
  async apiKeyExists(name: string): Promise<boolean> {
    const nameCell = this.page.getByText(name, { exact: true }).first();
    return await expect(nameCell, `API Key "${name}" to be visible`)
      .toBeVisible({ timeout: 5000 })
      .then(() => true)
      .catch(() => false);
  }

  /**
   * Wait for an API key to appear in the table by name
   *
   * @param name API key name
   * @param timeout Timeout in milliseconds (default: 10000)
   */
  async waitForApiKeyByName(
    name: string,
    timeout: number = 10000,
  ): Promise<void> {
    await expect(this.page.getByText(name, { exact: true })).toBeVisible({
      timeout,
    });
  }

  // ============================================================================
  // Form Helpers
  // ============================================================================

  /**
   * Fill the create form fields
   *
   * @param data Form data: name (required), expiresAt (optional), clientAppId (optional)
   */
  async fillCreateForm(data: {
    name: string;
    expiresAt?: string;
    clientAppId?: string;
  }): Promise<void> {
    await expect(this.nameInput).toBeVisible();
    await this.fillField(this.nameInput, data.name);
    this.logger?.testCode.log(`Filled API Key name: "${data.name}"`);

    if (data.clientAppId) {
      await this.selectClientApp(data.clientAppId);
    }

    if (data.expiresAt) {
      await expect(this.expiresAtInput).toBeVisible();
      await this.expiresAtInput.fill(data.expiresAt);
      this.logger?.testCode.log(`Filled expiresAt: "${data.expiresAt}"`);
    }
  }

  async selectClientApp(clientAppId: string): Promise<void> {
    await this.smartClick(this.clientAppSelectorTrigger);
    await expect(this.clientAppSelectorSearch).toBeVisible({ timeout: 5000 });
    await this.smartClick(
      this.page.locator(
        SELECTORS.apiKeyForm.clientAppSelectorItem(clientAppId),
      ),
    );
    this.logger?.testCode.log(
      `Selected Client App for API Key: "${clientAppId}"`,
    );
  }

  async getClientAppText(apiKeyName: string): Promise<string> {
    const row = await this.findRowByName(apiKeyName);
    if (!row) {
      throw new Error(`API Key "${apiKeyName}" not found`);
    }

    const cell = row.locator(SELECTORS.apiKeys.clientApp);
    await expect(cell).toBeVisible({ timeout: 5000 });
    return (await cell.textContent())?.trim() ?? "";
  }

  /**
   * Fill the edit form fields
   *
   * @param data Form data: name (optional), enabled (optional), expiresAt (optional)
   */
  async fillEditForm(data: {
    name?: string;
    enabled?: boolean;
    expiresAt?: string;
  }): Promise<void> {
    if (data.name !== undefined) {
      await expect(this.nameInput).toBeVisible();
      await this.fillField(this.nameInput, data.name);
      this.logger?.testCode.log(`Updated API Key name: "${data.name}"`);
    }

    if (data.enabled !== undefined) {
      await this.toggleSwitch(this.enabledSwitch, data.enabled, "Enabled");
    }

    if (data.expiresAt !== undefined) {
      await expect(this.expiresAtInput).toBeVisible();
      await this.expiresAtInput.fill(data.expiresAt);
      this.logger?.testCode.log(`Updated expiresAt: "${data.expiresAt}"`);
    }
  }

  /**
   * Submit the form
   *
   * Clicks the submit button and waits for the navigation response.
   * Does NOT wait for toast or specific navigation target -- caller handles subsequent page assertions
   * (e.g., `waitForRevealPage()` after create, `waitForReady()` after edit).
   */
  async submitForm(): Promise<void> {
    await expect(this.submitButton).toBeEnabled();
    // Use waitForResponse to ensure the API call completes before caller asserts next page
    const [response] = await Promise.all([
      this.page.waitForResponse(
        (resp) =>
          resp.url().includes("/api/api-keys/") &&
          resp.status() >= 200 &&
          resp.status() < 300,
        { timeout: 10000 },
      ),
      this.smartClick(this.submitButton),
    ]);
    this.logger?.testCode.log(
      `Submit completed with status ${response?.status() ?? "unknown"}`,
    );
  }

  /**
   * Cancel the form and return to list page
   */
  async cancelForm(): Promise<void> {
    await this.smartClick(this.cancelButton);
    this.logger?.testCode.log("Clicked cancel button");

    // Wait for navigation back to list page
    await expect(this.container).toBeVisible({ timeout: 5000 });
  }

  // ============================================================================
  // Reveal Helpers
  // ============================================================================

  /**
   * Wait for the reveal page to be visible
   *
   * Verifies the page title is "API Key Created".
   */
  async waitForRevealPage(): Promise<void> {
    await expect(this.revealPage).toBeVisible({ timeout: 10000 });
    await expect(this.revealPageTitle).toHaveText("API Key Created");
    this.logger?.testCode.log("API Key reveal page visible");
  }

  /**
   * Get the revealed API key value
   *
   * @returns The plaintext API key string
   */
  async getRevealedKeyValue(): Promise<string> {
    await expect(this.keyValue).toBeVisible();
    const value = await this.keyValue.textContent();
    return value?.trim() || "";
  }

  /**
   * Click copy then done button
   */
  async copyAndDone(): Promise<void> {
    await this.smartClick(this.copyButton);
    this.logger?.testCode.log("Clicked copy button");

    await this.smartClick(this.doneButton);
    this.logger?.testCode.log("Clicked done button");

    // Wait for navigation back to list page
    await expect(this.container).toBeVisible({ timeout: 5000 });
  }

  // ============================================================================
  // Delete Helpers
  // ============================================================================

  /**
   * Delete an API key by name
   *
   * Finds the row, clicks delete, and confirms in the dialog.
   *
   * @param name API key name
   */
  async deleteApiKeyByName(name: string): Promise<void> {
    // Ensure we are on the list page
    if (!(await this.container.isVisible().catch(() => false))) {
      await this.goto(this.currentRealmId);
    }

    const row = await this.findRowByName(name);
    if (!row) {
      throw new Error(`API Key "${name}" not found for deletion`);
    }

    const deleteBtn = row.locator(SELECTORS.apiKeys.deleteButton);
    await this.smartClick(deleteBtn);

    // Wait for delete confirmation dialog
    await expect(this.deleteDialog).toBeVisible({ timeout: 10000 });

    // Click confirm and wait for DELETE API response
    await Promise.all([
      this.page.waitForResponse(
        (resp) =>
          resp.url().includes("/api/api-keys/") &&
          resp.request().method() === "DELETE",
        { timeout: 10000 },
      ),
      this.smartClick(this.deleteConfirmButton),
    ]);

    // Wait for dialog to close
    await expect(this.deleteDialog).toBeHidden({ timeout: 10000 });

    // Wait for the deleted key to disappear from the table (React Query cache invalidation)
    await expect(this.page.getByText(name, { exact: true }).first()).toBeHidden(
      { timeout: 10000 },
    );
    this.logger?.testCode.log(`Deleted API Key "${name}"`);
  }

  /**
   * Open delete dialog for an API key and cancel
   *
   * @param name API key name
   */
  async cancelDeleteDialog(name: string): Promise<void> {
    // Ensure we are on the list page
    if (!(await this.container.isVisible().catch(() => false))) {
      await this.goto(this.currentRealmId);
    }

    const row = await this.findRowByName(name);
    if (!row) {
      throw new Error(`API Key "${name}" not found`);
    }

    const deleteBtn = row.locator(SELECTORS.apiKeys.deleteButton);
    await this.smartClick(deleteBtn);

    // Wait for delete confirmation dialog
    await expect(this.deleteDialog).toBeVisible({ timeout: 10000 });

    // Click cancel
    await this.smartClick(this.deleteCancelButton);

    // Verify dialog closes
    await expect(this.deleteDialog).toBeHidden({ timeout: 5000 });
    this.logger?.testCode.log(`Cancelled delete dialog for "${name}"`);
  }

  // ============================================================================
  // Roles Dialog Helpers
  // ============================================================================

  /**
   * Open the roles dialog for an API key by name
   *
   * Finds the table row by API key name, clicks the manage-roles button,
   * and waits for the roles dialog to appear and the roles API response to complete.
   *
   * @param apiKeyName Name of the API key
   */
  async openRolesDialog(apiKeyName: string): Promise<void> {
    // Ensure we are on the list page
    if (!(await this.container.isVisible().catch(() => false))) {
      await this.goto(this.currentRealmId);
    }

    const row = await this.findRowByName(apiKeyName);
    if (!row) {
      throw new Error(
        `API Key "${apiKeyName}" not found for opening roles dialog`,
      );
    }

    const manageRolesBtn = row.locator(SELECTORS.apiKeys.manageRolesButton);

    // Click manage-roles button and wait for dialog to open.
    // The GET /roles response may be served from React Query cache, so we don't
    // strictly require a network response — the dialog becoming visible is sufficient.
    const responsePromise = this.page
      .waitForResponse(
        (resp) =>
          resp.url().includes("/api/api-keys/") &&
          resp.url().includes("/roles") &&
          resp.request().method() === "GET",
        { timeout: 10000 },
      )
      .catch(() => null);

    await this.smartClick(manageRolesBtn);

    // Wait for dialog to be visible (this is the real gate)
    await expect(this.rolesDialog).toBeVisible({ timeout: 10000 });

    const response = await responsePromise;
    this.logger?.testCode.log(
      `Opened roles dialog for "${apiKeyName}" (status: ${response?.status() ?? "cached"})`,
    );
  }

  /**
   * Select a role in the roles dialog (toggle on).
   * The combobox uses toggle semantics — clicking assigns the role via immediate PUT.
   */
  async selectRoleInDialog(roleName: string, roleId: string): Promise<void> {
    await this.toggleRoleInDialog(roleName, roleId, "Selected");
  }

  /**
   * Deselect a role in the roles dialog (toggle off).
   * The combobox uses toggle semantics — clicking removes the role via immediate PUT.
   */
  async deselectRoleInDialog(roleName: string, roleId: string): Promise<void> {
    await this.toggleRoleInDialog(roleName, roleId, "Deselected");
  }

  /**
   * Close the roles dialog
   */
  async closeRolesDialog(): Promise<void> {
    await this.smartClick(this.rolesDialogClose);
    await expect(this.rolesDialog).toBeHidden({ timeout: 5000 });
    this.logger?.testCode.log("Closed roles dialog");
  }

  /**
   * Get role badge texts for an API key.
   * Returns empty array if em-dash is shown (no roles assigned).
   */
  async getRoleBadgeTexts(apiKeyName: string): Promise<string[]> {
    const rolesCell = await this.getRolesCellForApiKey(apiKeyName);

    const cellText = await rolesCell.textContent();
    if (cellText?.includes("—")) {
      return [];
    }

    const badges = rolesCell.locator(
      '[data-slot="badge"], span[class*="badge"]',
    );
    const count = await badges.count();

    if (count === 0) {
      const text = cellText?.trim();
      return text ? [text] : [];
    }

    const texts: string[] = [];
    for (let i = 0; i < count; i++) {
      const badgeText = await badges.nth(i).textContent();
      if (badgeText?.trim()) {
        texts.push(badgeText.trim());
      }
    }

    return texts;
  }

  /**
   * Check if an API key shows em-dash for roles (no roles assigned).
   */
  async hasEmDashRoleBadge(apiKeyName: string): Promise<boolean> {
    const rolesCell = await this.getRolesCellForApiKey(apiKeyName);
    const cellText = await rolesCell.textContent();
    return cellText?.includes("—") ?? false;
  }

  /**
   * Check if an API key has an overflow badge showing "+N more".
   */
  async hasOverflowBadge(apiKeyName: string): Promise<boolean> {
    const row = await this.findRowByName(apiKeyName);
    if (!row) {
      throw new Error(
        `API Key "${apiKeyName}" not found for checking overflow badge`,
      );
    }

    const overflowBadge = row.locator(SELECTORS.apiKeys.rolesOverflow);
    return await overflowBadge.isVisible().catch(() => false);
  }

  // ============================================================================
  // Private Helpers
  // ============================================================================

  /** Toggle a role on/off in the roles dialog combobox. */
  private async toggleRoleInDialog(
    roleName: string,
    roleId: string,
    action: "Selected" | "Deselected",
  ): Promise<void> {
    await this.smartClick(this.roleSelectorTrigger);

    await expect(this.roleSelectorSearch).toBeVisible({ timeout: 5000 });

    const roleItem = this.page.locator(
      SELECTORS.apiKeyRoles.roleSelectorItem(roleId),
    );

    await Promise.all([
      this.page.waitForResponse(
        (resp) =>
          resp.url().includes("/api/api-keys/") &&
          resp.url().includes("/roles") &&
          resp.request().method() === "PUT" &&
          resp.status() === 200,
        { timeout: 10000 },
      ),
      this.smartClick(roleItem),
    ]);

    const popoverOpen = await this.roleSelectorSearch
      .isVisible({ timeout: 1000 })
      .catch(() => false);
    if (popoverOpen) {
      await this.page.keyboard.press("Escape");
    }

    this.logger?.testCode.log(`${action} role "${roleName}" (${roleId})`);
  }

  /** Find the roles cell for a given API key name. */
  private async getRolesCellForApiKey(apiKeyName: string): Promise<Locator> {
    const row = await this.findRowByName(apiKeyName);
    if (!row) {
      throw new Error(`API Key "${apiKeyName}" not found`);
    }

    const rolesCell = row.locator(SELECTORS.apiKeys.rolesCell);
    await expect(rolesCell).toBeVisible({ timeout: 5000 });
    return rolesCell;
  }

  /**
   * Toggle a switch to the desired state
   */
  private async toggleSwitch(
    locator: Locator,
    desiredState: boolean,
    label: string,
  ): Promise<void> {
    const isChecked = await locator.getAttribute("aria-checked");
    const currentlyEnabled = isChecked === "true";
    if (currentlyEnabled !== desiredState) {
      await locator.click();
      this.logger?.testCode.log(`Toggled ${label}: ${desiredState}`);
    }
  }
}
