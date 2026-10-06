/**
 * Credit Bucket Directory Management Demo Tests (Realm Admin)
 *
 * User Stories:
 * - US-CB-001 场景1 (a): admin create + edit a Credit Bucket.
 * - US-CB-001 场景1 (b): a fresh bucket starts with zero distribution-rule
 *   references, while a seeded bucket already targeted by rules exposes those
 *   references read-only in its editor (the current "single registration pool"
 *   contract is expressed through distribution rules, NOT a per-realm flag).
 * - US-CB-001 场景2: disabling a bucket keeps already-held balances intact.
 * - US-CB-002 场景1: a coverage set of ≥1 client app is required (schema
 *   `.min(1)` — fail-loud validation error, no silent submit).
 * - US-CB-003 场景1: an entitlement mapping targets a bucket via a
 *   distribution rule; that binding is surfaced on the bucket as a read-only
 *   `ruleReferences` entry and reflected in `ruleReferenceCount`.
 * - US-CB-001 删除: bucket_in_use 409 blocks an in-use bucket; an empty
 *   throwaway bucket deletes cleanly.
 *
 * Design truth: `backend/api-billing/src/credit_bucket_handlers.rs` —
 * `BucketResponse` carries `ruleReferenceCount` and `BucketDetailResponse`
 * carries read-only `ruleReferences`. Error codes: `bucket_key_duplicate` 400,
 * `bucket_in_use` 409. There is NO `registration_pool_conflict` and NO
 * `receivesRegistrationCredits` field (removed by multi-wallet-grant-rules).
 *
 * Role: realm-admin (`REALM_ADMINS[realmId]`). Navigation is by route
 * `/manage/billing/credit-buckets` — the sidebar entry testid
 * `sidebar-menu-credit-buckets` is i18n-derived and therefore intentionally
 * NOT used (see selectors.ts creditBucket loud note).
 *
 * Test Structure:
 * - `test.describe('[Realm Admin] Credit Bucket 目录管理 (US-CB-001/002/003)')`.
 * - Each `test()` maps to one US scenario with BDD `test.step('Given/When/Then …')`
 *   in Chinese, mirroring `billing-admin/points-admin-comprehensive-demo.e2e.ts`.
 * - `beforeEach`: `verifyTestEnvironment`; `afterEach`: `cleanupTestData`.
 * - All assertions land on persistent state (list-item / detail / disabled-badge
 *   / stable error region / admin-wallets row). Toasts are auxiliary only.
 *
 * Authoring only (DE-D02). Execution is owned by DE-D06 via
 * `--grep "Credit Bucket 目录管理"`. This file is type-checked, not run, here.
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { SELECTORS } from '../selectors'
import { DEMO_ADMIN, REALM_ADMINS, createBearerApiContext } from '../helpers/auth'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { CreditBucketDirectoryPage } from '../pages/credit-bucket-directory-page'
import {
  createBucketViaUI,
  editBucketViaUI,
  openBucketEditor,
  bindCoverageSetViaUI,
  requestDeleteBucketViaUI,
  confirmDeleteBucket,
  parseBucketIdFromListItem,
  listBucketsViaApi,
} from '../helpers/bucket-helpers'
import {
  CREDIT_BUCKET_KEYS,
  CREDIT_BUCKET_REALMS,
} from '../helpers/bucket-seed-ids'

/**
 * Realm used for admin-side directory tests.
 *
 * `realm-001` is the primary demo realm: it seeds the Credit Bucket directory
 * (`primary-pool` registration receiver + `promo-pool` secondary), a points
 * demo user (`user@realm-001.com`) holding balance in `primary-pool`, and
 * one-time entitlement mappings (one intentionally `bucket_id = NULL`).
 */
const REALM_ID = CREDIT_BUCKET_REALMS.POINTS

/**
 * Design truth: NO `isDefault` / `set-default` / `default-bucket` control
 * exists anywhere in the Bucket editor (docs/user-stories/billing/credit-bucket.md;
 * `frontend/src/components/billing/credit-bucket/*.tsx` — verified by grep:
 * no `data-testid` matching these patterns is emitted). The test below asserts
 * this absence with a negative locator so a future regression that reintroduces
 * a default-bucket control is caught loudly.
 */
const A4_DEFAULT_CONTROL_TESTIDS =
  '[data-testid*="is-default"], [data-testid*="set-default"], [data-testid*="default-bucket"]'

