/**
 * Realm Admin 综合演示测试 - White-label 配置
 *
 * 用户故事（docs/user-stories/core/white-label.md）：
 * - [US-WL-001] 配置 Realm 品牌资产（logo、主色、背景、页脚、登录/注册文案）
 * - [US-WL-002] 终端用户看到品牌化 auth 流页面（草稿不生效、发布后生效、失效回退）
 * - [US-WL-003] 主色 WCAG AA 对比度安全提示（仅警告不拦截）
 * - [US-WL-004] 资产 URL 引用与失效时的可见回退
 *
 * 覆盖场景：
 * - 草稿不生效、发布后登录页呈现新品牌（draft→publish 生命周期 + 公共登录页渲染）
 * - 跨 Realm 隔离（config 仅作用于本 Realm）
 * - 恢复上一版（draft→publish→restore 生命周期）
 * - logo/background 失效回退
 * - WCAG AA 对比度警告（仅警告不拦截）
 *
 * Teardown 策略（restore-balanced）：
 * White-label 没有现成的 DB 端 demo 清理 helper，因此在 afterEach 中对每个被触及
 * 的 realm（admin/realm1/realm2）通过 UI 驱动 `SettingsPage.resetWhiteLabelConfig()`
 * 发布一份空白的 baseline config：清除所有文本字段、背景类型置为 none、然后 publish。
 * 这样既不遗留 published brand 跨 run 泄漏，也不留下 dangling draft。该 helper 在
 * SettingsPage 内部对所有失败做了兜底（catch + warn），不会硬失败测试运行。
 *
 * 断言策略：
 * - 关键断言落在持久业务状态（公共登录页的 logo src、页脚文案、登录标题/副标题、
 *   草稿提示可见性、背景/主色渲染），而非 sonner/toast 等自动消失提示。
 * - 主色通过 wrapper 根元素的 `--primary` CSS 变量断言（auth-page-wrapper l.144 的
 *   inline style），而非 Tailwind class。
 * - logo 失效回退通过断言 `auth-brand-text`（Herald）可见且 `auth-brand-logo` 不存在
 *   来验证，不依赖破损图标 paint。
 *
 * @see docs/user-stories/core/white-label.md
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginAsAdmin } from '../helpers/auth'
import { SettingsPage } from '../pages/settings-page'
import type { WhiteLabelFormValues } from '../pages/settings-page'
import { SELECTORS } from '../selectors'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

/**
 * A reliably-loadable SVG logo URL used as `logoUrl` for the "brand renders"
 * scenarios (US-WL-002 场景1, cross-realm positive). The file is served by the
 * Vite dev server from `frontend/public/wl-demo-logo.svg` (a real `image/svg+xml`
 * asset, not an SPA-fallback HTML page), so:
 *  - the backend accepts it (`logoUrl must be an http:// or https:// URL`);
 *  - the `<img>` loads → the `auth-page-wrapper` keeps `auth-brand-logo` mounted
 *    AND visible (no `onError` → Herald-text fallback).
 * Contrast with the asset-fallback test (US-WL-002 场景3 / US-WL-004 场景2),
 * which deliberately uses an un-loadable `https://invalid.invalid/...` URL.
 */
const LOADABLE_LOGO_URL = `${BASE_URL}/wl-demo-logo.svg`

/**
 * Realms touched by this demo suite (all restore-balanced in afterEach).
 *
 * Cross-realm isolation uses `admin` + `realm-001` — the two realms the demo
 * seed (`scripts/lib/demo_seed.py`) provisions WITH the `admin-web-console`
 * client app. Realms created via the admin UI at runtime do NOT receive
 * `admin-web-console`, so their admins cannot log in (login returns 400
 * "Client app ... not found"). Using the two seeded realms keeps the
 * cross-realm test self-contained against the seeded environment.
 */
const TOUCHED_REALMS = ['admin', 'realm-001'] as const

/**
 * Open a realm's public login route as an UNAUTHENTICATED viewer.
 *
 * Clears cookies + storage so the page renders the public white-label config
 * (draft config must NOT leak; only published config is served to end users).
 */
async function openPublicLoginPage(page: import('@playwright/test').Page, realmId: string): Promise<void> {
  await page.context().clearCookies()
  try {
    await page.evaluate(() => {
      localStorage.clear()
      sessionStorage.clear()
    })
  } catch {
    // ignore: storage access may be blocked pre-navigation
  }
  await page.goto(`${BASE_URL}/${realmId}/auth/login`, { waitUntil: 'domcontentloaded' })
  // Wait for the auth brand element to render (either logo or Herald text).
  await expect(page.locator(SELECTORS.authBrand.wrapper)).toBeVisible({ timeout: 15000 })
}

