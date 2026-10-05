/**
 * 认证辅助函数
 *
 * 为 E2E 演示测试提供登录、登出等认证相关功能
 */

import { Page, expect, request, type APIRequestContext, type Response } from '@playwright/test'
import { SELECTORS } from '../selectors'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

// ============================================================================
// Constants
// ============================================================================

/**
 * 默认演示管理员账号
 *
 * admin realm 的管理员账户
 */
export const DEMO_ADMIN = {
  email: 'admin@cas.com',
  password: 'password',
  realmId: 'admin',
}

/**
 * Realm 管理员账户映射
 *
 * `realm-001` entry added by DE-D06 (sanctioned fix per DE-D02 loud note): the
 * credit-bucket admin/balance/purchase demos target realm-001, whose admin is
 * `admin@realm-001.com` (seeded by `scripts/lib/demo_seed.py`::
 * `POINTS_REALM_ADMIN_EMAIL`). Without this entry `REALM_ADMINS[realmId]`
 * returned `undefined` and `loginAsAdmin` fell back to `DEMO_ADMIN`
 * (`admin@cas.com`), which is not a member of realm-001 → login API returned
 * 401 and every credit-bucket admin demo failed at the Given step.
 */
export const REALM_ADMINS: Record<string, { email: string; password: string }> = {
  admin: { email: 'admin@cas.com', password: 'password' },
  realm1: { email: 'realm1-admin@test.com', password: 'password' },
  realm2: { email: 'realm2-admin@test.com', password: 'password' },
  'realm-001': { email: 'admin@realm-001.com', password: 'password' },
}

/**
 * 演示用户账号
 */
export const DEMO_USERS = {
  user1: {
    email: 'user1@demo.com',
    password: 'password123',
    realmId: 'admin',
  },
  user2: {
    email: 'user2@demo.com',
    password: 'password123',
    realmId: 'admin',
  },
}

export async function createBearerApiContext(accessToken: string): Promise<APIRequestContext> {
  if (!accessToken) {
    throw new Error('Cannot create an authenticated API context without an access token')
  }

  return request.newContext({
    extraHTTPHeaders: {
      Authorization: `Bearer ${accessToken}`,
    },
  })
}

// ============================================================================
// Login Functions
// ============================================================================

/**
 * 关闭首登引导欢迎弹窗（如果它已打开）。
 *
 * 每个全新浏览器上下文（Playwright 每个测试独立 context）都没有本地完成
 * 标记，管理员首次进入管理控制台时会渲染首登引导欢迎弹窗（设计行为，
 * US-OG-003）。该弹窗是模态的：打开期间 aria-hidden 遮蔽控制台内容并拦截
 * 点击，任何基于 role 的定位或控制台 UI 操作都会失败。登录助手默认按用户
 * 故事语义立即关闭它（引导可随时关闭、不阻塞控制台）；引导类 demo 用
 * dismissOnboarding: false 保留弹窗做断言。
 */
export async function dismissOnboardingWelcomeDialogIfPresent(page: Page): Promise<void> {
  const dialog = page.locator(SELECTORS.onboarding.welcomeDialog)
  try {
    await dialog.waitFor({ state: 'visible', timeout: 5000 })
  } catch {
    // 本上下文无引导（已有完成标记或存储不可用）：无事可做
    return
  }
  await page.locator(SELECTORS.onboarding.welcomeDismissButton).click()
  await expect(dialog).toBeHidden()
  console.log('[Auth] 已关闭首登引导欢迎弹窗')
}

function isAdminConsoleUrl(url: string): boolean {
  try {
    return /\/manage(\/|$)/.test(new URL(url).pathname)
  } catch {
    return false
  }
}

/**
 * 使用指定凭证登录到指定 Realm
 *
 * @param page Playwright Page 对象
 * @param options 登录参数
 */
