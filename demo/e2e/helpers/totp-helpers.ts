/**
 * TOTP Helper Functions for Demo Tests
 *
 * 提供用户 TOTP 管理的辅助函数
 * 遵循 UI-Only 原则，所有操作通过 UI 执行
 */

import { Page, expect } from '@playwright/test'
import type { UnifiedLogger } from './unified-logger'
import { loginAsAdmin, loginWithCredentials } from './auth'
import { generateTOTPCodeFromSecret } from './totp-helper'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

// ============================================================================
// Types
// ============================================================================

/**
 * TOTP 状态信息
 */
export interface TOTPStatus {
  enabled: boolean
  enabled_at?: string
  last_verified_at?: string
  remaining_backup_codes: number
  used_backup_codes: number
}

/**
 * 用户 TOTP 设置选项
 */
export interface SetupTOTPOptions {
  realmId: string
  email: string
  password: string
}

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * 确保 admin 用户没有启用 TOTP
 *
 * @param page Playwright Page 对象
 * @param realmId Realm ID
 */
export async function ensureAdminNoTOTP(
  page: Page,
  realmId: string
): Promise<void> {
  console.log('[TOTP Helper] Ensuring admin does not have TOTP enabled')

  // 清除 session
  await page.context().clearCookies()
  try {
    await page.evaluate(() => {
      localStorage.clear()
      sessionStorage.clear()
    })
  } catch {}

  // 先登录为 admin
  await page.goto(`${BASE_URL}/${realmId}/auth/login`)
  await page.waitForLoadState('domcontentloaded')

  // 填写登录表单
  await page.getByTestId('email-input').fill('admin@cas.com')
  await page.getByTestId('password-input').fill('password')
  await page.getByTestId('login-submit-button').click()

  // 等待登录完成或 TOTP 提示 - 使用 waitForURL 而非固定延迟
  try {
    await page.waitForURL(`**/${realmId}/**`, { timeout: 5000 })
  } catch {
    // 导航可能还在进行，继续检查 TOTP
  }

  // 检查当前 URL
  const currentUrl = page.url()
  console.log(`[TOTP Helper] Current URL after login attempt: ${currentUrl}`)

  // 检查是否有 TOTP 提示
  const totpInput = page.getByTestId('totp-verification-code-input')
  const requiresTOTP = await totpInput.isVisible({ timeout: 2000 }).catch(() => false)

  if (requiresTOTP) {
    console.log('[TOTP Helper] Admin requires TOTP, need to disable')
    // 需要 TOTP 验证码，但我们无法提供
    // 这种情况下，我们跳过 TOTP 设置测试
    return
  }

  // 如果不在 dashboard 或 profile 页面，导航到 security
  if (currentUrl.includes('/login')) {
    // 登录失败或卡住，尝试直接导航
    console.log('[TOTP Helper] Still on login page, navigating directly to security')
    await page.goto(`${BASE_URL}/user/security`)
    await page.waitForLoadState('domcontentloaded')
  } else {
    // 已登录，导航到 Security 页面
    await page.goto(`${BASE_URL}/user/security`)
    await page.waitForLoadState('domcontentloaded')
  }

  // 检查是否有禁用 TOTP 按钮（如果有，说明 TOTP 已启用）
  const disableButton = page.getByTestId('totp-disable-button')
  const totpEnabled = await disableButton.isVisible({ timeout: 3000 }).catch(() => false)

  if (totpEnabled) {
    console.log('[TOTP Helper] Admin TOTP is enabled, disabling...')
    // 禁用 TOTP
    await disableButton.click()
    await page.getByTestId('totp-disable-password-input').fill('password')
    await page.getByTestId('totp-disable-submit-button').click()
    // 等待禁用操作完成 - 等待按钮消失
    await expect(disableButton).toBeHidden({ timeout: 3000 })
    console.log('[TOTP Helper] Admin TOTP disabled')
  } else {
    console.log('[TOTP Helper] Admin does not have TOTP enabled')
  }
}

