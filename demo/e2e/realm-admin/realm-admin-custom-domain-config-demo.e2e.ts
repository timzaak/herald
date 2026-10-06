/**
 * Realm Admin 演示测试 - Custom-domain 配置（单次保存即生效）
 *
 * 用户故事：
 * - [US-CD-001] 场景1（保存自定义域名配置）、场景2（系统提供 CNAME 指引并展示生效状态）、
 *   场景3（自定义域名全局唯一冲突）
 *
 * ⚠️ 范围声明（OUT OF SCOPE / DEFERRED）：
 * 本测试只覆盖 config-admin 单次保存（PUT /config/custom-domain 的 UI 映射 + 持久后端状态）。
 * 终端用户在自定义域名下完成 auth 流（旧 ID US-CD-002）与自定义域名/path-based canonical
 * 域名并存（旧 ID US-CD-004）在现行故事体系（US-CD-001/003/005）中已无对应故事——
 * host-mapping 机制已于 2026-07-09 回退，当前没有任何 host-based 公共入口可断言。
 * US-CD-005（未授权域名访问的拒绝）的 authorize 证书门控是独立 item 的职责，本测试不断言。
 *
 * Teardown 策略：
 * 在 afterEach 中对每个被触及的 realm（admin / realm-001）通过 UI 驱动
 * `SettingsPage.resetCustomDomainConfig()` 清空 hostname（保存 null），该 helper 内部对
 * 所有失败做了兜底（catch + warn），不会硬失败测试运行。
 *
 * 断言策略：
 * - 关键断言落在持久后端状态（GET /api/realms/{realm}/config/custom-domain 返回的
 *   `published.hostname`），而非 sonner/toast 等自动消失提示。
 * - CNAME 指引断言落在 custom-domain-cname-guidance 面板文本（包含配置的 cname_target），
 *   该面板常驻渲染、不会自动消失。
 * - 域名冲突（409）通过直接 PUT 的 409 响应断言（可靠、持久），而非瞬态表单错误。
 *
 * @see frontend/src/components/realm-config/custom-domain-config-form.tsx
 * @see backend/api/src/application/http/realm/custom_domain_config.rs
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginAsAdmin, createBearerApiContext, clearSessionData, DEMO_ADMIN, REALM_ADMINS } from '../helpers/auth'
import { SettingsPage } from '../pages/settings-page'
import { LoginPage } from '../pages/login-page'
import type { CustomDomainFormValues } from '../pages/settings-page'
import type { APIRequestContext } from '@playwright/test'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

/**
 * The configured CNAME target, from `[custom_domain].cname_target` in
 * backend/config/demo.toml. The CNAME guidance panel renders this value; the
 * save test asserts it appears so the panel is never empty.
 */
const CONFIGURED_CNAME_TARGET = 'custom.demo.herald.local'

/**
 * Realms touched by this demo suite (all cleared in afterEach).
 *
 * The uniqueness-conflict scenario uses `admin` + `realm-001` — the two realms
 * the demo seed (`scripts/lib/demo_seed.py`) provisions WITH the
 * `admin-web-console` client app so their admins can log in.
 */
const TOUCHED_REALMS = ['admin', 'realm-001'] as const

/**
 * GET the persisted custom-domain config state for a realm.
 *
 * Uses a Bearer-authenticated `APIRequestContext` (NOT `page.request`):
 * `/api/realms/{realmId}/config/custom-domain` is gated behind
 * `inject_token_identity` + `require_admin_console_token`, which ONLY read the
 * `Authorization: Bearer` header. The access token lives in-memory in the
 * frontend store (`frontend/src/stores/auth-store.ts`, "is NEVER persisted") and
 * is injected per-request by the SPA fetch interceptor, so `page.request.*`
 * (which only inherits browser-context cookies) cannot reach it and would 401.
 *
 * Callers must build the context via {@link createBearerApiContext} from a token
 * captured by `LoginPage.loginAsAdmin` (which awaits the post-switch
 * admin-web-console token). Returns the parsed `CustomDomainConfigStateResponse`
 * body (`{published, cnameTarget, status}`).
 */