export async function loginWithCredentials(
  page: Page,
  options: {
    realmId: string
    email: string
    password: string
    waitNavigation?: boolean
    dismissOnboarding?: boolean
  }
): Promise<void> {
  const { realmId, email, password, waitNavigation = true, dismissOnboarding = true } = options

  console.log(`[Auth] 使用凭证登录到 realm: ${realmId} (email: ${email})`)

  // 清除 session 数据
  await clearSessionData(page)

  // 导航到登录页
  await page.goto(`${BASE_URL}/${realmId}/auth/login`, { waitUntil: 'domcontentloaded' })

  // 检查是否已自动跳转到 dashboard（已有 session）
  if (isAuthenticatedRealmUrl(page.url(), realmId)) {
    console.log(`[Auth] 用户已登录到 realm ${realmId}，跳过登录步骤`)
    return
  }

  // 提交登录表单
  await page.waitForSelector('[data-testid="email-input"]', { timeout: 10000 })
  await page.waitForSelector('[data-testid="password-input"]', { timeout: 10000 })
  await page.fill('[data-testid="email-input"]', email)
  await page.fill('[data-testid="password-input"]', password)
  await page.waitForSelector('[data-testid="login-submit-button"]', { timeout: 5000 })
  await page.click('[data-testid="login-submit-button"]')

  // 等待登录 API 响应
  const loginResponse = await waitForLoginResponse(page)
  if (loginResponse && !loginResponse.ok()) {
    const errorBody = await loginResponse.text().catch(() => 'Unable to read error body')
    console.error(`[Auth] Login API failed: ${loginResponse.status()} - ${errorBody}`)
    throw new Error(`Login failed: API returned ${loginResponse.status()}`)
  }

  await acceptLoginReconsentIfPresent(page, realmId)

  // 验证导航
  if (waitNavigation) {
    await verifyPostLoginNavigation(page, { expectedRoute: 'dashboard', realmId })
  }

  // 首登引导只挂载在管理控制台（/manage 树）；普通用户落在 /user，无需处理
  if (dismissOnboarding && isAdminConsoleUrl(page.url())) {
    await dismissOnboardingWelcomeDialogIfPresent(page)
  }

  console.log(`[Auth] 登录成功`)
}

/**
 * 使用管理员账号登录
 *
 * @param page Playwright Page 对象
 * @param options 可选参数
 * @returns 如果 getToken 为 true，返回 auth token；否则返回 void
 */