/**
 * Read the configured `--primary` CSS variable from the auth-page wrapper root.
 *
 * The wrapper root (auth-page-wrapper.tsx l.144) carries inline style with
 * `--primary` set to the accent color when valid. We read it off the parent of
 * the brand element (the wrapper root is the direct parent of auth-brand-*).
 */
async function getAuthAccentPrimary(page: import('@playwright/test').Page): Promise<string> {
  const brand = page.locator(SELECTORS.authBrand.wrapper).first()
  const value = await brand.evaluate((el) => {
    const wrapper = el.parentElement
    if (!wrapper) return ''
    return getComputedStyle(wrapper).getPropertyValue('--primary').trim()
  })
  return value
}

test.describe('[Realm Admin] White-label 配置综合演示测试', () => {
  let testStartTime: number
  let settingsPage: SettingsPage

  test.afterEach(async ({ page }) => {
    // Restore-balance every realm touched by this suite. resetWhiteLabelConfig
    // is best-effort (catches + warns internally) so teardown never hard-fails.
    for (const realm of TOUCHED_REALMS) {
      try {
        const realmSettings = new SettingsPage(page, undefined, realm)
        await loginAsAdmin(page, { realmId: realm, forceRelogin: true })
        await realmSettings.goto()
        await realmSettings.waitForReady()
        await realmSettings.resetWhiteLabelConfig()
      } catch (error) {
        console.warn(`[afterEach] restore-balance failed for realm "${realm}":`, error)
      }
    }

    // ⚠️ MANDATORY: 清理测试数据
    await cleanupTestData(page, 'admin', {
      keepUsers: ['admin@cas.com'],
      timestamp: testStartTime,
    })
  })

  // ==========================================================================
  // Test 1 — 草稿不生效、发布后登录页呈现新品牌
  // 映射: [US-WL-001]（配置品牌资产）、[US-WL-002] 场景1（登录页呈现 Realm 品牌）
  // 来源: docs/user-stories/core/white-label.md
  // ==========================================================================

  test('草稿不生效、发布后登录页呈现新品牌', async ({ page, demoLogger }) => {
    // [US-WL-001] / [US-WL-002] 场景1 — 来源: docs/user-stories/core/white-label.md
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

    const draftValues: WhiteLabelFormValues = {
      logoUrl: LOADABLE_LOGO_URL,
      accentColor: '#2563eb',
      background: { type: 'gradient', value: 'linear-gradient(135deg, #1e3a8a, #2563eb)' },
      footerText: '© Cas Demo Brand',
      loginTitle: 'Sign in to Cas Demo',
      loginSubtitle: 'Use your Cas Demo account',
      registerTitle: 'Create your Cas Demo account',
      registerSubtitle: 'Start with Cas Demo',
    }

    await test.step('登录并进入 White-label 配置 Tab', async () => {
      await loginAsAdmin(page, { realmId })
      await settingsPage.goto()
      await settingsPage.waitForReady()
      await settingsPage.switchToWhiteLabelTab()
    })

    // ----------------------------------------------------------------------
    // Step A [US-WL-001]: 填写全部品牌资产并保存草稿 → 草稿提示可见
    // ----------------------------------------------------------------------
    await test.step('Step A [US-WL-001]: 填写全部品牌资产并保存草稿', async () => {
      await settingsPage.fillWhiteLabelForm(draftValues)
      await settingsPage.saveDraft()

      // 草稿提示必须可见（draft 已保存或表单为 dirty）
      await expect(page.getByTestId('white-label-draft-notice')).toBeVisible({ timeout: 10000 })
      demoLogger.testCode.log('草稿已保存，draft notice 可见')
    })

    // ----------------------------------------------------------------------
    // Step B [US-WL-002] 场景1 (draft-not-published): 草稿不得泄漏到公共登录页
    // ----------------------------------------------------------------------
    await test.step('Step B [US-WL-002] 场景1: 草稿未发布，公共登录页仍为旧/默认品牌', async () => {
      await openPublicLoginPage(page, realmId)

      // 草稿不得泄漏：公共登录页应显示默认 Herald 文字品牌或上一次发布的品牌，
      // 而非刚保存的草稿 logo URL。
      const logo = page.getByTestId('auth-brand-logo')
      const logoVisible = await logo.isVisible().catch(() => false)
      if (logoVisible) {
        const src = await logo.getAttribute('src')
        // 刚保存的草稿 logo 绝不能出现在公共页（草稿未发布）。
        expect(src).not.toContain('wl-demo-logo.svg')
      } else {
        // 无 logo（Herald 文字回退）或旧品牌 → 草稿确实未泄漏。
        await expect(page.getByTestId('auth-brand-text')).toBeVisible()
      }

      // 页脚同样不应出现草稿文案（除非上一轮发布了相同值，此处只断言不含草稿值）。
      const footer = page.getByTestId('auth-brand-footer')
      const footerVisible = await footer.isVisible().catch(() => false)
      if (footerVisible) {
        const text = (await footer.textContent()) || ''
        expect(text).not.toContain('Cas Demo Brand')
      }
      demoLogger.testCode.log('草稿未泄漏到公共登录页（仍为旧/默认品牌）')
    })

    // ----------------------------------------------------------------------
    // Step C [US-WL-002] 场景1 (publish): 发布后公共登录页呈现新品牌
    // ----------------------------------------------------------------------
    await test.step('Step C [US-WL-002] 场景1: 发布后公共登录页呈现新品牌', async () => {
      // 回到 Settings 重新登录并发布
      await loginAsAdmin(page, { realmId, forceRelogin: true })
      await settingsPage.goto()
      await settingsPage.waitForReady()
      await settingsPage.switchToWhiteLabelTab()

      // 表单应仍持有草稿值（草稿已保存）；直接发布。
      await settingsPage.publish()
      demoLogger.testCode.log('品牌配置已发布')

      // 以未认证身份打开公共登录页，验证发布后的品牌渲染
      await openPublicLoginPage(page, realmId)

      await test.step('logo 渲染为配置的 src', async () => {
        const logo = page.getByTestId('auth-brand-logo')
        await expect(logo).toBeVisible({ timeout: 15000 })
        const src = await logo.getAttribute('src')
        // 发布后的 logo 必须是配置的 URL（不是 Herald 文字回退）。
        expect(src).toContain('wl-demo-logo.svg')
      })

      await test.step('页脚文案匹配配置', async () => {
        const footer = page.getByTestId('auth-brand-footer')
        await expect(footer).toBeVisible({ timeout: 10000 })
        await expect(footer).toContainText('© Cas Demo Brand')
      })

      await test.step('主色通过 --primary CSS 变量应用', async () => {
        // accent 断言落在 wrapper 根元素的 inline --primary（auth-page-wrapper l.144），
        // 而非 class。配置值为 #2563eb。
        const primary = await getAuthAccentPrimary(page)
        // 浏览器可能将 hex 规范化为 rgb()；接受任一形式包含 2563eb 或对应 rgb。
        const normalized = primary.toLowerCase().replace(/\s+/g, '')
        const matchesHex = normalized.includes('#2563eb')
        const matchesRgb = normalized.includes('rgb(37,99,235)')
        expect(matchesHex || matchesRgb).toBeTruthy()
      })

      await test.step('登录标题/副标题渲染配置文案', async () => {
        await expect(page.getByText('Sign in to Cas Demo')).toBeVisible()
        await expect(page.getByText('Use your Cas Demo account')).toBeVisible()
      })

      demoLogger.testCode.log('发布后公共登录页品牌渲染验证通过')
    })
  })

  // ==========================================================================
  // Test 2 — 跨 Realm 隔离
  // 映射: [US-WL-001] 场景1（config 仅作用于本 Realm）、[US-WL-002] 场景1
  // 来源: docs/user-stories/core/white-label.md
  // ==========================================================================

  test('跨 Realm 隔离', async ({ page, demoLogger }) => {
    // [US-WL-001] 场景1 / [US-WL-002] 场景1 — 来源: docs/user-stories/core/white-label.md
    // 使用 demo seed 提供的 admin + realm-001 两个 realm（均含 admin-web-console
    // client app，可登录）；通过 UI 运行时创建的 realm 不带 admin-web-console，
    // 其 admin 无法登录（400 Client app not found）。
    testStartTime = Date.now()

    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
      skipRealmVerification: true,
      skipDatabaseCheck: false,
      skipRedisCheck: false,
    })

    const sourceRealm = 'realm-001'
    const otherRealm = 'admin'

    const sourceBrand: WhiteLabelFormValues = {
      logoUrl: LOADABLE_LOGO_URL,
      accentColor: '#16a34a',
      background: null,
      footerText: '© Realm-001 Isolated Brand',
      loginTitle: 'Sign in to Realm-001',
      loginSubtitle: '',
      registerTitle: '',
      registerSubtitle: '',
    }

    await test.step(`在 ${sourceRealm} 发布独特品牌`, async () => {
      const sourceSettings = new SettingsPage(page, demoLogger, sourceRealm)
      await loginAsAdmin(page, { realmId: sourceRealm, forceRelogin: true })
      await sourceSettings.goto()
      await sourceSettings.waitForReady()
      await sourceSettings.switchToWhiteLabelTab()
      await sourceSettings.fillWhiteLabelForm(sourceBrand)
      await sourceSettings.publish()
      demoLogger.testCode.log(`${sourceRealm} 品牌已发布`)
    })

    await test.step(`${otherRealm} 登录页显示自有品牌（或默认 Herald），不受 ${sourceRealm} 影响`, async () => {
      await openPublicLoginPage(page, otherRealm)

      // otherRealm 必须显示自己的品牌或默认 Herald，绝不能出现 sourceRealm 的品牌值。
      const footer = page.getByTestId('auth-brand-footer')
      const footerVisible = await footer.isVisible().catch(() => false)
      if (footerVisible) {
        const text = (await footer.textContent()) || ''
        expect(text).not.toContain('Realm-001 Isolated Brand')
      }

      const logo = page.getByTestId('auth-brand-logo')
      const logoVisible = await logo.isVisible().catch(() => false)
      if (logoVisible) {
        const src = await logo.getAttribute('src')
        expect(src).not.toContain('wl-demo-logo.svg')
      } else {
        // 默认 Herald 文字回退也是合法的（otherRealm 未配置/已清空品牌）。
        await expect(page.getByTestId('auth-brand-text')).toBeVisible()
      }

      // otherRealm 的 accent 不得是 sourceRealm 的 #16a34a
      const primary = await getAuthAccentPrimary(page)
      const normalized = primary.toLowerCase().replace(/\s+/g, '')
      const isSourceAccent = normalized.includes('#16a34a') || normalized.includes('rgb(22,163,74)')
      expect(isSourceAccent).toBeFalsy()

      demoLogger.testCode.log(`${otherRealm} 登录页品牌与 ${sourceRealm} 隔离验证通过`)
    })

    await test.step(`（正面验证）${sourceRealm} 登录页确实呈现 ${sourceRealm} 品牌`, async () => {
      await openPublicLoginPage(page, sourceRealm)

      const footer = page.getByTestId('auth-brand-footer')
      await expect(footer).toBeVisible({ timeout: 10000 })
      await expect(footer).toContainText('© Realm-001 Isolated Brand')

      const logo = page.getByTestId('auth-brand-logo')
      await expect(logo).toBeVisible({ timeout: 10000 })
      const src = await logo.getAttribute('src')
      expect(src).toContain('wl-demo-logo.svg')

      demoLogger.testCode.log(`${sourceRealm} 登录页品牌呈现验证通过（跨 Realm 隔离正面案例）`)
    })
  })

  // ==========================================================================
  // Test 3 — 恢复上一版
  // 映射: [US-WL-001]（draft→publish→restore 生命周期）、[US-WL-002] 场景1
  // 来源: docs/user-stories/core/white-label.md
  // ==========================================================================

  test('恢复上一版', async ({ page, demoLogger }) => {
    // [US-WL-001] / [US-WL-002] 场景1 — 来源: docs/user-stories/core/white-label.md
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

    // 品牌 A：正确的品牌（绿色 + 正确页脚）
    const brandA: WhiteLabelFormValues = {
      logoUrl: null,
      accentColor: '#15803d',
      background: null,
      footerText: '© Brand A (Good)',
      loginTitle: 'Welcome to Brand A',
      loginSubtitle: '',
      registerTitle: '',
      registerSubtitle: '',
    }

    // 品牌 B：错误/刺眼的品牌（亮红 + 错误页脚）
    const brandB: WhiteLabelFormValues = {
      logoUrl: null,
      accentColor: '#dc2626',
      background: null,
      footerText: '© Brand B (Wrong)',
      loginTitle: 'Oops Wrong Brand B',
      loginSubtitle: '',
      registerTitle: '',
      registerSubtitle: '',
    }

    await test.step('登录并发布「好的」品牌 A', async () => {
      await loginAsAdmin(page, { realmId })
      await settingsPage.goto()
      await settingsPage.waitForReady()
      await settingsPage.switchToWhiteLabelTab()
      await settingsPage.fillWhiteLabelForm(brandA)
      await settingsPage.publish()
      demoLogger.testCode.log('品牌 A 已发布（作为可恢复的上一版）')
    })

    await test.step('发布「错误的」品牌 B', async () => {
      // 重新切到 white-label tab，填充并发布品牌 B（刺眼主色 + 错误页脚）。
      await settingsPage.fillWhiteLabelForm(brandB)
      await settingsPage.publish()
      demoLogger.testCode.log('品牌 B 已发布（当前版本）')
    })

    await test.step('公共登录页当前呈现品牌 B', async () => {
      await openPublicLoginPage(page, realmId)
      const footer = page.getByTestId('auth-brand-footer')
      await expect(footer).toBeVisible({ timeout: 10000 })
      await expect(footer).toContainText('Brand B (Wrong)')
      demoLogger.testCode.log('登录页确认呈现品牌 B')
    })

    await test.step('执行 restore，恢复上一版（品牌 A）', async () => {
      // 回到 Settings 重新登录，驱动 restore 对话框 → 确认。
      await loginAsAdmin(page, { realmId, forceRelogin: true })
      await settingsPage.goto()
      await settingsPage.waitForReady()
      await settingsPage.switchToWhiteLabelTab()

      await settingsPage.restore()
      demoLogger.testCode.log('restore 已确认，上一版（品牌 A）已恢复')
    })

    await test.step('公共登录页回退为品牌 A', async () => {
      await openPublicLoginPage(page, realmId)
      const footer = page.getByTestId('auth-brand-footer')
      await expect(footer).toBeVisible({ timeout: 10000 })
      await expect(footer).toContainText('Brand A (Good)')
      // 不应再呈现品牌 B 的文案
      await expect(page.getByText('Oops Wrong Brand B')).toHaveCount(0)
      demoLogger.testCode.log('登录页已回退为品牌 A（restore 成功）')
    })
  })

  // ==========================================================================
  // Test 4 — logo/background 失效回退
  // 映射: [US-WL-002] 场景3（logo 加载失败回退）、[US-WL-004] 场景2（URL 失效可见回退）
  // 来源: docs/user-stories/core/white-label.md
  // ==========================================================================

  test('logo/background 失效回退', async ({ page, demoLogger }) => {
    // [US-WL-002] 场景3 / [US-WL-004] 场景2 — 来源: docs/user-stories/core/white-label.md
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

    // 不可加载的 logo + 不可加载的背景图，但有效的主色 + 页脚。
    const brokenBrand: WhiteLabelFormValues = {
      logoUrl: 'https://invalid.invalid/logo.png',
      accentColor: '#7c3aed',
      background: { type: 'image', value: 'https://invalid.invalid/bg.jpg' },
      footerText: '© Fallback Demo',
      loginTitle: '',
      loginSubtitle: '',
      registerTitle: '',
      registerSubtitle: '',
    }

    await test.step('发布含失效 URL 的品牌配置（主色 + 页脚有效）', async () => {
      await loginAsAdmin(page, { realmId })
      await settingsPage.goto()
      await settingsPage.waitForReady()
      await settingsPage.switchToWhiteLabelTab()
      await settingsPage.fillWhiteLabelForm(brokenBrand)
      await settingsPage.publish()
      demoLogger.testCode.log('含失效 URL 的品牌配置已发布')
    })

    await test.step('公共登录页：logo 回退为 Herald 文字，无破损 img', async () => {
      await openPublicLoginPage(page, realmId)

      // logo 加载失败 → 回退显示 Herald 文字品牌，且不应存在破损的 <img>。
      await expect(page.getByTestId('auth-brand-text')).toBeVisible({ timeout: 15000 })
      const brokenLogo = page.getByTestId('auth-brand-logo')
      await expect(brokenLogo).toHaveCount(0)
      demoLogger.testCode.log('logo 回退为 Herald 文字（无破损 img）')
    })

    await test.step('公共登录页：背景回退为默认（页面可用，无破损样式）', async () => {
      // 背景图 URL 不可加载 → auth-page-wrapper 通过 Image() preload 失败后回退默认渐变，
      // 不设置 backgroundImage inline style。断言 wrapper 根元素未持有破损的 backgroundImage。
      const brand = page.locator(SELECTORS.authBrand.wrapper).first()
      const bgImage = await brand.evaluate((el) => {
        const wrapper = el.parentElement
        if (!wrapper) return ''
        return getComputedStyle(wrapper).backgroundImage.trim()
      })
      // 回退为默认渐变 class（bg-gradient-to-b）或 none；绝不能引用 invalid.invalid。
      expect(bgImage).not.toContain('invalid.invalid')
      demoLogger.testCode.log('背景回退为默认（未引用失效 URL）')
    })

    await test.step('公共登录页：主色与页脚仍按配置正确渲染', async () => {
      // 主色（有效值 #7c3aed）仍应通过 --primary 应用。
      const primary = await getAuthAccentPrimary(page)
      const normalized = primary.toLowerCase().replace(/\s+/g, '')
      const matchesHex = normalized.includes('#7c3aed')
      const matchesRgb = normalized.includes('rgb(124,58,237)')
      expect(matchesHex || matchesRgb).toBeTruthy()

      // 页脚仍按配置渲染。
      const footer = page.getByTestId('auth-brand-footer')
      await expect(footer).toBeVisible({ timeout: 10000 })
      await expect(footer).toContainText('© Fallback Demo')

      demoLogger.testCode.log('失效 URL 不影响主色与页脚（其他资产正常）')
    })
  })

  // ==========================================================================
  // Test 5 — WCAG AA 对比度警告
  // 映射: [US-WL-003] 场景1（达标不警告）、场景2（不达标仅警告不拦截）
  // 来源: docs/user-stories/core/white-label.md
  // ==========================================================================

  test('WCAG AA 对比度警告', async ({ page, demoLogger }) => {
    // [US-WL-003] 场景1/场景2 — 来源: docs/user-stories/core/white-label.md
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

    await test.step('登录并进入 White-label Tab', async () => {
      await loginAsAdmin(page, { realmId })
      await settingsPage.goto()
      await settingsPage.waitForReady()
      await settingsPage.switchToWhiteLabelTab()
    })

    await test.step('场景2 [US-WL-003]: 低对比度主色 → 显示警告', async () => {
      // #ffff00（纯黄）对白色按钮文字对比度远低于 WCAG AA 4.5:1。
      await settingsPage.fillWhiteLabelForm({ accentColor: '#ffff00' })
      await expect(page.getByTestId('white-label-accent-warning')).toBeVisible({ timeout: 10000 })
      demoLogger.testCode.log('低对比度主色触发警告（场景2）')
    })

    await test.step('场景2 [US-WL-003]: 警告不拦截保存/发布', async () => {
      // 警告存在时仍可保存草稿 + 发布。
      await settingsPage.fillWhiteLabelForm({
        footerText: '© Low Contrast Brand',
        loginTitle: 'Low Contrast Login',
        loginSubtitle: '',
        registerTitle: '',
        registerSubtitle: '',
      })
      await settingsPage.saveDraft()
      await settingsPage.publish()
      demoLogger.testCode.log('低对比度配置保存+发布未被警告拦截')
    })

    await test.step('场景2 [US-WL-003]: 低对比度主色按配置应用于终端用户页面', async () => {
      await openPublicLoginPage(page, realmId)
      // 低对比度主色仍按配置渲染（#ffff00）。
      const primary = await getAuthAccentPrimary(page)
      const normalized = primary.toLowerCase().replace(/\s+/g, '')
      const matchesHex = normalized.includes('#ffff00')
      const matchesRgb = normalized.includes('rgb(255,255,0)')
      expect(matchesHex || matchesRgb).toBeTruthy()
      demoLogger.testCode.log('低对比度主色已按配置应用于登录页')
    })

    await test.step('场景1 [US-WL-003]: 高对比度主色 → 不显示警告', async () => {
      // 回到 Settings，填入高对比度主色（#1d4ed8 对白字对比度 > 4.5:1），警告应消失。
      await loginAsAdmin(page, { realmId, forceRelogin: true })
      await settingsPage.goto()
      await settingsPage.waitForReady()
      await settingsPage.switchToWhiteLabelTab()
      await settingsPage.fillWhiteLabelForm({ accentColor: '#1d4ed8' })
      await expect(page.getByTestId('white-label-accent-warning')).toBeHidden({ timeout: 10000 })
      demoLogger.testCode.log('高对比度主色未触发警告（场景1）')
    })
  })
})
