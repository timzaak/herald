/**
 * API Key Role Assignment Demo Test
 *
 * User Story: US-RA-017 — API Key Role Assignment Lifecycle
 * - Create API Key without roles, verify em-dash badge
 * - Verify API Key defaults to the admin-api-client Client App scope
 * - Create role with realm:view permission, assign to API Key
 * - Verify role badge updates in table
 * - Verify ext API access granted with role permission
 * - Remove role, verify badge reverts to em-dash
 * - Verify ext API returns 403 after role removal
 *
 * @see ../../../docs/user-stories/core/realm-admin.md
 */

import { test, cleanupTestData, expect } from "../fixtures/demo-page.fixtures";
import { RolesPage } from "../pages/roles-page";
import { callExtApiWithApiKey } from "../helpers/ext-api-helper";

const REALM_ID = "admin";

/**
 * Extract role ID from the roles table by role name.
 *
 * The edit/delete buttons use data-testid="role-edit-button-{roleId}" pattern.
 * We find the row by name, then parse the roleId from the edit button's data-testid.
 */
async function getRoleIdFromTable(
  rolesPage: RolesPage,
  roleName: string,
): Promise<string> {
  const row = rolesPage.findRoleRow(roleName);
  await expect(row).toBeVisible({ timeout: 5000 });

  const editButton = row.locator('[data-testid^="role-edit-button-"]').first();
  await expect(editButton).toBeVisible({ timeout: 5000 });

  const testId = await editButton.getAttribute("data-testid");
  if (!testId) {
    throw new Error(
      `Could not get data-testid from edit button for role "${roleName}"`,
    );
  }

  // testId format: "role-edit-button-{roleId}"
  const roleId = testId.replace("role-edit-button-", "");
  if (!roleId) {
    throw new Error(`Could not parse roleId from data-testid "${testId}"`);
  }

  return roleId;
}