export async function loginAsAdmin(
  page: Page,
  options: {
    realmId?: string
    waitNavigation?: boolean
    forceRelogin?: boolean
    totpCode?: string
    getToken?: boolean
    dismissOnboarding?: boolean
  } = {}
): Promise<void | string> {
  const { realmId = 'admin', waitNavigation = true, forceRelogin = false, totpCode, getToken = false, dismissOnboarding = true } = options

  console.log(`[Auth] 使用管理员账号登录到 realm: ${realmId}`)

  const credentials = REALM_ADMINS[realmId] || DEMO_ADMIN
  console.log(`[Auth] 使用凭证: ${credentials.email} (realm: ${realmId})`)

  // 检查当前 session 是否需要重新登录
  let shouldRelogin = forceRelogin

  if (!shouldRelogin) {
    const cookies = await page.context().cookies()
    const xAuthCookie = cookies.find(c => c.name === 'X-Auth')

    if (xAuthCookie && xAuthCookie.value) {
      const cookieEmail = xAuthCookie.value.split('_')[0]
      if (cookieEmail !== credentials.email) {
        console.log(`[Auth] 当前 session 用户与目标用户不匹配，重新登录`)
        shouldRelogin = true
      }
    }
  }

  // 检查是否已在目标 realm 的 dashboard (admin console).
  // Post route-refactor (commit 03eeb456): the admin console moved to the
  // top-level /manage path (NO realm prefix). Recognize both the new top-level
  // /manage and the legacy realm-scoped /{realmId}/manage here.
  const currentUrl = page.url()
  const isOnTargetRealmDashboard = currentUrl.includes(`/${realmId}/manage`)
    || isAdminConsoleUrl(currentUrl)

  if (!shouldRelogin && isOnTargetRealmDashboard) {
    console.log(`[Auth] 用户已登录到 realm ${realmId}，跳过登录步骤`)
    return
  }

  // 清除 session 后重新登录
  if (shouldRelogin || !isOnTargetRealmDashboard) {
    await clearSessionData(page)
  }

  // 导航到登录页
  await page.goto(`${BASE_URL}/${realmId}/auth/login`, { waitUntil: 'domcontentloaded' })

  // 再次检查是否已自动跳转到一个已认证的落地页。
  // 重构后已认证用户访问 auth 页会跳转到顶层 /manage 或 /user。
  const postGotoUrl = page.url()
  const postGotoPath = (() => { try { return new URL(postGotoUrl).pathname } catch { return postGotoUrl } })()
  if (postGotoUrl.includes(`/${realmId}/manage`)
      || postGotoPath.startsWith('/manage')
      || postGotoPath.startsWith('/user')
      || postGotoUrl === `${BASE_URL}/${realmId}`) {
    console.log(`[Auth] 用户已登录到 realm ${realmId}，跳过登录步骤`)
    return
  }

  // 提交登录表单
  await page.waitForSelector('[data-testid="email-input"]', { timeout: 10000 })
  await page.waitForSelector('[data-testid="password-input"]', { timeout: 10000 })
  await page.fill('[data-testid="email-input"]', credentials.email)
  await page.fill('[data-testid="password-input"]', credentials.password)
  await page.waitForSelector('[data-testid="login-submit-button"]', { timeout: 5000 })
  await page.click('[data-testid="login-submit-button"]')

  // 等待登录 API 响应
  const loginResponse = await waitForLoginResponse(page)
  if (loginResponse && !loginResponse.ok()) {
    const errorBody = await loginResponse.text().catch(() => 'Unable to read error body')
    console.error(`[Auth] Login API failed: ${loginResponse.status()} - ${errorBody}`)
    throw new Error(`Login failed: API returned ${loginResponse.status()}`)
  }

  await acceptLoginReconsentIfPresent(page, realmId)

  // 检查是否需要 TOTP 验证
  const totpInput = page.getByTestId('totp-verification-code-input')
  const requiresTOTP = await totpInput.isVisible({ timeout: 2000 }).catch(() => false)

  if (requiresTOTP) {
    console.log(`[Auth] 登录需要 TOTP 验证`)

    if (!totpCode) {
      throw new Error(
        'Login requires TOTP verification but no TOTP code was provided. ' +
        'Please provide the totpCode parameter to loginAsAdmin().'
      )
    }

    // 输入 TOTP 验证码
    console.log(`[Auth] 输入 TOTP 验证码`)
    await expect(totpInput).toBeVisible()
    await totpInput.fill(totpCode)

    // TOTP 表单会在输入 6 位数字后自动提交
    // 等待导航到 dashboard
    console.log(`[Auth] 等待 TOTP 验证完成`)
  }

  // 验证导航
  if (waitNavigation) {
    await verifyPostLoginNavigation(page, { expectedRoute: 'dashboard', realmId })
  }

  // 管理员必然落在管理控制台：默认立即关闭首登引导欢迎弹窗，
  // 保证登录后的控制台步骤无遮挡（引导类 demo 用 dismissOnboarding: false 保留）
  if (dismissOnboarding && isAdminConsoleUrl(page.url())) {
    await dismissOnboardingWelcomeDialogIfPresent(page)
  }

  console.log(`[Auth] 登录成功`)

  // 如果需要 token，提取并返回
  if (getToken) {
    const token = await extractAuthToken(page)
    if (token) {
      console.log(`[Auth] 成功提取 auth token`)
      return token
    } else {
      console.warn(`[Auth] 未能提取 auth token`)
    }
  }
}

/**
 * 登出当前用户
 *
 * @param page Playwright Page 对象
 */
