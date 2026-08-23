/**
 * User Forbidden Session Revoke Demo Tests — US-RA-021 (scenarios 1–3)
 *
 * User Story:
 *   docs/user-stories/core/realm-admin.md — US-RA-021 (故事 20), scenarios 1–3.
 *
 * Scope: when a Realm Admin sets a user's status to Forbidden via the edit-user
 * dialog, the linkage must revoke ALL of that user's active sessions immediately
 * (design §1.1, §5.3). This file reuses the POM, selectors, and target-user
 * session helper authored in DE-D01 (`user-sessions-management-demo.e2e.ts`).
 *
 * Selector calibration sources (all verified against current frontend):
 *   - frontend/src/components/users/edit-user-dialog.tsx
 *       :88   dialog root            = `user-edit-dialog`
 *       :90   dialog title           = `user-edit-dialog-title`
 *       :109  email input (disabled) = `user-edit-email-input`
 *       :119  nickname input         = `user-edit-nickname-input`
 *       :130  status Select wrapper   = `user-edit-status-select` — NOTE:
 *              this testid is forwarded to Radix Select.Root, which renders
 *              NO DOM element (ui/select.tsx:8-10), so it never appears in
 *              the DOM; the clickable trigger is the dialog's only
 *              `[data-slot="select-trigger"]` (see selectUserStatus below).
 *       :162  submit button          = `user-edit-submit-button`
 *       :129  onValueChange={(value) => field.handleChange(Number(value))}
 *             → option values are numeric status codes as strings.
 *   - frontend/src/components/ui/select.tsx (SelectItem)
 *       `data-value={props.value}` is rendered on each Radix SelectPrimitive.Item,
 *       so a status option can be chosen locale-INDEPENDENTLY via
 *       `[data-value="<status-code>"]` inside `[data-slot="select-content"]`.
 *       This matches the codebase's own `BasePage.selectRadixOption` pattern
 *       (pages/base-page.ts:235-252) and avoids relying on i18n labels
 *       (`m['user_status.forbidden']()` in lib/constants/user.ts).
 *   - frontend/src/lib/constants/user.ts: FORBIDDEN = 2.
 *   - backend/domain/src/user/entities.rs:36-42: `UserStatus::Forbidden = 2`
 *     (WaitVerified=0, Normal=1, Forbidden=2, Invalid=3, Deleted=4).
 *
 * Key assertions are on PERSISTENT business state, not auto-dismissing toasts:
 *   - 401/200 on a protected call made with the target user's Bearer token.
 *   - The edit-success sonner toast (`m['users.user_updated']()`) is NOT used as
 *     a verdict — it is auto-dismissing and locale-dependent. At most it is a
 *     soft signal that the PUT completed; the load-bearing verdicts are the
 *     401 / 200 HTTP statuses below.
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import type { Browser } from '@playwright/test'
import {
  createTargetUserSession,
  createAdminBearerContext,
  assertContextUnauthorized,
  type TargetUserSession,
} from './user-sessions-management-demo.e2e'

// ─── Shared constants ────────────────────────────────────────────────────────

const ADMIN_REALM = 'admin'

/** Dedicated admin-realm target user for US-RA-021 scenarios 1/2/3. */
const FORBIDDEN_USER_EMAIL = 'forbidden-test@demo.com'
const FORBIDDEN_USER_PASSWORD = 'TestPass123!'

/** UserStatus numeric codes (backend/domain/src/user/entities.rs:36-42). */
const STATUS_NORMAL = '1' as const
const STATUS_FORBIDDEN = '2' as const

// ─── Backend base URL (local copy — the sibling sessions demo does not export backendBaseUrl) ───
//
// The sibling `user-sessions-management-demo.e2e.ts` keeps `backendBaseUrl()`
// as a module-local (non-exported) helper. We duplicate it here rather than
// mutate that file's shared infrastructure. The resolution mirrors it and
// helpers/api-validator.ts exactly.

/**
 * Backend base URL. Mirrors the sibling sessions demo's helper
 * (`user-sessions-management-demo.e2e.ts`). Used here only for the target
 * user's OWN protected probe (`/api/auth/status`, exercised with an explicit
 * `Authorization: Bearer` header on the isolated target context) and the
 * target user's OWN re-login probe — both of which carry the target user's
 * bearer explicitly and need no admin token.
 *
 * The admin user-API calls in this file (target-user delete in afterEach) are
 * issued through a Bearer-authenticated `APIRequestContext` built by
 * {@link createAdminBearerContext}: the `/api/users/{realmId}` endpoints are
 * gated on `Authorization: Bearer`, which `page.request.*` cannot supply (the
 * access token lives in SPA memory only). See the sibling sessions demo's
 * `backendBaseUrl` note for the full rationale.
 */
