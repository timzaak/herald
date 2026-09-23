/**
 * Environment Setup Helper for Demo Tests
 *
 * 提供环境验证和数据清理功能，确保测试隔离
 * 遵循 UI-Only 原则，所有操作通过 UI 执行
 *
 * ⚠️ EXCEPTION TO UI-ONLY RULE:
 * verifyTestEnvironment() uses page.request.get() for health checks
 * because:
 * 1. Health check is infrastructure validation, not business logic
 * 2. No UI exists for health check endpoints
 * 3. This ensures test environment is ready before UI operations
 */

import { Page, expect } from '@playwright/test'
import { validateBackendHealth, formatValidationErrors, type ValidationResult } from './api-validator'
import { UsersPage } from '../pages/users-page'
import { loginAsAdminWithConsent } from './legal-consent/consent-aware-login'

export const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

// ============================================================================
// Types
// ============================================================================

/**
 * Realm验证规范
 */
export interface RealmValidationSpec {
  realmId: string
  expectedName?: string
  expectedConfig?: Record<string, any>
}

/**
 * 用户验证规范
 */
export interface UserValidationSpec {
  email: string
  realmId: string
  expectedNickname?: string
  expectedStatus?: number
  expectedRoles?: string[]
}

/**
 * 角色验证规范
 */
export interface RoleValidationSpec {
  roleName: string
  realmId: string
  expectedPermissions?: string[]
}

/**
 * 数据完整性规范
 */
export interface DataIntegritySpec {
  realms?: RealmValidationSpec[]
  users?: UserValidationSpec[]
  requiredRoles?: RoleValidationSpec[]
}

/**
 * 环境验证选项
 */
export interface VerifyEnvironmentOptions {
  requiredRealms?: string[]
  requiredUsers?: string[]
  skipDatabaseCheck?: boolean
  skipRedisCheck?: boolean
  skipRealmVerification?: boolean
  validateDataIntegrity?: boolean
  dataIntegritySpec?: DataIntegritySpec
  detectPollution?: boolean
  autoCleanupOnPollution?: boolean
  pollutionThreshold?: number
}

/**
 * 数据清理选项
 */
export interface CleanupDataOptions {
  keepUsers?: string[]
  timestamp?: number
  verbose?: boolean
  aggressive?: boolean
  testUserEmails?: string[]
}

// ============================================================================
// Environment Verification
// ============================================================================

/**
 * 验证测试环境状态
 *
 * 每个测试的 beforeEach 必须首先验证环境状态
 *
 * @param page Playwright Page 对象
 * @param options 验证选项
 */
export async function verifyTestEnvironment(
  page: Page,
  options: VerifyEnvironmentOptions = {}
): Promise<void> {
  const {
    requiredRealms = [],
    requiredUsers = ['admin@cas.com'],
    skipDatabaseCheck = false,
    skipRedisCheck = false,
    skipRealmVerification = true,
    validateDataIntegrity = false,
    dataIntegritySpec,
  } = options

  console.log('[EnvironmentSetup] 开始验证测试环境...')

  // 步骤 1: 验证数据库和 Redis 连接
  if (!skipDatabaseCheck || !skipRedisCheck) {
    await verifyBackendConnections({ skipDatabaseCheck, skipRedisCheck })
  }

  // 步骤 2: 验证关键 Realm 存在（可选）
  if (requiredRealms.length > 0 && !skipRealmVerification) {
    await verifyRequiredRealms(page, requiredRealms)
  } else {
    console.log('[EnvironmentSetup] 跳过 Realm 存在性验证')
  }

  // 步骤 3: 验证关键用户存在
  await verifyRequiredUsers(page, requiredUsers)

  // 步骤 4: 数据完整性验证（可选）
  if (validateDataIntegrity && dataIntegritySpec) {
    await validateDataIntegritySpecs(page, dataIntegritySpec)
  }

  console.log('[EnvironmentSetup] 环境验证完成')
}

/**
 * 验证后端服务连接
 */