export async function logout(page: Page): Promise<void> {
  console.log('[Auth] 执行登出操作')

  // 尝试通过 UI 点击 logout 按钮
  try {
    const userAvatar = page.locator('[data-testid="user-avatar"]').first()
    if (await userAvatar.isVisible({ timeout: 2000 })) {
      await userAvatar.click()

      const logoutMenuItem = page.locator('[data-testid="logout-menu-item"]').first()
      if (await logoutMenuItem.isVisible({ timeout: 2000 })) {
        await logoutMenuItem.click()
        // 等待 logout 完成并导航到登录页
        await page.waitForURL('**/login', { timeout: 5000 })
        console.log('[Auth] UI 登出成功')
      }
    }
  } catch (error) {
    console.log('[Auth] 登出过程出错，尝试直接清除会话数据')
  } finally {
    // 作为后备方案，清除所有会话数据
    await clearSessionData(page)

    // 导航到登录页确保干净状态。`domcontentloaded` is a more stable readiness
    // signal than `networkidle` for the static login page — `networkidle` can
    // flake-timeout when persistent connections (e.g. TanStack Query
    // refetch intervals, devtools, websocket heartbeats) keep the network busy
    // past the goto timeout, which aborted whole test files in beforeEach.
    await page.goto(`${BASE_URL}/admin/auth/login`, { waitUntil: 'domcontentloaded' })
    console.log('[Auth] 已清除所有会话数据')
  }
}

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * 清除所有会话数据（cookies 和 storage）
 *
 * Exported so admin-setup helpers (e.g. `enableEmailOtpForRealm`) can leave a
 * clean unauthenticated state after they finish — otherwise the next
 * `goto /realm/auth/login` is redirected to `/manage` by the root loader's
 * "authenticated users are sent away from auth pages" guard, and the login
 * card never renders.
 */
export async function clearSessionData(page: Page): Promise<void> {
  await page.context().clearCookies()
  try {
    await page.evaluate(() => {
      localStorage.clear()
      sessionStorage.clear()
    })
  } catch {
    // localStorage 访问被阻止，忽略错误
  }
}

/**
 * 从页面提取 auth token
 * 尝试从多个可能的来源提取 token
 */
async function extractAuthToken(page: Page): Promise<string | null> {
  try {
    // 方法 1: 从 localStorage 获取
    const localToken = await page.evaluate(() => {
      return localStorage.getItem('auth_token') ||
             localStorage.getItem('token') ||
             localStorage.getItem('accessToken')
    })
    if (localToken) {
      console.log(`[Auth] 从 localStorage 提取 token`)
      return localToken
    }

    // 方法 2: 从 cookies 获取
    const cookies = await page.context().cookies()
    const authCookie = cookies.find(c =>
      c.name === 'auth_token' ||
      c.name === 'token' ||
      c.name === 'X-Auth' ||
      c.name === 'session'
    )
    if (authCookie && authCookie.value) {
      console.log(`[Auth] 从 cookie 提取 token: ${authCookie.name}`)
      return authCookie.value
    }

    // 方法 3: 从 sessionStorage 获取
    const sessionToken = await page.evaluate(() => {
      return sessionStorage.getItem('auth_token') ||
             sessionStorage.getItem('token')
    })
    if (sessionToken) {
      console.log(`[Auth] 从 sessionStorage 提取 token`)
      return sessionToken
    }

    console.warn(`[Auth] 未能从任何来源提取 token`)
    return null
  } catch (error) {
    console.error(`[Auth] 提取 token 时出错:`, error)
    return null
  }
}

/**
 * 等待登录 API 响应
 */
async function waitForLoginResponse(page: Page): Promise<Response | null> {
  const loginPromise = page
    .waitForResponse(
      response => response.url().includes('/login') && response.request().method() === 'POST',
      { timeout: 10000 }
    )
    .catch(() => null)

  const loginResponse = await loginPromise
  if (loginResponse) {
    console.log(`[Auth] 登录 API 响应状态: ${loginResponse.status()}`)
  }

  return loginResponse
}

async function acceptLoginReconsentIfPresent(page: Page, realmId: string): Promise<void> {
  const reconsentView = page.locator(SELECTORS.legalConsent.loginReconsentView)
  const needsReconsent = await reconsentView.isVisible({ timeout: 3000 }).catch(() => false)

  if (!needsReconsent) {
    return
  }

  console.log('[Auth] Login-time re-consent required; agreeing to current agreements')
  const agreeButton = page.locator(SELECTORS.legalConsent.loginAgreeAndContinueButton)
  await Promise.all([
    page.waitForURL((url) => !url.pathname.endsWith(`/${realmId}/auth/login`), {
      timeout: 15000,
    }),
    agreeButton.click(),
  ])
}