function backendBaseUrl(): string {
  return (
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+$/, ':8080') ||
    'http://localhost:8080'
  )
}

// ─── Protected-call helper (local to this file) ──────────────────────────────

/**
 * Query a protected endpoint with the target user's Bearer token and return the
 * raw HTTP status. Used as a persistent, locale-independent precondition /
 * verdict signal (200 = still authorized, 401 = revoked).
 *
 * Endpoint is the self-service `GET /api/auth/status` — the same architecture-
 * neutral probe `assertContextUnauthorized` (imported from DE-D01) uses. That
 * route is mounted under `token_router` with ONLY `inject_token_identity`
 * (`backend/api/src/application/http/server/mod.rs:617-622`): not first-party-
 * gated, no admin permission, so 200/401 depends purely on Bearer validity.
 * The target user logs in via direct `/api/auth/login` → CustomUserUi token,
 * which the admin `/api/users/{realmId}/{userId}/sessions` endpoint would
 * reject with 403 (first-party credential required, `mod.rs:660-663`), masking
 * the revoke signal.
 *
 * @param t        The target user session (carries context + accessToken + userId).
 * @param realmId  Realm the target user lives in (kept for symmetry with
 *                 `assertContextUnauthorized`; `/api/auth/status` takes no path
 *                 parameter).
 */