test.describe("[Realm Admin] API Key Role Assignment Demo", () => {
  test.afterEach(async ({ page, apiKeyPage, demoLogger, testStartTime }) => {
    // Clean up: delete test API key (if it exists)
    try {
      const exists = await apiKeyPage.apiKeyExists(
        `Test-API-Key-${testStartTime}`,
      );
      if (exists) {
        await apiKeyPage.goto(REALM_ID);
        await apiKeyPage.deleteApiKeyByName(`Test-API-Key-${testStartTime}`);
      }
    } catch (e) {
      console.log(
        "[Cleanup] API key deletion skipped or failed:",
        (e as Error).message,
      );
    }

    // Clean up: delete test role (if it exists)
    try {
      const cleanupRolesPage = new RolesPage(page, demoLogger);
      await cleanupRolesPage.goto(REALM_ID);
      const roleExists = await cleanupRolesPage.roleExists(
        `Test-API-Role-${testStartTime}`,
      );
      if (roleExists) {
        await cleanupRolesPage.deleteRole(`Test-API-Role-${testStartTime}`);
      }
    } catch (e) {
      console.log(
        "[Cleanup] Role deletion skipped or failed:",
        (e as Error).message,
      );
    }

    // General cleanup
    await cleanupTestData(page, REALM_ID, { timestamp: testStartTime });
  });

  test("API Key Role Assignment Lifecycle", async ({
    page,
    apiKeyPage,
    demoLogger,
    testStartTime,
  }) => {
    const TEST_ROLE_NAME = `Test-API-Role-${testStartTime}`;
    const TEST_API_KEY_NAME = `Test-API-Key-${testStartTime}`;
    let revealedApiKey = "";
    let testRoleId = "";

    // -----------------------------------------------------------------------
    // Step 1: Login as Realm Admin
    // The apiKeyPage fixture already logs in as admin and navigates to API Keys page.
    // -----------------------------------------------------------------------
    await test.step("Given: Admin is logged in and on API Keys page", async () => {
      await expect(apiKeyPage.heading).toBeVisible();
    });

    // -----------------------------------------------------------------------
    // Step 2: Create API Key without roles
    // -----------------------------------------------------------------------
    await test.step("When: Create API Key without roles", async () => {
      await apiKeyPage.gotoCreatePage();
      await expect(apiKeyPage.pageTitle).toHaveText("Create API Key");

      await apiKeyPage.fillCreateForm({ name: TEST_API_KEY_NAME });
      await apiKeyPage.submitForm();

      await apiKeyPage.waitForRevealPage();
      revealedApiKey = await apiKeyPage.getRevealedKeyValue();
      expect(revealedApiKey.length).toBeGreaterThan(0);

      // Return to list page
      await apiKeyPage.smartClick(apiKeyPage.doneButton);
      await expect(apiKeyPage.container).toBeVisible({ timeout: 5000 });
      await apiKeyPage.waitForApiKeyByName(TEST_API_KEY_NAME);
    });

    // -----------------------------------------------------------------------
    // Step 3: Verify no role badge shows em-dash
    // -----------------------------------------------------------------------
    await test.step("Then: Verify em-dash badge for API Key without roles", async () => {
      const badges = await apiKeyPage.getRoleBadgeTexts(TEST_API_KEY_NAME);
      expect(badges).toEqual([]);

      const clientAppText =
        await apiKeyPage.getClientAppText(TEST_API_KEY_NAME);
      expect(clientAppText).toContain("API Key Client");
    });

    // -----------------------------------------------------------------------
    // Step 4: Open Roles dialog for the API Key
    // -----------------------------------------------------------------------
    await test.step("When: Open Roles dialog for the API Key", async () => {
      await apiKeyPage.openRolesDialog(TEST_API_KEY_NAME);

      await expect(apiKeyPage.rolesDialogTitle).toContainText(
        "Manage API Key Roles",
      );
    });

    // -----------------------------------------------------------------------
    // Step 5: Create test role with realm:view, then assign it to the API Key
    // -----------------------------------------------------------------------
    await test.step("When: Create test role with realm:view and assign to API Key", async () => {
      // Close roles dialog to navigate away
      await apiKeyPage.closeRolesDialog();

      // Navigate to Roles page and create a test role
      const rolesPage = new RolesPage(page, demoLogger);
      await rolesPage.goto(REALM_ID);

      await rolesPage.createRole({
        name: TEST_ROLE_NAME,
        description: "Test role for API Key assignment",
      });

      // Assign realm.view permission to the test role
      await rolesPage.clickPermissionsButton(TEST_ROLE_NAME);
      await rolesPage.setPermission("realm.view", true);
      await rolesPage.savePermissions();

      // Extract the roleId from the table (needed for selectRoleInDialog)
      testRoleId = await getRoleIdFromTable(rolesPage, TEST_ROLE_NAME);

      // Navigate back to API Keys page
      await apiKeyPage.goto(REALM_ID);
      await apiKeyPage.waitForApiKeyByName(TEST_API_KEY_NAME);

      // Open roles dialog for the API Key
      await apiKeyPage.openRolesDialog(TEST_API_KEY_NAME);

      // Select the test role (immediate save via PUT)
      await apiKeyPage.selectRoleInDialog(TEST_ROLE_NAME, testRoleId);

      // Verify the role name appears in the dialog (as a badge in the selector trigger area)
      await expect(apiKeyPage.roleSelectorTrigger).toContainText(
        TEST_ROLE_NAME,
      );
    });

    // -----------------------------------------------------------------------
    // Step 6: Close dialog and verify badge update
    // -----------------------------------------------------------------------
    await test.step("Then: Verify role badge updates in table", async () => {
      await apiKeyPage.closeRolesDialog();

      const badges = await apiKeyPage.getRoleBadgeTexts(TEST_API_KEY_NAME);
      expect(badges).toContain(TEST_ROLE_NAME);

      const hasEmDash = await apiKeyPage.hasEmDashRoleBadge(TEST_API_KEY_NAME);
      expect(hasEmDash).toBe(false);
    });

    // -----------------------------------------------------------------------
    // Step 7: Verify API Key can access ext API endpoint
    // -----------------------------------------------------------------------
    await test.step("Then: Verify API Key can access ext API with role permission", async () => {
      const status = await callExtApiWithApiKey(revealedApiKey, "/realms");
      expect(status).toBe(200);
    });

    // -----------------------------------------------------------------------
    // Step 8: Clear roles and verify badge reverts to em-dash
    // -----------------------------------------------------------------------
    await test.step("When: Remove role from API Key", async () => {
      await apiKeyPage.openRolesDialog(TEST_API_KEY_NAME);
      await apiKeyPage.deselectRoleInDialog(TEST_ROLE_NAME, testRoleId);
      await apiKeyPage.closeRolesDialog();

      const hasEmDash = await apiKeyPage.hasEmDashRoleBadge(TEST_API_KEY_NAME);
      expect(hasEmDash).toBe(true);

      const badges = await apiKeyPage.getRoleBadgeTexts(TEST_API_KEY_NAME);
      expect(badges).toEqual([]);
    });

    // -----------------------------------------------------------------------
    // Step 9: Verify ext API returns 403 after role removal
    // -----------------------------------------------------------------------
    await test.step("Then: Verify ext API returns 403 after role removal", async () => {
      // Wait for backend cache invalidation to propagate and retry
      let status = 200;
      for (let i = 0; i < 5; i++) {
        await page.waitForTimeout(500);
        status = await callExtApiWithApiKey(revealedApiKey, "/realms");
        if (status === 403) break;
      }
      expect(status).toBe(403);
    });
  });
});