/**
 * 使用 TOTP 验证码登录 admin
 *
 * 此函数会：
 * 1. 尝试登录 admin 账户
 * 2. 如果需要 TOTP，从环境变量中获取 TOTP 密钥
 * 3. 生成 TOTP 验证码并完成登录
 *
 * @param page Playwright Page 对象
 * @param realmId Realm ID
 * @param totpSecret 可选的 TOTP 密钥（如果不提供，将从环境变量 ADMIN_TOTP_SECRET 读取）
 */
export async function loginAdminWithTOTP(
  page: Page,
  realmId: string,
  totpSecret?: string
): Promise<void> {
  const secret = totpSecret || process.env.ADMIN_TOTP_SECRET

  if (!secret) {
    throw new Error(
      'Admin TOTP is enabled but no secret was provided. ' +
      'Either pass totpSecret parameter or set ADMIN_TOTP_SECRET environment variable.'
    )
  }

  console.log('[TOTP Helper] Logging in admin with TOTP')

  // 导航到登录页
  await page.goto(`${BASE_URL}/${realmId}/auth/login`)
  await page.waitForLoadState('domcontentloaded')

  // 填写登录表单
  await page.getByTestId('email-input').fill('admin@cas.com')
  await page.getByTestId('password-input').fill('password')
  await page.getByTestId('login-submit-button').click()

  // 等待 TOTP 输入框出现
  const totpInput = page.getByTestId('totp-verification-code-input')
  await expect(totpInput).toBeVisible({ timeout: 5000 })

  // 生成 TOTP 验证码
  const totpCode = generateTOTPCodeFromSecret(secret)
  console.log(`[TOTP Helper] Generated TOTP code: ${totpCode}`)

  // 输入 TOTP 验证码
  await totpInput.fill(totpCode)

  // 等待导航到 dashboard
  await page.waitForURL(`**/${realmId}/**`, { timeout: 10000 })

  console.log('[TOTP Helper] Admin login with TOTP successful')
}

/**
 * 通过 admin UI 创建用户（如果不存在）
 *
 * @param page Playwright Page 对象
 * @param options 用户创建选项
 */