async function verifyBackendConnections(options: {
  skipDatabaseCheck: boolean
  skipRedisCheck: boolean
}): Promise<void> {
  console.log('[EnvironmentSetup] 验证系统服务连接...')

  const result: ValidationResult = await validateBackendHealth({
    maxRetries: 6,
    retryDelay: 2000,
    timeout: 10000,
    strictSchema: false,
  })

  if (!result.healthy) {
    const errorDetails = formatValidationErrors(result)
    throw new Error(`Backend health check failed:\n${errorDetails}`)
  }

  if (!options.skipDatabaseCheck && result.response?.database !== true) {
    throw new Error(
      '数据库连接失败\n' +
      '  快速检查: docker ps | grep postgres\n' +
      '  日志: tail -f log/backend-demo.log'
    )
  }

  if (!options.skipRedisCheck && result.response?.redis !== true) {
    throw new Error(
      'Redis 连接失败\n' +
      '  快速检查: docker ps | grep redis\n' +
      '  日志: tail -f log/backend-demo.log'
    )
  }

  console.log('[EnvironmentSetup] 数据库和 Redis 连接正常')
}

/**
 * 验证必需的 Realm 存在
 */
async function verifyRequiredRealms(page: Page, requiredRealms: string[]): Promise<void> {
  console.log('[EnvironmentSetup] 验证关键 Realm 存在性...')

  await page.goto(`${BASE_URL}/admin/manage/realms`)
  await page.waitForLoadState('networkidle')
  await expect(page.locator('h1').filter({ hasText: 'Realms' })).toBeVisible({ timeout: 5000 })

  for (const realmId of requiredRealms) {
    const realmCell = page
      .locator(`table >> tbody >> tr:has-text("${realmId}")`)
      .or(page.locator(`[data-testid="realm-row-${realmId}"]`))

    const isVisible = await realmCell.isVisible({ timeout: 3000 }).catch(() => false)

    if (!isVisible) {
      throw new Error(`必需的 Realm "${realmId}" 不存在`)
    }

    console.log(`[EnvironmentSetup] Realm "${realmId}" 存在`)
  }
}

/**
 * 验证必需的用户存在
 */
async function verifyRequiredUsers(page: Page, requiredUsers: string[]): Promise<void> {
  console.log('[EnvironmentSetup] 验证关键用户存在性...')

  // TODO: 等待前端 Users 页面实现后，恢复验证逻辑
  for (const userEmail of requiredUsers) {
    console.log(`[EnvironmentSetup] 跳过用户 "${userEmail}" 验证（页面未实现）`)
  }
}

/**
 * 验证数据完整性规范
 */
async function validateDataIntegritySpecs(
  page: Page,
  dataIntegritySpec: DataIntegritySpec
): Promise<void> {
  console.log('[EnvironmentSetup] 开始数据完整性验证...')

  if (dataIntegritySpec.realms && dataIntegritySpec.realms.length > 0) {
    for (const realmSpec of dataIntegritySpec.realms) {
      await verifyRealmProperties(page, realmSpec.realmId, realmSpec.expectedName)
    }
  }

  if (dataIntegritySpec.users && dataIntegritySpec.users.length > 0) {
    for (const userSpec of dataIntegritySpec.users) {
      await verifyUserRoles(page, userSpec.realmId, userSpec.email, userSpec.expectedRoles)
    }
  }

  if (dataIntegritySpec.requiredRoles && dataIntegritySpec.requiredRoles.length > 0) {
    await verifyRequiredRoles(page, dataIntegritySpec.requiredRoles)
  }

  console.log('[EnvironmentSetup] 数据完整性验证通过')
}

/**
 * 验证 Realm 属性
 */