test.describe('[Realm Admin] Credit Bucket 目录管理 (US-CB-001/002/003)', () => {
  let testStartTime: number

  test.beforeEach(async ({ page }) => {
    testStartTime = Date.now()
    await verifyTestEnvironment(page, {
      requiredRealms: [REALM_ID],
      requiredUsers: [REALM_ADMINS[REALM_ID]?.email ?? DEMO_ADMIN.email],
    })
  })

  test.afterEach(async ({ page }) => {
    await cleanupTestData(page, REALM_ID, { timestamp: testStartTime })
  })

  // ==========================================================================
  // US-CB-001 场景1 (a): create + edit a Credit Bucket
  // ==========================================================================

  test('US-CB-001 场景1 (a): 创建并编辑 Credit Bucket', async ({ page, loginPage, demoLogger }) => {
    const directory = new CreditBucketDirectoryPage(page, demoLogger)
    const suffix = `${testStartTime}`
    const bucketKey = `e2e-create-${suffix}`
    const initialName = `E2E Create ${suffix}`
    const editedName = `E2E Edited ${suffix}`

    await test.step('Given: realm-admin 已登录', async () => {
      const admin = REALM_ADMINS[REALM_ID] ?? {
        email: DEMO_ADMIN.email,
        password: DEMO_ADMIN.password,
      }
      await loginPage.loginAsAdmin(admin.email, admin.password, REALM_ID)
      demoLogger.testCode.log('[Test] ✓ realm-admin 登录成功')
    })

    await test.step('When: 通过路由访问 Credit Bucket 目录页（非 sidebar testid）', async () => {
      await directory.gotoDirectory(REALM_ID)
      demoLogger.testCode.log('[Test] ✓ 已通过路由进入目录页，规避 i18n 派生 testid')
    })

    let createdBucketId: string
    await test.step('When: 新建 Bucket（填写 bucket_key + name + ≥1 覆盖项）', async () => {
      // Resolve a real client-app id by briefly opening the editor to read the
      // coverage multiselect (lists every client app in the realm). This avoids
      // hardcoding an app UUID that could drift across re-seeds. The create flow
      // itself is then driven by `createBucketViaUI`, which owns navigation +
      // editor open/close as a single atomic helper.
      const coverageId = await discoverFirstCoverageAppId(page, directory)
      createdBucketId = await createBucketViaUI(page, REALM_ID, {
        bucketKey,
        name: initialName,
        description: 'created by DE-D02',
        clientAppIds: [coverageId],
      })
      demoLogger.testCode.log(`[Test] ✓ Bucket 已创建，bucketId=${createdBucketId}`)
    })

    await test.step('Then: 目录列表渲染新 bucket 项；默认启用（无 -disabled-badge）', async () => {
      const item = page.locator(SELECTORS.creditBucket.listItem(createdBucketId!))
      await expect(item).toBeVisible({ timeout: 10000 })
      // Newly created bucket defaults to enabled=true (POST default). The
      // disabled badge is conditionally rendered only when enabled=false, so its
      // absence on the item is the persistent signal of the default-enabled state.
      await expect(
        page.locator(SELECTORS.creditBucket.listItemDisabledBadge(createdBucketId!)),
      ).toHaveCount(0)
      demoLogger.testCode.log('[Test] ✓ 新 bucket 默认启用，未渲染 disabled badge')
    })

    await test.step('When: 编辑 bucket 的 name 字段并保存', async () => {
      await editBucketViaUI(page, createdBucketId!, { name: editedName })
      demoLogger.testCode.log('[Test] ✓ 编辑表单已提交')
    })

    await test.step('Then: 列表项名称更新为编辑后的值（持久化状态）', async () => {
      const item = page.locator(SELECTORS.creditBucket.listItem(createdBucketId!))
      await expect(item).toContainText(editedName)
      // And the original name is no longer the rendered label.
      await expect(item).not.toContainText(initialName)
      demoLogger.testCode.log('[Test] ✓ 列表项名称已更新为编辑值')
    })

    await test.step('Then: A4 — 编辑器内不存在 *is-default* / *set-default* / *default-bucket* 控件', async () => {
      // Open the editor for the just-edited bucket and assert no default-bucket
      // control exists within the editor scope. Documents design decision.
      await openBucketEditor(page, createdBucketId!)
      const editorScope = page.locator(SELECTORS.creditBucket.editor)
      await expect(editorScope.locator(A4_DEFAULT_CONTROL_TESTIDS)).toHaveCount(0)
      demoLogger.testCode.log('[Test] ✓ A4: 编辑器内未发现 default-bucket 控件')
    })
  })

  // ==========================================================================
  // US-CB-001 场景1 (b): rule references — fresh bucket has none; seeded
  // registration-receiver bucket exposes its distribution rules read-only.
  //
  // The legacy "single registration pool per realm + 409 registration_pool_conflict"
  // contract was removed by the multi-wallet-grant-rules refactor: there is no
  // `receivesRegistrationCredits` flag, no registration switch in the editor,
  // and no `registration_pool_conflict` error. The current way a bucket becomes
  // the registration/entitlement receiver is by being targeted by ≥1
  // distribution rule (seeded: `registration` + `topup` rules point at
  // `primary-pool`). This test verifies that real, current contract:
  //   1. A brand-new bucket starts with `ruleReferenceCount = 0` and shows the
  //      editor's empty-rule-references copy.
  //   2. The seeded `primary-pool` (targeted by rules) has `ruleReferenceCount
  //      >= 1` and its editor lists those rule references read-only.
  // ==========================================================================

  test('US-CB-001 场景1 (b): 新建 bucket 无规则引用；被规则指向的 seed bucket 在编辑器只读展示规则引用', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const directory = new CreditBucketDirectoryPage(page, demoLogger)
    const suffix = `${testStartTime}`
    const freshKey = `e2e-fresh-${suffix}`

    await test.step('Given: realm-admin 已登录并进入目录页', async () => {
      const admin = REALM_ADMINS[REALM_ID] ?? {
        email: DEMO_ADMIN.email,
        password: DEMO_ADMIN.password,
      }
      await loginPage.loginAsAdmin(admin.email, admin.password, REALM_ID)
      await directory.gotoDirectory(REALM_ID)
    })

    let freshBucketId: string
    await test.step('Given: 新建一个空 bucket（不指向任何规则）', async () => {
      const coverageId = await discoverFirstCoverageAppId(page, directory)
      freshBucketId = await createBucketViaUI(page, REALM_ID, {
        bucketKey: freshKey,
        name: `E2E Fresh ${suffix}`,
        clientAppIds: [coverageId],
      })
      demoLogger.testCode.log(`[Test] ✓ 空 bucket 已创建，bucketId=${freshBucketId}`)
    })

    await test.step('Then: 列表项/API 显示 ruleReferenceCount=0（无规则指向）', async () => {
      const buckets = await listBucketsViaApi(
        page,
        REALM_ID,
        await createBearerApiContext(loginPage.getAccessToken()),
      )
      const fresh = buckets.find((b) => b.bucketKey === freshKey)
      expect(fresh, 'fresh bucket must be listable via API').toBeTruthy()
      expect(
        fresh!.ruleReferenceCount,
        'a brand-new bucket must have zero distribution-rule references',
      ).toBe(0)
      // The list item footer renders "{ruleReferenceCount} rule references".
      const item = page.locator(SELECTORS.creditBucket.listItem(freshBucketId!))
      await expect(item).toContainText('0 rule references')
      demoLogger.testCode.log('[Test] ✓ 新 bucket ruleReferenceCount=0')
    })

    await test.step('Then: 该空 bucket 编辑器只读展示「无规则引用」空文案', async () => {
      await openBucketEditor(page, freshBucketId!)
      const refsBlock = page.locator(SELECTORS.creditBucket.editorRuleReferences)
      await expect(refsBlock).toBeVisible()
      // The empty copy is a fixed English string in credit-bucket-editor.tsx
      // (`No rules reference this account.`) — not i18n-derived, so stable.
      await expect(refsBlock).toContainText('No rules reference this account')
      demoLogger.testCode.log('[Test] ✓ 空 bucket 编辑器渲染空规则引用文案')
    })

    await test.step('When: 切换到 seed primary-pool（被注册/topup 规则指向）', async () => {
      // `primary-pool` is the bucket targeted by the seeded distribution rules
      // (registration + topup). Resolve its id from the directory list.
      const seededPrimaryId = await parseBucketIdFromListItem(
        page,
        CREDIT_BUCKET_KEYS.PRIMARY_POOL,
      )
      await openBucketEditor(page, seededPrimaryId)
      demoLogger.testCode.log(`[Test] ✓ 已打开 seed primary-pool 编辑器`)
    })

    await test.step('Then: seed bucket API ruleReferenceCount≥1，编辑器只读列出 ≥1 条规则引用', async () => {
      const buckets = await listBucketsViaApi(
        page,
        REALM_ID,
        await createBearerApiContext(loginPage.getAccessToken()),
      )
      const seeded = buckets.find((b) => b.bucketKey === CREDIT_BUCKET_KEYS.PRIMARY_POOL)
      expect(seeded, 'seeded primary-pool must be listable').toBeTruthy()
      expect(
        seeded!.ruleReferenceCount,
        'seeded primary-pool must be targeted by ≥1 distribution rule',
      ).toBeGreaterThanOrEqual(1)
      demoLogger.testCode.log(
        `[Test] ✓ seed primary-pool ruleReferenceCount=${seeded!.ruleReferenceCount}`,
      )

      // The editor surfaces these references read-only (a `<ul>` of rule id +
      // owner type + trigger sources). Assert at least one reference row is
      // rendered — this is the persistent signal that the bucket is a
      // distribution/receiving target (the modern equivalent of the old
      // "registration receiver" badge). The rule id is a UUID printed in
      // `font-mono` inside the list item.
      const refsBlock = page.locator(SELECTORS.creditBucket.editorRuleReferences)
      await expect(refsBlock).toBeVisible()
      const refRows = refsBlock.locator('ul li')
      await expect(refRows.first()).toBeVisible({ timeout: 5000 })
      const refCount = await refRows.count()
      expect(refCount, 'editor must list ≥1 rule reference for primary-pool').toBeGreaterThanOrEqual(1)
      // The seed registers a `registration`-trigger rule on primary-pool; assert
      // that trigger source is represented among the rendered rows.
      await expect(refsBlock).toContainText('registration')
      demoLogger.testCode.log(
        `[Test] ✓ seed primary-pool 编辑器只读列出 ${refCount} 条规则引用（含 registration 触发源）`,
      )
    })
  })

  // ==========================================================================
  // US-CB-002 场景1: coverage set ≥1 required (fail-loud)
  // ==========================================================================

  test('US-CB-002 场景1: 覆盖集 ≥1 必填（空覆盖提交被阻止；绑定后列表项 coveredClientAppCount ≥1）', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const directory = new CreditBucketDirectoryPage(page, demoLogger)
    const suffix = `${testStartTime}`
    const bucketKey = `e2e-coverage-${suffix}`
    const bucketName = `E2E Coverage ${suffix}`

    await test.step('Given: realm-admin 已登录并进入目录页', async () => {
      const admin = REALM_ADMINS[REALM_ID] ?? {
        email: DEMO_ADMIN.email,
        password: DEMO_ADMIN.password,
      }
      await loginPage.loginAsAdmin(admin.email, admin.password, REALM_ID)
      await directory.gotoDirectory(REALM_ID)
    })

    await test.step('When: 尝试在未选择任何覆盖项的情况下提交新建表单', async () => {
      await directory.clickNewBucket()
      await page.locator(SELECTORS.creditBucket.editorBucketKey).fill(bucketKey)
      await page.locator(SELECTORS.creditBucket.editorName).fill(bucketName)
      // Intentionally do NOT pick any coverage item.
      await page.locator(SELECTORS.creditBucket.editorSubmit).click()
    })

    await test.step('Then: 表单 fail-loud —— 覆盖集错误区可见，编辑器保持打开，列表未出现该 bucket', async () => {
      // Schema enforces clientAppIds.min(1); TanStack Form shows the schema error
      // inside the coverage multiselect error region after the submit attempt.
      const coverageError = page.locator(SELECTORS.creditBucket.coverageMultiselectError)
      await expect(coverageError).toBeVisible({ timeout: 5000 })

      // Editor stays open (no successful create).
      await expect(page.locator(SELECTORS.creditBucket.editor)).toBeVisible()

      // No list item with this bucketKey has been created.
      const directoryPage = page.locator(SELECTORS.creditBucket.directoryPage)
      await expect(directoryPage).not.toContainText(bucketKey)
      demoLogger.testCode.log(
        '[Test] ✓ 空覆盖提交被 fail-loud 阻止，未创建 bucket',
      )
    })

    let createdBucketId: string
    await test.step('When: 绑定 ≥1 覆盖项后重新提交', async () => {
      // `discoverFirstCoverageAppId` opens a throwaway create editor to read the
      // coverage options, then navigates back to the directory root (closing
      // that editor). Re-open the create editor here and re-fill the
      // bucketKey/name (the previous attempt's values were never persisted —
      // the empty-coverage submit was rejected by schema validation), then
      // bind the discovered coverage id and submit.
      const coverageId = await discoverFirstCoverageAppId(page, directory)
      await directory.clickNewBucket()
      await page.locator(SELECTORS.creditBucket.editorBucketKey).fill(bucketKey)
      await page.locator(SELECTORS.creditBucket.editorName).fill(bucketName)
      await bindCoverageSetViaUI(page, [coverageId])
      await page.locator(SELECTORS.creditBucket.editorSubmit).click()
      // Editor closes on successful create.
      await expect(page.locator(SELECTORS.creditBucket.editor)).toBeHidden({
        timeout: 15000,
      })
      createdBucketId = await parseBucketIdFromListItem(page, bucketKey)
    })

    await test.step('Then: 列表项持久化渲染 coveredClientAppCount ≥1', async () => {
      // The list item footer renders `m.credit_buckets.covered_apps_count({count})`.
      // Assert the count is non-zero by checking the rendered text contains a
      // digit ≥1 for the coverage count. The list item carries the bucketKey
      // text plus counts in its footer; assert it contains at least one covered
      // app by checking the item text reflects a non-zero coverage.
      const item = page.locator(SELECTORS.creditBucket.listItem(createdBucketId!))
      await expect(item).toBeVisible()
      // Persistent state: the bucketKey is present and the coverage count text
      // is rendered (the exact label is i18n, but the count is a number; verify
      // the item does NOT show the zero-coverage shape by confirming the footer
      // rendered the coverage count segment — match a digit other than 0 in
      // the coverage position is brittle, so instead confirm via API that the
      // bucket has coveredClientAppCount >= 1, the authoritative source).
      const buckets = await listBucketsViaApi(page, REALM_ID, await createBearerApiContext(loginPage.getAccessToken()))
      const created = buckets.find((b) => b.bucketKey === bucketKey)
      expect(created, 'created bucket must be listable via API').toBeTruthy()
      expect(
        created!.coveredClientAppCount,
        'coveredClientAppCount must be >= 1 after binding coverage',
      ).toBeGreaterThanOrEqual(1)
      demoLogger.testCode.log(
        `[Test] ✓ 列表项/API 显示 coveredClientAppCount=${created!.coveredClientAppCount}`,
      )
    })
  })

  // ==========================================================================
  // US-CB-003 场景1: a purchasable mapping targets a bucket via a distribution
  // rule; that ownership is surfaced on the bucket as a read-only
  // `ruleReferences` entry and is stable across editor reopen.
  //
  // The legacy "bucket editor has a mappings multiselect + entitlementMappingCount"
  // contract was removed by the multi-wallet-grant-rules refactor: mapping→bucket
  // ownership is now expressed as distribution rules (configured on the mapping
  // side via the point-rule editor), and the bucket editor only READS those
  // references. The seeded `multi-wallet-topup` mapping targets `primary-pool`
  // via an entitlement_mapping-owned `topup` rule, so `primary-pool` carries an
  // `entitlement_mapping`-owned reference — the real, current expression of
  // "把可购买的套餐归属到积分账户" (US-CB-003 场景1).
  // ==========================================================================

  test('US-CB-003 场景1: 被套餐规则指向的 bucket 持久化展示 mapping 规则引用（只读，重新打开仍在）', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const directory = new CreditBucketDirectoryPage(page, demoLogger)

    await test.step('Given: realm-admin 已登录并进入目录页', async () => {
      const admin = REALM_ADMINS[REALM_ID] ?? {
        email: DEMO_ADMIN.email,
        password: DEMO_ADMIN.password,
      }
      await loginPage.loginAsAdmin(admin.email, admin.password, REALM_ID)
      await directory.gotoDirectory(REALM_ID)
    })

    let seededPrimaryId: string
    let countBefore: number
    await test.step('Given: seed primary-pool 已被套餐映射规则指向（ruleReferenceCount≥1）', async () => {
      const buckets = await listBucketsViaApi(
        page,
        REALM_ID,
        await createBearerApiContext(loginPage.getAccessToken()),
      )
      const seeded = buckets.find((b) => b.bucketKey === CREDIT_BUCKET_KEYS.PRIMARY_POOL)
      expect(seeded, 'seeded primary-pool must be listable').toBeTruthy()
      countBefore = seeded!.ruleReferenceCount
      expect(
        countBefore,
        'seeded primary-pool must be targeted by ≥1 distribution rule (mapping ownership)',
      ).toBeGreaterThanOrEqual(1)
      seededPrimaryId = await parseBucketIdFromListItem(
        page,
        CREDIT_BUCKET_KEYS.PRIMARY_POOL,
      )
      demoLogger.testCode.log(
        `[Test] ℹ seed primary-pool ruleReferenceCount=${countBefore}`,
      )
    })

    await test.step('Then: 编辑器只读列出该套餐映射的规则引用（entitlement_mapping owner，topup 触发源）', async () => {
      await openBucketEditor(page, seededPrimaryId!)
      const refsBlock = page.locator(SELECTORS.creditBucket.editorRuleReferences)
      await expect(refsBlock).toBeVisible()
      // The seeded `multi-wallet-topup` mapping owns a `topup` rule targeting
      // primary-pool; assert that ownership surfaces as an entitlement_mapping
      // reference with a `topup` trigger source. ownerType is rendered with
      // underscores replaced by spaces ("entitlement mapping").
      const refRows = refsBlock.locator('ul li')
      await expect(refRows.first()).toBeVisible({ timeout: 5000 })
      await expect(refsBlock).toContainText('entitlement mapping')
      await expect(refsBlock).toContainText('topup')
      demoLogger.testCode.log(
        '[Test] ✓ 编辑器只读展示 entitlement_mapping 规则引用（topup）',
      )
    })

    await test.step('Then: 重新打开编辑器确认规则引用持久化（稳定只读状态，非 toast）', async () => {
      // Navigate away and back to force a fresh detail refetch, then reopen —
      // the mapping ownership must persist (it is server-side, derived from the
      // distribution rule, not transient editor state).
      await directory.gotoDirectory(REALM_ID)
      await openBucketEditor(page, seededPrimaryId!)
      const refsBlock = page.locator(SELECTORS.creditBucket.editorRuleReferences)
      await expect(refsBlock).toBeVisible()
      const refRows = refsBlock.locator('ul li')
      await expect(refRows.first()).toBeVisible({ timeout: 5000 })

      // Re-read via API to confirm the count is unchanged (ownership persisted).
      const buckets = await listBucketsViaApi(
        page,
        REALM_ID,
        await createBearerApiContext(loginPage.getAccessToken()),
      )
      const seeded = buckets.find((b) => b.bucketKey === CREDIT_BUCKET_KEYS.PRIMARY_POOL)
      expect(seeded, 'seeded primary-pool must still be listable').toBeTruthy()
      expect(
        seeded!.ruleReferenceCount,
        'ruleReferenceCount must be unchanged after editor reopen (ownership persisted)',
      ).toBe(countBefore)
      demoLogger.testCode.log(
        `[Test] ✓ 重新打开编辑器后规则引用仍在，ruleReferenceCount=${seeded!.ruleReferenceCount}`,
      )
    })

    await test.step('Then: 一个 bucket 可归属多个套餐/积分包（同一 bucket 被多条不同规则指向时均列出）', async () => {
      // US-CB-003 acceptance: "一个积分账户可归属多个套餐/积分包". The seed
      // targets BOTH primary-pool and promo-pool with the multi-wallet-topup
      // mapping's two topup rules, and primary-pool also receives registration
      // rules. So primary-pool's editor lists ≥2 reference rows (the topup
      // mapping rule + at least one registration rule), demonstrating that a
      // single bucket aggregates multiple entitlement sources.
      const refsBlock = page.locator(SELECTORS.creditBucket.editorRuleReferences)
      await expect(refsBlock).toBeVisible()
      const refRows = refsBlock.locator('ul li')
      const refCount = await refRows.count()
      expect(
        refCount,
        'primary-pool must aggregate ≥2 rule references (multi-source ownership)',
      ).toBeGreaterThanOrEqual(2)
      demoLogger.testCode.log(
        `[Test] ✓ primary-pool 聚合 ${refCount} 条规则引用（归属多个来源）`,
      )
    })
  })

  // ==========================================================================
  // US-CB-001 场景2: disabling a bucket keeps held balances intact
  // ==========================================================================

  test('US-CB-001 场景2: 禁用 Bucket 后已持有余额保持不变', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    await test.step('Given: realm-admin 已登录', async () => {
      const admin = REALM_ADMINS[REALM_ID] ?? {
        email: DEMO_ADMIN.email,
        password: DEMO_ADMIN.password,
      }
      await loginPage.loginAsAdmin(admin.email, admin.password, REALM_ID)
    })

    const { userId, bucketId, balanceBefore } = await test.step('When: 在 admin wallets 页定位一个有余额的 seed bucket 持有者', async () => {
      // The admin wallets page groups rows by (userId, bucketId). The seeded
      // `primary-pool` always has holders with non-zero balance. Resolve both
      // ids from the rendered row testid rather than hardcoding UUIDs.
      await page.goto(`/manage/points/wallets`)
      await expect(page.locator(SELECTORS.pointsAdmin.accountsPage)).toBeVisible({
        timeout: 10000,
      })

      // Find the first row whose testid encodes the primary-pool bucket. We
      // don't know the seeded bucket's UUID ahead of time, so scan the rendered
      // rows and pick one that exists. If the wallets page is empty (seed drift),
      // fail loud with a clear message — do not fake a holder.
      //
      // The row testid format is `admin-wallet-row-${userId}-${bucketId}` where
      // BOTH ids are UUIDs (each containing 4 hyphens). A greedy `(.+)-(.+)`
      // regex would split at the LAST hyphen and mis-extract — pin to the UUID
      // shape `[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}` so
      // the two ids are unambiguous.
      const anyRow = page.locator('[data-testid^="admin-wallet-row-"]')
      await expect(anyRow.first()).toBeVisible({ timeout: 10000 })
      const rowTestid = await anyRow.first().getAttribute('data-testid')
      const UUID = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}'
      const rowTestidRe = new RegExp(`^admin-wallet-row-(${UUID})-(${UUID})$`)
      expect(rowTestid, 'admin-wallets row testid must encode userId+bucketId').toMatch(
        rowTestidRe,
      )
      const [, userId, bucketId] = rowTestid!.match(rowTestidRe)!

      // Read the held balance TOTAL from this row before disabling. The row
      // renders the bucket total inside a `<div class="text-2xl font-bold">`
      // (PointsWalletsPage.tsx renderWalletRow). Reading the WHOLE row text
      // would be brittle: disabling the bucket later inserts a `Disabled`
      // Badge into the metadata row, which legitimately changes the row text
      // without affecting the balance — comparing whole-row text would then
      // produce a phantom failure. Pin to the total cell so the assertion
      // verifies the actual invariant (held balance unchanged by disable).
      const row = page.locator(SELECTORS.pointsAdmin.walletRowByBucket(userId, bucketId))
      const balanceTotalCell = row.locator('.text-2xl.font-bold').first()
      await expect(balanceTotalCell).toBeVisible()
      const balanceBefore = ((await balanceTotalCell.textContent()) ?? '').trim()
      demoLogger.testCode.log(
        `[Test] ℹ 定位到持有者 userId=${userId} bucketId=${bucketId} 余额=${balanceBefore}`,
      )
      return { userId, bucketId, balanceBefore }
    })

    await test.step('When: 禁用该 bucket（editor-enabled 关闭并保存）', async () => {
      // Navigate back to the directory and disable the bucket via the editor.
      await page.goto(`/manage/billing/credit-buckets`)
      await expect(page.locator(SELECTORS.creditBucket.directoryPage)).toBeVisible()
      await editBucketViaUI(page, bucketId, { enabled: false })
      demoLogger.testCode.log(`[Test] ✓ bucket ${bucketId} 已禁用`)
    })

    await test.step('Then: 目录列表渲染 -disabled-badge', async () => {
      const disabledBadge = page.locator(
        SELECTORS.creditBucket.listItemDisabledBadge(bucketId),
      )
      await expect(disabledBadge).toBeVisible({ timeout: 10000 })
    })

    await test.step('Then: admin wallets 页该持有者余额不变（非零，持久状态）', async () => {
      // Re-read the SAME balance total cell after disabling. Disable affects
      // future grants, not existing holdings (design), so the total
      // must be identical. The `Disabled` Badge is intentionally NOT part of
      // this comparison (it is the expected new UI state, not a balance
      // change).
      await page.goto(`/manage/points/wallets`)
      await expect(page.locator(SELECTORS.pointsAdmin.accountsPage)).toBeVisible()
      const row = page.locator(SELECTORS.pointsAdmin.walletRowByBucket(userId, bucketId))
      await expect(row).toBeVisible({ timeout: 10000 })
      const balanceTotalCell = row.locator('.text-2xl.font-bold').first()
      await expect(balanceTotalCell).toBeVisible()
      const balanceAfter = ((await balanceTotalCell.textContent()) ?? '').trim()
      expect(balanceAfter, 'held balance total must be unchanged after disable').toBe(balanceBefore)
      // Also assert the balance is non-trivial (seeded holder carries balance).
      expect(
        balanceAfter,
        'held balance must be a non-zero numeric total (seeded holder)',
      ).toMatch(/[1-9]/)
      demoLogger.testCode.log(
        `[Test] ✓ 禁用后余额未变: before="${balanceBefore}" after="${balanceAfter}"`,
      )
    })

    await test.step('Cleanup: 恢复 bucket 启用状态（避免污染其他 demo）', async () => {
      await page.goto(`/manage/billing/credit-buckets`)
      await editBucketViaUI(page, bucketId, { enabled: true })
      demoLogger.testCode.log('[Test] ✓ 已恢复 bucket 启用状态')
    })
  })

  // ==========================================================================
  // US-CB-001 delete: blocked when bucket in use; succeeds for empty throwaway
  // ==========================================================================

  test('US-CB-001 删除: 使用中 bucket 被 409 bucket_in_use 阻止；空 throwaway bucket 删除成功', async ({
    page,
    loginPage,
    demoLogger,
  }) => {
    const directory = new CreditBucketDirectoryPage(page, demoLogger)
    await test.step('Given: realm-admin 已登录', async () => {
      const admin = REALM_ADMINS[REALM_ID] ?? {
        email: DEMO_ADMIN.email,
        password: DEMO_ADMIN.password,
      }
      await loginPage.loginAsAdmin(admin.email, admin.password, REALM_ID)
    })

    await test.step('When: 选取一个有活跃持有者/订阅的 seed bucket 并请求删除', async () => {
      await page.goto(`/manage/billing/credit-buckets`)
      await expect(page.locator(SELECTORS.creditBucket.directoryPage)).toBeVisible()

      // The seeded `primary-pool` carries registration + wallet holders + the
      // seeded subscription, so deleting it must hit `bucket_in_use`.
      // LOUD NOTE: if this 409 cannot be reproduced because the seed has been
      // emptied (env drift), the assertion below will fail loudly rather than
      // fake the rejection. See item handoff_summary.
      const seededPrimaryId = await parseBucketIdFromListItem(
        page,
        CREDIT_BUCKET_KEYS.PRIMARY_POOL,
      )
      await requestDeleteBucketViaUI(page, seededPrimaryId)
    })

    await test.step('Then: 确认删除触发 409 bucket_in_use（持久错误区，非 toast）', async () => {
      const outcome = await confirmDeleteBucket(page)
      expect(outcome.success, 'delete of in-use bucket must be refused').toBe(false)
      expect(
        outcome.errorCode,
        'expected bucket_in_use error code in stable error region',
      ).toBe('bucket_in_use')
      // The error message region is persistent (AlertDialog stays open on 409).
      await expect(page.locator(SELECTORS.creditBucket.deleteErrorMessage)).toBeVisible()
      // Cancel out of the dialog so the directory returns to a clean state.
      await page.locator(SELECTORS.creditBucket.deleteCancelButton).click()
      await expect(page.locator(SELECTORS.creditBucket.deleteConfirmDialog)).toBeHidden()
      demoLogger.testCode.log(
        `[Test] ✓ bucket_in_use 409 触发，errorCode=${outcome.errorCode}`,
      )
    })

    let throwawayId: string
    await test.step('When: 新建一个空 throwaway bucket', async () => {
      const suffix = `${testStartTime}`
      const coverageId = await discoverFirstCoverageAppId(page, directory)
      throwawayId = await createBucketViaUI(page, REALM_ID, {
        bucketKey: `e2e-throwaway-${suffix}`,
        name: `E2E Throwaway ${suffix}`,
        clientAppIds: [coverageId],
      })
      await expect(page.locator(SELECTORS.creditBucket.listItem(throwawayId!))).toBeVisible()
    })

    await test.step('Then: 删除空 throwaway bucket 成功（列表项移除）', async () => {
      await requestDeleteBucketViaUI(page, throwawayId!)
      const outcome = await confirmDeleteBucket(page)
      expect(outcome.success, 'empty throwaway bucket delete must succeed').toBe(true)
      // Persistent state: the list item is gone.
      await expect(page.locator(SELECTORS.creditBucket.listItem(throwawayId!))).toHaveCount(0)
      demoLogger.testCode.log('[Test] ✓ 空 throwaway bucket 删除成功，列表项已移除')
    })
  })

  // ==========================================================================
  // Helper: resolve the first coverage app id from the editor multiselect.
  // ==========================================================================

  /**
   * Resolve a client-app id from the coverage multiselect by briefly opening the
   * "new bucket" editor, reading the first item's testid suffix, then navigating
   * back to the directory root. Used to avoid hardcoding client-app UUIDs that
   * could drift across re-seeds. The directory POM is used so the helper leaves
   * the page on the directory page (no open editor) when it returns.
   *
   * Implemented as a module-scope helper so every test in this describe shares
   * one deterministic coverage-id resolution path.
   */
  async function discoverFirstCoverageAppId(
    page: import('@playwright/test').Page,
    directory: CreditBucketDirectoryPage,
  ): Promise<string> {
    // Navigate to the directory root (closes any open editor), then open the
    // create editor to read the coverage multiselect. The multiselect search
    // input lives inside a Radix Popover and is NOT in the DOM until the
    // `bucket-coverage-multiselect` trigger is clicked — click the trigger
    // first, then the search input.
    await directory.gotoDirectory(REALM_ID)
    await directory.clickNewBucket()
    await page.locator(SELECTORS.creditBucket.coverageMultiselect).click()
    const search = page.locator(SELECTORS.creditBucket.coverageMultiselectSearch)
    await expect(search).toBeVisible({ timeout: 5000 })
    await search.click()
    const firstItem = page.locator('[data-testid^="bucket-coverage-multiselect-item-"]').first()
    await expect(firstItem).toBeVisible({ timeout: 5000 })
    const testid = await firstItem.getAttribute('data-testid')
    // Close the popover via Escape (blur does NOT close Radix Popover; without
    // this the popover stays open and interferes with the next editor interaction).
    await page.keyboard.press('Escape')
    await expect(search).toBeHidden({ timeout: 2000 })
    // Return to the directory root so callers (e.g. `createBucketViaUI`) start
    // from a known navigation state.
    await directory.gotoDirectory(REALM_ID)
    return testid!.replace('bucket-coverage-multiselect-item-', '')
  }
})