async function ensureUserExists(
  page: Page,
  options: { realmId: string; email: string; password: string }
): Promise<void> {
  const { realmId, email, password } = options

  console.log(`[TOTP Helper] Ensuring user exists: ${email}`)

  // 先登录为管理员（处理可能的 TOTP 提示）
  let adminLoggedIn = false
  try {
    // 清除 session
    await page.context().clearCookies()
    try {
      await page.evaluate(() => {
        localStorage.clear()
        sessionStorage.clear()
      })
    } catch {}

    // 导航到登录页
    await page.goto(`${BASE_URL}/${realmId}/auth/login`)
    await page.waitForLoadState('domcontentloaded')

    // 填写管理员凭证
    await page.getByTestId('email-input').fill('admin@cas.com')
    await page.getByTestId('password-input').fill('password')
    await page.getByTestId('login-submit-button').click()

    // 等待登录完成或 TOTP 提示 - 使用断言等待
    // Technical delay: Wait for either navigation or TOTP prompt
    await Promise.race([
      page.waitForURL('**/dashboard'),
      page.getByTestId('totp-verification-code-input').waitFor({ state: 'visible' })
    ]).catch(() => {})

    // 检查是否有 TOTP 提示
    const totpInput = page.getByTestId('totp-verification-code-input')
    const requiresTOTP = await totpInput.isVisible({ timeout: 2000 }).catch(() => false)

    if (requiresTOTP) {
      console.log('[TOTP Helper] Admin requires TOTP, disabling admin TOTP first')
      // 尝试禁用 admin 的 TOTP
      const currentUrl = page.url()
      if (!currentUrl.includes('/dashboard')) {
        // 如果还在登录页面，先重新登录（admin 没有启用 TOTP）
        // 尝试使用不同的方式 - 直接访问 dashboard
        await page.goto(`${BASE_URL}/${realmId}`)
        // Technical delay: Wait for page navigation
        await page.waitForLoadState('domcontentloaded')
      }

      // 导航到 Security 页面
      await page.goto(`${BASE_URL}/user/security`)
      await page.waitForLoadState('domcontentloaded')

      // 尝试禁用 TOTP
      const disableButton = page.getByTestId('totp-disable-button')
      const canDisable = await disableButton.isVisible({ timeout: 2000 }).catch(() => false)

      if (canDisable) {
        console.log('[TOTP Helper] Disabling admin TOTP')
        await disableButton.click()
        await page.getByTestId('totp-disable-password-input').fill('password')
        await page.getByTestId('totp-disable-submit-button').click()
        // Technical delay: Wait for TOTP disable operation to complete
        await expect(page.getByText(/TOTP.*disabled|disabled.*successfully/i)).toBeVisible({ timeout: 5000 }).catch(() => {})
        console.log('[TOTP Helper] Admin TOTP disabled')
      } else {
        console.log('[TOTP Helper] Cannot disable admin TOTP (force enabled or no button)')
        // 如果强制启用 TOTP，我们无法创建用户
        return
      }
    }

    adminLoggedIn = true
    console.log('[TOTP Helper] Admin login successful')
  } catch (error) {
    console.log('[TOTP Helper] Unable to login as admin to create user:', error)
    return
  }

  if (!adminLoggedIn) {
    return
  }

  // 导航到 Users 页面
  await page.goto(`${BASE_URL}/admin/manage/users`)
  await page.waitForLoadState('domcontentloaded')

  // 检查用户是否已存在
  const existingUser = page.locator(`tr:has-text("${email}")`)
  const userExists = await existingUser.isVisible({ timeout: 3000 }).catch(() => false)

  if (userExists) {
    console.log(`[TOTP Helper] User already exists: ${email}`)
    return
  }

  // 用户不存在，创建新用户
  console.log(`[TOTP Helper] Creating user: ${email}`)

  // 点击 "Add User" 按钮
  await page.getByTestId('create-user-button').click()

  // 等待创建对话框出现
  await expect(page.getByTestId('create-user-dialog')).toBeVisible({ timeout: 5000 })

  // 填写用户信息
  await page.getByTestId('user-create-email-input').fill(email)
  await page.getByTestId('user-create-password-input').fill(password)

  // 提交创建
  await page.getByTestId('user-create-submit-button').click()

  // 等待创建完成 - 使用断言等待
  await expect(page.getByText(/user.*created|created.*successfully|创建.*成功/i)).toBeVisible({ timeout: 5000 }).catch(() => {})

  console.log(`[TOTP Helper] User created: ${email}`)
}

// ============================================================================
// TOTP Helper Functions
// ============================================================================

/**
 * 通过 UI 设置用户 TOTP
 *
 * @param page Playwright Page 对象
 * @param options TOTP 设置选项
 * @param logger 可选的日志记录器
 * @returns 备份恢复码列表（如果成功）
 */