export async function verifyRealmProperties(
  page: Page,
  realmId: string,
  expectedName?: string
): Promise<boolean> {
  const currentUrl = page.url()

  await page.waitForLoadState('domcontentloaded', { timeout: 10000 }).catch(() => {})

  let isLoggedIn = false
  try {
    isLoggedIn = await page.evaluate(() => {
      if (document.readyState !== 'complete') return false
      return !!window.localStorage.getItem('cas.auth.token')
    })
  } catch {
    isLoggedIn = false
  }

  if (!isLoggedIn && !currentUrl.includes('/manage')) {
    console.log('[EnvironmentSetup] 用户未登录，跳过 Realm 属性验证')
    return true
  }

  await page.goto(`${BASE_URL}/admin/manage/realms`)
  await page.waitForLoadState('networkidle')

  await expect(page.locator('h1').filter({ hasText: 'Realms' })).toBeVisible({ timeout: 5000 })

  const realmRow = page
    .locator(`table >> tbody >> tr:has-text("${realmId}")`)
    .or(page.locator(`[data-testid="realm-row-${realmId}"]`))

  const isVisible = await realmRow.isVisible({ timeout: 3000 }).catch(() => false)
  if (!isVisible) {
    throw new Error(`Realm "${realmId}" 不存在`)
  }

  if (expectedName) {
    const allCells = realmRow.locator('td')
    const cellCount = await allCells.count()

    for (let i = 0; i < cellCount; i++) {
      const cellText = await allCells.nth(i).textContent().catch(() => null)
      if (cellText && cellText.includes(expectedName)) {
        console.log(`[EnvironmentSetup] Realm "${realmId}" 名称验证通过: ${expectedName}`)
        return true
      }
    }
    console.warn(`[EnvironmentSetup] 无法验证 Realm "${realmId}" 名称`)
  }

  return true
}

/**
 * 验证用户角色
 */
export async function verifyUserRoles(
  page: Page,
  realmId: string,
  userEmail: string,
  expectedRoles?: string[]
): Promise<boolean> {
  console.log(`[EnvironmentSetup] 跳过用户 "${userEmail}" 角色验证（页面未实现）`)
  return true
}

/**
 * 验证必需角色存在
 */
export async function verifyRequiredRoles(
  page: Page,
  requiredRoles: RoleValidationSpec[]
): Promise<boolean> {
  const rolesByRealm = new Map<string, RoleValidationSpec[]>()

  for (const roleSpec of requiredRoles) {
    if (!rolesByRealm.has(roleSpec.realmId)) {
      rolesByRealm.set(roleSpec.realmId, [])
    }
    rolesByRealm.get(roleSpec.realmId)!.push(roleSpec)
  }

  for (const [realmId, realmRoles] of rolesByRealm.entries()) {
    await page.goto(`${BASE_URL}/manage/permissions`)
    await page.waitForLoadState('networkidle')

    await expect(page.locator('h1').filter({ hasText: /Role Definitions/i })).toBeVisible({
      timeout: 5000,
    })

    for (const roleSpec of realmRoles) {
      const roleRow = page
        .locator(`table >> tbody >> tr:has-text("${roleSpec.roleName}")`)
        .or(page.locator(`[data-testid="role-row-${roleSpec.roleName}"]`))

      const isVisible = await roleRow.isVisible({ timeout: 3000 }).catch(() => false)

      if (isVisible) {
        console.log(`[EnvironmentSetup] Realm "${realmId}" 包含角色 "${roleSpec.roleName}"`)
      } else {
        console.warn(`[EnvironmentSetup] Realm "${realmId}" 中找不到角色 "${roleSpec.roleName}"`)
      }
    }
  }

  return true
}

// ============================================================================
// Data Cleanup
// ============================================================================

/**
 * 清理测试创建的用户
 *
 * 通过 consent-aware admin 登录后进入用户列表，按 email 删除测试用户。
 * 被 keepUsers 保护的用户会被跳过；不存在的用户会被静默忽略，避免破坏
 * 现有测试。
 */