async function getCustomDomainState(
  apiContext: APIRequestContext,
  realmId: string,
): Promise<{
  published: { hostname: string | null }
  cnameTarget: string
}> {
  const resp = await apiContext.get(
    `${BASE_URL}/api/realms/${realmId}/config/custom-domain`,
  )
  expect(resp.ok(), `GET custom-domain config for "${realmId}" must succeed`).toBeTruthy()
  return resp.json()
}

/**
 * Log into `realmId` as its admin via the LoginPage page object and return a
 * Bearer-authenticated `APIRequestContext` for direct config-endpoint calls.
 *
 * `LoginPage.loginAsAdmin` awaits `/api/auth/browser-token/switch-client` and
 * captures the post-switch admin-web-console token, which `createBearerApiContext`
 * stamps onto every request as `Authorization: Bearer <token>`.
 *
 * Mirrors the established pattern in
 * `demo/e2e/realm-admin/realm-admin-custom-domain-authorize-gate-demo.e2e.ts`
 * and `demo/e2e/billing-admin/points-grant-sdk-demo.e2e.ts:60-75`.
 */
async function loginAsAdminBearer(
  page: import('@playwright/test').Page,
  realmId: string,
): Promise<APIRequestContext> {
  // Drop any prior session (cookies + storage) so LoginPage.loginAsAdmin lands
  // on a clean /auth/login and logs in as the requested realm's admin. The
  // access token lives only in SPA memory, so without clearing cookies a stale
  // authenticated session survives the page load and the auth gate redirects
  // off the login page before the card renders.
  await clearSessionData(page)
  const loginPage = new LoginPage(page)
  const creds = REALM_ADMINS[realmId] || DEMO_ADMIN
  await loginPage.loginAsAdmin(creds.email, creds.password, realmId)
  return createBearerApiContext(loginPage.getAccessToken())
}