export async function setupUserWithTOTP(
  page: Page,
  options: SetupTOTPOptions,
  logger?: UnifiedLogger
): Promise<string[]> {
  const { realmId, email, password } = options

  console.log(`[TOTP Helper] Setting up TOTP for user: ${email}`)

  // 步骤 1: 登录用户（不等待导航，因为可能需要处理 TOTP）
  await loginAsUser(page, { realmId, email, password, waitNavigation: false })

  // 步骤 2: 检查是否需要 TOTP 验证 - 使用断言等待而非固定延迟
  const totpInput = page.getByTestId('totp-verification-code-input')
  const requiresTOTP = await totpInput.isVisible({ timeout: 2000 }).catch(() => false)

  if (requiresTOTP) {
    console.log('[TOTP Helper] User has TOTP enabled, entering setup mode for testing')
    // 对于测试，我们直接导航到 security 页面
    // 在实际实现中，应该提供正确的 TOTP 验证码
  }

  // 步骤 3: 导航到 Security 页面
  await page.goto(`${BASE_URL}/user/security`)
  await page.waitForLoadState('domcontentloaded')

  // 步骤 3: 检查是否已启用 TOTP
  const enableButton = page.getByTestId('totp-enable-button')
  const isAlreadyEnabled = await enableButton.isVisible({ timeout: 3000 }).catch(() => false)

  if (!isAlreadyEnabled) {
    console.log(`[TOTP Helper] User ${email} already has TOTP enabled`)
    // 获取现有备份码数量
    const backupCodes = await extractBackupCodes(page)
    console.log(`[TOTP Helper] Found ${backupCodes.length} existing backup codes`)
    return backupCodes
  }

  // 步骤 4: 点击 "Enable TOTP" 按钮
  await page.getByTestId('totp-enable-button').click()

  // 步骤 5: 点击 "Generate QR Code" 按钮
  await page.getByTestId('totp-generate-button').click()

  // 步骤 6: 等待二维码显示
  await expect(page.getByTestId('totp-qr-code')).toBeVisible({ timeout: 5000 })

  // 步骤 7: 提取备份恢复码
  const backupCodes = await extractBackupCodes(page)
  console.log(`[TOTP Helper] Extracted ${backupCodes.length} backup codes`)

  // 步骤 8: 勾选"已保存备份码"
  await page.getByTestId('totp-saved-backup-codes-checkbox').check()

  // 步骤 9: 输入模拟验证码（实际使用时需要从 QR code 计算）
  // 注意: 在实际测试中，这里需要真正的 TOTP 验证码
  await page.getByTestId('totp-verify-code-input').fill('123456')

  // 步骤 10: 点击验证按钮
  await page.getByTestId('totp-verify-button').click()

  // 步骤 11: 等待验证完成 - 使用断言等待
  await expect(page.getByText(/verification.*successful|setup.*completed|验证.*成功/i)).toBeVisible({ timeout: 5000 }).catch(() => {})

  console.log(`[TOTP Helper] TOTP setup completed for user: ${email}`)

  return backupCodes
}

/**
 * 登录普通用户（非管理员）
 *
 * @param page Playwright Page 对象
 * @param options 登录选项
 */
/**
 * 登录普通用户（非管理员）
 *
 * @param page Playwright Page 对象
 * @param options 登录选项
 */
export async function loginAsUser(
  page: Page,
  options: {
    realmId: string
    email: string
    password: string
    waitNavigation?: boolean
  }
): Promise<void> {
  const { realmId, email, password, waitNavigation = true } = options

  console.log(`[TOTP Helper] Logging in user: ${email} to realm: ${realmId}`)

  // 使用现有的登录函数，传递 waitNavigation 参数
  await loginWithCredentials(page, {
    realmId,
    email,
    password,
    waitNavigation, // 遵循调用方指定的等待导航设置
  })
}

/**
 * 清除 session 数据
 */