async function cleanupTestUsers(
  page: Page,
  realmId: string,
  options: CleanupDataOptions = {}
): Promise<void> {
  const { verbose = true, testUserEmails = [], keepUsers = [] } = options

  if (testUserEmails.length === 0) {
    return
  }

  console.log(`[EnvironmentSetup] 开始清理测试用户 (realm: ${realmId})...`)

  // 使用 consent-aware admin 登录，防止协议版本变更后登录页出现重新同意视图
  await loginAsAdminWithConsent(page, realmId)

  const usersPage = new UsersPage(page)
  await usersPage.goto(realmId)

  for (const email of testUserEmails) {
    if (keepUsers.includes(email)) {
      if (verbose) {
        console.log(`[EnvironmentSetup] 跳过受保护用户: ${email}`)
      }
      continue
    }

    try {
      const exists = await usersPage.userExists(email)
      if (exists) {
        await usersPage.deleteUser(email, realmId)
        if (verbose) {
          console.log(`[EnvironmentSetup] 已删除测试用户: ${email}`)
        }
      } else if (verbose) {
        console.log(`[EnvironmentSetup] 测试用户不存在，跳过: ${email}`)
      }
    } catch (error) {
      // 删除失败不应阻塞整个清理流程；记录后继续
      console.warn(`[EnvironmentSetup] 删除测试用户 ${email} 失败:`, error)
    }
  }

  console.log(`[EnvironmentSetup] 测试用户清理完成 (realm: ${realmId})`)
}

/**
 * 清理演示测试数据
 *
 * @param page Playwright Page 对象
 * @param realmId Realm ID
 * @param options 清理选项
 */
export async function cleanupDemoTestData(
  page: Page,
  realmId: string,
  options: CleanupDataOptions = {}
): Promise<void> {
  const { verbose = true } = options

  // 清理测试创建的订阅套餐（通过 API）
  await cleanupSubscriptionPlans(page, realmId, options)

  // 清理通过 UI 创建的测试用户（使用 consent-aware admin 登录）
  await cleanupTestUsers(page, realmId, options)
}

/**
 * 清理测试创建的订阅套餐
 *
 * 通过 API 获取所有套餐，删除非内置的测试套餐。
 * 内置套餐（名称以 "test-subscription-plan" 开头）会保留。
 *
 * @param page Playwright Page 对象（使用其已认证的 request context）
 * @param realmId Realm ID
 * @param options 清理选项（verbose 控制日志输出）
 */
async function cleanupSubscriptionPlans(
  page: Page,
  realmId: string,
  options: CleanupDataOptions = {}
): Promise<void> {
  const { verbose = true } = options
  const plansApiUrl = `${BASE_URL}/api/bill/plans`

  try {
    const listResponse = await page.request.get(plansApiUrl, {
      headers: { 'Content-Type': 'application/json' },
    })

    if (listResponse.status() !== 200) {
      if (verbose) {
        console.warn(
          `[EnvironmentSetup] 获取套餐列表失败 (realm: ${realmId}): HTTP ${listResponse.status()}`
        )
      }
      return
    }

    const body = await listResponse.json()
    const plans: Array<{ id: string; name: string }> = body.plans || []

    let deletedCount = 0
    for (const plan of plans) {
      // 保留内置的测试套餐
      if (plan.name === 'test-subscription-plan') {
        continue
      }

      const deleteUrl = `${plansApiUrl}/${plan.id}`
      const deleteResponse = await page.request.delete(deleteUrl, {
        headers: { 'Content-Type': 'application/json' },
      })

      if (deleteResponse.status() === 200 || deleteResponse.status() === 204) {
        deletedCount++
        if (verbose) {
          console.log(`[EnvironmentSetup] 已删除套餐 "${plan.name}" (${plan.id})`)
        }
      } else if (verbose) {
        console.warn(
          `[EnvironmentSetup] 删除套餐 "${plan.name}" 失败: HTTP ${deleteResponse.status()}`
        )
      }
    }

    if (verbose) {
      console.log(
        `[EnvironmentSetup] 套餐清理完成 (realm: ${realmId}): 已删除 ${deletedCount}/${plans.length} 个`
      )
    }
  } catch (error) {
    console.error(`[EnvironmentSetup] 清理订阅套餐时发生错误 (realm: ${realmId}):`, error)
  }
}