/**
 * 验证登录后导航
 */
async function verifyPostLoginNavigation(
  page: Page,
  config: { expectedRoute: string; realmId: string; timeout?: number }
): Promise<void> {
  const { expectedRoute, realmId, timeout = 10000 } = config

  console.log(`[Auth] Verifying navigation to ${expectedRoute}...`)

  // First, wait for URL to match an authenticated landing page.
  //
  // Post route-refactor (commit 03eeb456), the expectedRoute for login flows is
  // 'dashboard', but realm-scoped `/${realmId}/dashboard` no longer exists.
  // Admin/regular login now lands on the top-level session-scoped routes
  // /manage or /user/profile (NO realm prefix). Match those directly so this
  // resolves immediately instead of always falling through to the 10s-timeout
  // recovery branch below. The legacy realm-scoped /${realmId}/${expectedRoute}
  // glob is kept as a secondary pattern for backward-compat.
  try {
    await page.waitForURL(
      (url) => {
        const path = url.pathname
        return path.startsWith('/manage')
          || path.startsWith('/user/profile')
          || path.startsWith('/user')
          || path === `/${realmId}`
          || path.startsWith(`/${realmId}/${expectedRoute}`)
      },
      { timeout }
    )
  } catch {
    const currentUrl = page.url()
    console.log(`[Auth] Current URL after timeout: ${currentUrl}`)

    // Check if we're at least on the correct realm root
    if (currentUrl.includes(`/${realmId}/`) || currentUrl === `/${realmId}`) {
      console.log('[Auth] URL matches realm root, continuing...')
    } else if (currentUrl.includes('/login')) {
      throw new Error(`Login failed: Still on login page`)
    }
  }

  if (expectedRoute === 'dashboard') {
    const currentUrl = page.url()
    console.log(`[Auth] Current URL: ${currentUrl}`)

    // 登录后可能落在管理端首页，也可能落在普通用户的 profile / points 页面。
    if (!isAuthenticatedRealmUrl(currentUrl, realmId)) {
      throw new Error(`Expected to be on an authenticated realm page, but URL is: ${currentUrl}`)
    }

    // Quick check if page has loaded (with short timeout to avoid blocking)
    try {
      await page.waitForLoadState('domcontentloaded', { timeout: 3000 })
    } catch {
      console.log('[Auth] domcontentloaded timeout, but URL is correct, continuing...')
    }

    console.log('[Auth] Dashboard navigation verified')
  }
}

function isAuthenticatedRealmUrl(currentUrl: string, realmId: string): boolean {
  const normalizedBaseUrl = BASE_URL.replace(/\/$/, '')
  const rootUrl = `${normalizedBaseUrl}/${realmId}`
  const allowedPrefixes = [
    `${rootUrl}/manage`,
    `${rootUrl}/user`,
    `${rootUrl}/profile`,
  ]

  if (currentUrl === rootUrl) {
    return true
  }

  if (allowedPrefixes.some(prefix => currentUrl.startsWith(prefix))) {
    return true
  }

  // Session-scoped裸路径（无 realm 前缀）也是合法的认证后落地页。
  // custom-domain 双模式路由（commit e1ec3a98）后，admin 登录落地 /manage、
  // 普通用户落地 /user/profile —— 这些段是 session-scoped（realm 从 session
  // 取，不在 URL 里），由 frontend/src/lib/realm-routing.ts `realmPath()` 决定。
  // 对应的前端路由 frontend/src/routes/manage/ 与 routes/user/ 同样合法。
  // 见 frontend/src/lib/constants/auth-constants.ts DEFAULT_ADMIN_REDIRECT。
  const sessionScopedPrefixes = [
    `${normalizedBaseUrl}/manage`,
    `${normalizedBaseUrl}/user`,
  ]
  return sessionScopedPrefixes.some(prefix => currentUrl.startsWith(prefix))
}