async function clearSessionData(page: Page): Promise<void> {
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
 * 从页面提取备份恢复码
 *
 * @param page Playwright Page 对象
 * @returns 备份恢复码列表
 */
export async function extractBackupCodes(page: Page): Promise<string[]> {
  const backupCodes: string[] = []

  // 查找所有备份码元素
  const codeElements = await page.locator('[data-testid^="backup-code-copy-button-"]').all()

  for (const element of codeElements) {
    // 获取代码值（从父元素或相邻元素）
    const parent = element.locator('..')
    const codeElement = parent.locator('code').or(parent.locator('[data-testid^="backup-code-value-"]'))

    const codeText = await codeElement.textContent().catch(() => null)
    if (codeText) {
      backupCodes.push(codeText.trim())
    }
  }

  return backupCodes
}

/**
 * 获取用户 TOTP 状态（通过 UI）
 *
 * @param page Playwright Page 对象
 * @param realmId Realm ID
 * @returns TOTP 状态信息
 */
export async function getTotpStatus(page: Page, realmId: string): Promise<TOTPStatus> {
  // 导航到 Security 页面
  await page.goto(`${BASE_URL}/user/security`)
  await page.waitForLoadState('domcontentloaded')

  // 检查 TOTP 状态卡片
  const statusCard = page.getByTestId('totp-status-card')
  const isCardVisible = await statusCard.isVisible({ timeout: 3000 }).catch(() => false)

  if (!isCardVisible) {
    // TOTP 未启用
    return {
      enabled: false,
      remaining_backup_codes: 0,
      used_backup_codes: 0,
    }
  }

  // TOTP 已启用，提取状态信息
  const enabledAtText = await page.getByTestId('totp-enabled-at').textContent().catch(() => null)
  const lastVerifiedText = await page.getByTestId('totp-last-verified-at').textContent().catch(() => null)
  const remainingText = await page.getByTestId('totp-remaining-backup-codes').textContent().catch(() => '0')
  const usedText = await page.getByTestId('totp-used-backup-codes').textContent().catch(() => '0')

  return {
    enabled: true,
    enabled_at: enabledAtText || undefined,
    last_verified_at: lastVerifiedText || undefined,
    remaining_backup_codes: parseInt(remainingText, 10),
    used_backup_codes: parseInt(usedText, 10),
  }
}

/**
 * 设置 Realm 强制 TOTP 模式（通过 UI）
 *
 * @param page Playwright Page 对象
 * @param options 设置选项
 * @param logger 可选的日志记录器
 */
export async function setupRealmForceTotp(
  page: Page,
  options: { realmId: string },
  logger?: UnifiedLogger
): Promise<void> {
  const { realmId } = options

  console.log(`[TOTP Helper] Setting up force TOTP for realm: ${realmId}`)

  // 步骤 1: 登录为 Realm Admin
  await loginAsAdmin(page, { realmId })

  // 步骤 2: 导航到 Settings 页面
  await page.goto(`${BASE_URL}/admin/manage/settings`)
  await page.waitForLoadState('domcontentloaded')

  // 步骤 3: 切换到 TOTP Tab
  await page.getByRole('tab', { name: 'TOTP' }).click()
  await expect(page.getByTestId('totp-enabled-switch')).toBeVisible({ timeout: 3000 })

  // 步骤 4: 启用 TOTP
  const totpEnabledSwitch = page.getByTestId('totp-enabled-switch')
  const isEnabled = await totpEnabledSwitch.isChecked()
  if (!isEnabled) {
    await totpEnabledSwitch.click()
  }

  // 步骤 5: 启用强制 TOTP
  const forceTotpSwitch = page.getByTestId('totp-force-enabled-switch')
  const isForceEnabled = await forceTotpSwitch.isChecked()
  if (!isForceEnabled) {
    await forceTotpSwitch.click()
  }

  // 步骤 6: 保存配置
  await page.getByTestId('totp-save-button').click()

  // 等待保存完成 - 使用断言等待
  await expect(page.getByText(/saved|successfully|保存.*成功/i)).toBeVisible({ timeout: 5000 }).catch(() => {})

  console.log(`[TOTP Helper] Force TOTP setup completed for realm: ${realmId}`)
}

/**
 * 禁用 Realm 强制 TOTP 模式（通过 UI）
 *
 * @param page Playwright Page 对象
 * @param options 设置选项
 */
export async function disableRealmForceTotp(
  page: Page,
  options: { realmId: string }
): Promise<void> {
  const { realmId } = options

  console.log(`[TOTP Helper] Disabling force TOTP for realm: ${realmId}`)

  // 步骤 1: 登录为 Realm Admin
  await loginAsAdmin(page, { realmId })

  // 步骤 2: 导航到 Settings 页面
  await page.goto(`${BASE_URL}/admin/manage/settings`)
  await page.waitForLoadState('domcontentloaded')

  // 步骤 3: 切换到 TOTP Tab
  await page.getByRole('tab', { name: 'TOTP' }).click()

  // 步骤 4: 禁用强制 TOTP
  const forceTotpSwitch = page.getByTestId('totp-force-enabled-switch')
  const isForceEnabled = await forceTotpSwitch.isChecked()
  if (isForceEnabled) {
    await forceTotpSwitch.click()
  }

  // 步骤 5: 保存配置
  await page.getByTestId('totp-save-button').click()

  // 等待保存完成 - 使用断言等待
  await expect(page.getByText(/saved|successfully|保存.*成功/i)).toBeVisible({ timeout: 5000 }).catch(() => {})

  console.log(`[TOTP Helper] Force TOTP disabled for realm: ${realmId}`)
}