async function protectedStatus(
  t: TargetUserSession,
  realmId: string
): Promise<number> {
  const res = await t.context.request.get(
    `${backendBaseUrl()}/api/auth/status`,
    { headers: { Authorization: `Bearer ${t.accessToken}` } }
  )
  return res.status()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/**
 * Track target-user sessions created during a test so afterEach can close their
 * contexts. Each test resets this.
 */
const createdSessions: TargetUserSession[] = []

test.describe('[US-RA-021] Forbidden status linkage revokes all sessions', () => {
  test.afterEach(async ({ usersPage, page, browser }) => {
    // Close every tracked target context (non-fatal).
    for (const t of createdSessions) {
      await t.context.close().catch((error) => {
        console.warn('[user-forbidden afterEach] context close error:', error)
      })
    }
    createdSessions.length = 0

    // Best-effort delete of the dedicated target user (non-fatal). The
    // user-API delete is gated on `Authorization: Bearer`, which
    // `page.request.*` cannot supply (token is SPA in-memory only); build a
    // one-shot admin Bearer context from `browser` instead.
    await deleteExistingUser(browser, FORBIDDEN_USER_EMAIL).catch((error) =>
      console.warn('[user-forbidden afterEach] user cleanup:', error)
    )

    // Shared cleanup for any other stray test data in the admin realm.
    await cleanupTestData(page, ADMIN_REALM, { keepUsers: [] }).catch((error) =>
      console.warn('[user-forbidden afterEach] cleanupTestData:', error)
    )
  })

  test('[US-RA-021 S1] setting status to Forbidden revokes all sessions immediately', async ({
    usersPage,
    page,
    browser,
  }) => {
    // Given: a Normal target user with an active session.
    await test.step('Given the target user has an active session', async () => {
      const session = await createTargetUserSession(
        browser,
        ADMIN_REALM,
        FORBIDDEN_USER_EMAIL,
        FORBIDDEN_USER_PASSWORD,
        page
      )
      createdSessions.push(session)
    })
    const t = createdSessions[0]!

    // Persistent precondition: the target user's protected call SUCCEEDS
    // before any mutation. This guards against a false pass where the session
    // was never valid in the first place.
    await test.step('Then the protected call initially succeeds (200)', async () => {
      expect(await protectedStatus(t, ADMIN_REALM)).toBe(200)
    })

    // When: admin opens the edit-user dialog and sets status to Forbidden.
    await test.step('When admin sets the user status to Forbidden', async () => {
      await usersPage.clickEditUser(FORBIDDEN_USER_EMAIL)

      // No field changes other than the status Select, so no form fill is
      // needed; the status Select is driven via its Radix trigger + data-value
      // option so the choice is locale-independent (see selectUserStatus).
      await selectUserStatus(usersPage, STATUS_FORBIDDEN)

      await usersPage.submitEditUserForm()
    })

    // Primary persistent assertion (NOT a toast): the same protected call now
    // returns 401 — the load-bearing revoke-effect verdict.
    await test.step('Then the protected call returns 401 (sessions revoked)', async () => {
      await assertContextUnauthorized(t, ADMIN_REALM)
    })

    // Additional persistent assertion (US-RA-021 S1: "此后尝试登录被拒绝"):
    // a fresh login for the now-Forbidden user is rejected. Accept 401 OR 403
    // and record which; do not hard-pin (design §1.1).
    await test.step('Then subsequent login is rejected (401 or 403)', async () => {
      const rel = await t.context.request.post(
        `${backendBaseUrl()}/api/auth/${ADMIN_REALM}/login`,
        {
          data: {
            email: FORBIDDEN_USER_EMAIL,
            password: FORBIDDEN_USER_PASSWORD,
            // LoginRequestPayload is `#[serde(rename_all = "camelCase")]`
            // (backend/api-auth/src/login.rs:43-46), so the wire field is
            // `clientId`. A snake_case `client_id` key would be dropped by
            // serde, the required field would be missing, and the login would
            // fail 400 BEFORE the Forbidden-status check — masking the
            // 401/403 verdict this step asserts.
            clientId: t.clientAppId,
          },
          headers: { 'content-type': 'application/json' },
        }
      )
      // Forbidden status blocks login (design §1.1). Either 401 or 403 is
      // acceptable depending on the backend's mapping; we assert membership.
      expect([401, 403]).toContain(rel.status())
    })
  })

  test('[US-RA-021 S2] Forbidden combined with other field changes still revokes', async ({
    usersPage,
    page,
    browser,
  }) => {
    // Given: a Normal target user with an active session.
    await test.step('Given the target user has an active session', async () => {
      const session = await createTargetUserSession(
        browser,
        ADMIN_REALM,
        FORBIDDEN_USER_EMAIL,
        FORBIDDEN_USER_PASSWORD,
        page
      )
      createdSessions.push(session)
    })
    const t = createdSessions[0]!

    await test.step('Then the protected call initially succeeds (200)', async () => {
      expect(await protectedStatus(t, ADMIN_REALM)).toBe(200)
    })

    // When: admin edits the user — changing nickname AND setting status to
    // Forbidden in the same save. This verifies the linkage fires even when
    // Forbidden is one of several changed fields (design §5.3: condition is
    // `new_status == Forbidden && old_status != Forbidden`).
    await test.step('When admin changes nickname and sets status to Forbidden', async () => {
      await usersPage.clickEditUser(FORBIDDEN_USER_EMAIL)

      await usersPage.fillEditUserForm({ nickname: 'forbidden-renamed' })
      await selectUserStatus(usersPage, STATUS_FORBIDDEN)

      await usersPage.submitEditUserForm()
    })

    // Primary persistent assertion: 401 — sessions revoked despite the
    // nickname also being changed.
    await test.step('Then the protected call returns 401 (sessions revoked)', async () => {
      await assertContextUnauthorized(t, ADMIN_REALM)
    })
  })

  test('[US-RA-021 S3] non-Forbidden status change does NOT revoke sessions', async ({
    usersPage,
    page,
    browser,
  }) => {
    // Given: a Normal target user with an active session.
    await test.step('Given the target user has an active session', async () => {
      const session = await createTargetUserSession(
        browser,
        ADMIN_REALM,
        FORBIDDEN_USER_EMAIL,
        FORBIDDEN_USER_PASSWORD,
        page
      )
      createdSessions.push(session)
    })
    const t = createdSessions[0]!

    await test.step('Then the protected call initially succeeds (200)', async () => {
      expect(await protectedStatus(t, ADMIN_REALM)).toBe(200)
    })

    // When: admin edits the user, changing nickname only (status left Normal).
    await test.step('When admin changes nickname only (status stays Normal)', async () => {
      await usersPage.clickEditUser(FORBIDDEN_USER_EMAIL)

      await usersPage.fillEditUserForm({ nickname: 'still-normal-renamed' })

      await usersPage.submitEditUserForm()
    })

    // Persistent assertion: the session survived the non-Forbidden edit.
    await test.step('Then the session is still valid (200)', async () => {
      expect(await protectedStatus(t, ADMIN_REALM)).toBe(200)
    })

    // Negative-control variant (same test to avoid test-data churn): re-edit
    // the user to a different non-Forbidden status — explicitly setting status
    // back to Normal (value "1") — and confirm the session STILL survives. This
    // proves the revoke linkage is gated on the Forbidden value specifically,
    // not on any status mutation.
    await test.step('When admin explicitly re-sets status to Normal (still non-Forbidden)', async () => {
      await usersPage.clickEditUser(FORBIDDEN_USER_EMAIL)

      await selectUserStatus(usersPage, STATUS_NORMAL)

      await usersPage.submitEditUserForm()
    })

    await test.step('Then the session is still valid (200)', async () => {
      expect(await protectedStatus(t, ADMIN_REALM)).toBe(200)
    })
  })
})

// ─── Local helpers ───────────────────────────────────────────────────────────

/**
 * Drive the edit-user dialog's status Select to a given numeric status code.
 *
 * Trigger location note: edit-user-dialog.tsx:130 passes
 * `data-testid="user-edit-status-select"` to the shadcn `Select` wrapper — but
 * that wrapper forwards it to Radix's `SelectPrimitive.Root`, which renders NO
 * DOM element (ui/select.tsx:8-10), so the testid never appears in the DOM.
 * The rendered, clickable element is the `SelectTrigger`
 * (`data-slot="select-trigger"`, ui/select.tsx:22-24) — the edit dialog's
 * status Select is its only trigger.
 *
 * Option selection stays locale-independent: `select.tsx`'s `SelectItem`
 * renders `data-value={props.value}` (ui/select.tsx:141), and the status
 * options' values are the numeric status codes as strings
 * (lib/constants/user.ts getUserStatusOptions). Click the trigger, wait for
 * the `[data-slot="select-content"]` listbox, click the option by value.
 *
 * MUST be called after `usersPage.clickEditUser(email)` has opened the dialog.
 *
 * @param usersPage  The admin UsersPage POM (dialog must be open).
 * @param statusCode Numeric status code as a string ("1"=Normal, "2"=Forbidden).
 */
async function selectUserStatus(
  usersPage: { page: import('@playwright/test').Page },
  statusCode: string
): Promise<void> {
  const page = usersPage.page
  const editDialog = page.locator('[data-testid="user-edit-dialog"]')

  const trigger = editDialog.locator('[data-slot="select-trigger"]')
  await trigger.click()

  const listbox = page.locator('[data-slot="select-content"]')
  await expect(listbox).toBeVisible({ timeout: 3000 })

  const option = listbox.locator(`[data-value="${statusCode}"]`)
  await option.click()

  await expect(listbox).toBeHidden({ timeout: 3000 })
}

/**
 * Idempotent delete of the target user by email via an admin Bearer context.
 * Mirrors DE-D01's `deleteExistingUser`
 * (user-sessions-management-demo.e2e.ts) — duplicated locally because DE-D01
 * keeps it module-local. Non-fatal on 404 / missing user.
 *
 * The `/api/users/{realmId}` search + delete endpoints are gated on
 * `Authorization: Bearer`, which `page.request.*` cannot supply (token is SPA
 * in-memory only — see the sibling sessions demo's `backendBaseUrl` note). A
 * one-shot admin Bearer context is built from `browser` and disposed after the
 * delete.
 */
async function deleteExistingUser(
  browser: Browser,
  email: string
): Promise<void> {
  let adminApi
  try {
    adminApi = await createAdminBearerContext(browser, ADMIN_REALM)
  } catch (error) {
    console.warn(
      `[user-forbidden] admin Bearer setup failed (non-fatal):`,
      error
    )
    return
  }
  try {
    try {
      // Resolve the user id by email via the admin user list API.
      const searchUrl = `${backendBaseUrl()}/api/users/${ADMIN_REALM}?search=${encodeURIComponent(
        email
      )}`
      const searchRes = await adminApi.get(searchUrl)
      if (!searchRes.ok()) {
        const body = await searchRes.text().catch(() => '<unreadable>')
        console.warn(
          `[user-forbidden] user search for ${email} failed ` +
            `(HTTP ${searchRes.status()}): ${body}`
        )
        return
      }
      const searchBody = await searchRes.json()
      const items = (searchBody?.items ?? []) as Array<{
        id: string
        email: string
      }>
      const match = items.find((u) => u.email === email)
      if (!match) {
        return
      }

      const deleteUrl = `${backendBaseUrl()}/api/users/${ADMIN_REALM}/${match.id}`
      const delRes = await adminApi.delete(deleteUrl)
      if (delRes.status() >= 400 && delRes.status() !== 404) {
        const body = await delRes.text().catch(() => '<unreadable>')
        console.warn(
          `[user-forbidden] delete user ${email} (${match.id}) ` +
            `returned HTTP ${delRes.status()}: ${body}`
        )
      }
    } catch (error) {
      console.warn(`[user-forbidden] deleteExistingUser error (non-fatal):`, error)
    }
  } finally {
    await adminApi.dispose().catch(() => {})
  }
}