test.describe('[Realm Admin] Custom-domain 配置演示测试', () => {
  let testStartTime: number
  let settingsPage: SettingsPage

  test.afterEach(async ({ page }) => {
    // Clear every realm touched by this suite. resetCustomDomainConfig is
    // best-effort (catches + warns internally) so teardown never hard-fails.
    for (const realm of TOUCHED_REALMS) {
      try {
        const realmSettings = new SettingsPage(page, undefined, realm)
        await loginAsAdmin(page, { realmId: realm, forceRelogin: true })
        await realmSettings.goto()
        await realmSettings.waitForReady()
        await realmSettings.resetCustomDomainConfig()
      } catch (error) {
        console.warn(`[afterEach] clear failed for realm "${realm}":`, error)
      }
    }

    // ⚠️ MANDATORY: 清理测试数据
    await cleanupTestData(page, 'admin', {
      keepUsers: ['admin@cas.com'],
      timestamp: testStartTime,
    })
  })

  // ==========================================================================
  // Test 1 — 保存域名与 CNAME 指引展示
  // 映射: [US-CD-001] 场景1（保存自定义域名）、场景2（CNAME 指引 + 生效状态展示）
  // ==========================================================================

  test('保存域名与 CNAME 指引展示', async ({ page, demoLogger }) => {
    // [US-CD-001] 场景1/场景2
    testStartTime = Date.now()
    const realmId = 'admin'
    settingsPage = new SettingsPage(page, demoLogger, realmId)

    await verifyTestEnvironment(page, {
      requiredRealms: [realmId],
      requiredUsers: ['admin@cas.com'],
      skipRealmVerification: true,
      skipDatabaseCheck: false,
      skipRedisCheck: false,
    })

    const values: CustomDomainFormValues = {
      hostname: 'login.acme.com',
    }

    // 登录并捕获 admin-web-console token 用于 Step D 的直接 GET（admin console
    // 中间件只读 Authorization Bearer 头，page.request 无 token 会 401）。
    const apiContext = await loginAsAdminBearer(page, realmId)
    try {
      await test.step('登录并进入 Custom-domain 配置 Tab', async () => {
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToCustomDomainTab()
      })

      // ----------------------------------------------------------------------
      // Step A [US-CD-001] 场景1: 填入域名并保存 → published.hostname 生效
      // ----------------------------------------------------------------------
      await test.step('Step A [US-CD-001] 场景1: 填入域名并保存', async () => {
        await settingsPage.fillCustomDomainForm(values)
        await settingsPage.saveCustomDomain()
        demoLogger.testCode.log('域名已保存（单次保存即生效）')
      })

      // ----------------------------------------------------------------------
      // Step B [US-CD-001] 场景2: CNAME 指引面板展示配置的 cname_target
      // ----------------------------------------------------------------------
      await test.step('Step B [US-CD-001] 场景2: CNAME 指引面板包含配置的 cname_target', async () => {
        const guidanceText = await settingsPage.getCnameGuidanceText()
        expect(guidanceText.length, 'CNAME guidance panel must render non-empty').toBeGreaterThan(0)
        expect(
          guidanceText,
          `CNAME guidance must contain the configured cname_target "${CONFIGURED_CNAME_TARGET}"`,
        ).toContain(CONFIGURED_CNAME_TARGET)
        demoLogger.testCode.log(`CNAME 指引面板包含 cname_target: ${CONFIGURED_CNAME_TARGET}`)
      })

      // ----------------------------------------------------------------------
      // Step C [US-CD-001] 场景2: CNAME/TLS 生效状态展示（状态徽章存在，非瞬态）
      // ----------------------------------------------------------------------
      await test.step('Step C [US-CD-001] 场景2: CNAME/TLS 状态徽章渲染', async () => {
        await expect(page.getByTestId('custom-domain-status-cname')).toBeVisible({ timeout: 10000 })
        await expect(page.getByTestId('custom-domain-status-tls')).toBeVisible({ timeout: 10000 })
        demoLogger.testCode.log('CNAME/TLS 状态徽章已渲染')
      })

      // ----------------------------------------------------------------------
      // Step D: 通过 GET 断言 published.hostname == 保存的值
      // ----------------------------------------------------------------------
      await test.step('Step D: 通过 GET 断言 published.hostname == 保存值', async () => {
        const state = await getCustomDomainState(apiContext, realmId)
        expect(
          state.published.hostname,
          'published.hostname must equal the just-saved value',
        ).toBe('login.acme.com')
        demoLogger.testCode.log(`published.hostname=${state.published.hostname} (保存生效)`)
      })
    } finally {
      await apiContext.dispose()
    }
  })

  // ==========================================================================
  // Test 2 — 切换域名
  // 映射: [US-CD-001] 场景1（重新保存覆盖当前域名）
  // ==========================================================================

  test('切换域名（重新保存覆盖当前域名）', async ({ page, demoLogger }) => {
    // [US-CD-001] 场景1 — 保存 hostname A，再保存 hostname B → published == B。
    testStartTime = Date.now()
    const realmId = 'admin'
    settingsPage = new SettingsPage(page, demoLogger, realmId)

    await verifyTestEnvironment(page, {
      requiredRealms: [realmId],
      requiredUsers: ['admin@cas.com'],
      skipRealmVerification: true,
      skipDatabaseCheck: false,
      skipRedisCheck: false,
    })

    const hostnameA = 'switch-a.cd.test'
    const hostnameB = 'switch-b.cd.test'

    // 登录并捕获 admin-web-console token 用于直接 GET（同 Test 1）。
    const apiContext = await loginAsAdminBearer(page, realmId)
    try {
      await test.step('登录并进入 Custom-domain 配置 Tab', async () => {
        await settingsPage.goto()
        await settingsPage.waitForReady()
        await settingsPage.switchToCustomDomainTab()
      })

      await test.step('保存 hostname A', async () => {
        await settingsPage.fillCustomDomainForm({ hostname: hostnameA })
        await settingsPage.saveCustomDomain()
        demoLogger.testCode.log(`hostname A (${hostnameA}) 已保存`)
      })

      await test.step('通过 GET 确认 published == A', async () => {
        const state = await getCustomDomainState(apiContext, realmId)
        expect(state.published.hostname).toBe(hostnameA)
      })

      await test.step('保存 hostname B（覆盖 A）', async () => {
        await settingsPage.fillCustomDomainForm({ hostname: hostnameB })
        await settingsPage.saveCustomDomain()
        demoLogger.testCode.log(`hostname B (${hostnameB}) 已保存（覆盖 A）`)
      })

      await test.step('通过 GET 断言 published == B', async () => {
        const state = await getCustomDomainState(apiContext, realmId)
        expect(
          state.published.hostname,
          'published.hostname must equal the newly-saved value',
        ).toBe(hostnameB)
        demoLogger.testCode.log(`published.hostname=${state.published.hostname} (切换生效)`)
      })
    } finally {
      await apiContext.dispose()
    }
  })

  // ==========================================================================
  // Test 3 — 域名全局唯一冲突
  // 映射: [US-CD-001] 场景3（自定义域名全局唯一）
  // ==========================================================================

  test('域名全局唯一冲突', async ({ page, demoLogger }) => {
    // [US-CD-001] 场景3
    // 在 realm-001 保存 unique hostname，然后在 admin 尝试保存相同 hostname → 期望 409。
    // 优先通过直接 PUT 断言 409（可靠、持久），而非瞬态表单错误/toast。
    testStartTime = Date.now()

    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
      skipRealmVerification: true,
      skipDatabaseCheck: false,
      skipRedisCheck: false,
    })

    const conflictRealm = 'realm-001'
    const otherRealm = 'admin'
    const uniqueHostname = 'unique.cd.test'

    await test.step(`在 ${conflictRealm} 保存独特 hostname ${uniqueHostname}`, async () => {
      const conflictSettings = new SettingsPage(page, demoLogger, conflictRealm)
      await loginAsAdmin(page, { realmId: conflictRealm, forceRelogin: true })
      await conflictSettings.goto()
      await conflictSettings.waitForReady()
      await conflictSettings.switchToCustomDomainTab()
      await conflictSettings.fillCustomDomainForm({ hostname: uniqueHostname })
      await conflictSettings.saveCustomDomain()
      demoLogger.testCode.log(`${conflictRealm} 已保存 ${uniqueHostname}`)
    })

    await test.step(`在 ${otherRealm} 直接 PUT 相同 hostname → 期望 409`, async () => {
      // 登录到 otherRealm 并捕获其 admin-web-console token。直接 PUT 端点
      // （/config/custom-domain）被 inject_token_identity + require_admin_console_token
      // 门禁，只读 Authorization Bearer 头；page.request.* 只继承 cookie、无 token
      // 会在冲突检查前就被 401，掩盖真正的 409。改用 bearer context 发起 PUT。
      const apiContext = await loginAsAdminBearer(page, otherRealm)
      try {
        const resp = await apiContext.put(
          `${BASE_URL}/api/realms/${otherRealm}/config/custom-domain`,
          { data: { hostname: uniqueHostname } },
        )

        // 域名全局唯一：otherRealm 不得占用 conflictRealm 已保存的 hostname → 409。
        expect(
          resp.status(),
          `PUT with already-occupied hostname must return 409 (got ${resp.status()})`,
        ).toBe(409)

        const body = await resp.json().catch(() => ({}))
        demoLogger.testCode.log(
          `${otherRealm} PUT ${uniqueHostname} → 409 (body: ${JSON.stringify(body)})`,
        )

        // 额外验证：otherRealm 的 published 未被这次失败的保存写入污染。
        const otherState = await getCustomDomainState(apiContext, otherRealm)
        expect(
          otherState.published.hostname,
          'failed conflict save must NOT mutate otherRealm published config',
        ).not.toBe(uniqueHostname)
        demoLogger.testCode.log(`${otherRealm} published 未被冲突保存污染`)
      } finally {
        await apiContext.dispose()
      }
    })
  })
})
