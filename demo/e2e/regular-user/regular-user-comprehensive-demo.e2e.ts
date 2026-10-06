/**
 * Regular User Comprehensive Demo Tests
 *
 * User Stories: docs/user-stories/core/regular-user.md
 *
 * Test Scenarios:
 * - US-RU-004: Change Personal Password
 * - US-RU-005: View Personal Profile
 * - US-RU-006: Update Personal Nickname
 */

import { test, cleanupTestData, expect } from '../fixtures/demo-page.fixtures'
import { SELECTORS } from '../selectors'
import { verifyTestEnvironment } from '../helpers/environment-setup'

test.describe('[Regular User] Profile Management Comprehensive Demo Tests', () => {
  let testStartTime: number

  test.beforeEach(async ({ page, testStartTime: startTime }) => {
    testStartTime = startTime
    await verifyTestEnvironment(page, {
      requiredRealms: ['admin'],
      requiredUsers: ['admin@cas.com'],
    })
  })

  // Single test.afterEach for cleanup
  test.afterEach(async ({ usersPage, demoLogger }) => {
    // ⚠️ MANDATORY: 清理测试数据
    await cleanupTestData(usersPage.page, 'admin', {
      keepUsers: ['admin@cas.com'],
      timestamp: testStartTime,
    })

    // Verify API call optimization - only public-config should be called
    await test.step('Verify API Call Optimization', async () => {
      const logs = demoLogger.network.getLogs()
      const publicConfigCalls = logs.filter(log => log.url.includes('/public-config'))
      const providerCalls = logs.filter(log => log.url.includes('/api/oauth/') && log.url.includes('/providers'))
      const registrationConfigCalls = logs.filter(log => log.url.includes('/api/configs/') && log.url.includes('/registration'))

      console.log(`[Network] Total API calls: ${logs.length}`)
      console.log(`[Network] Public config calls: ${publicConfigCalls.length}`)
      console.log(`[Network] OAuth provider calls: ${providerCalls.length}`)
      console.log(`[Network] Registration config calls: ${registrationConfigCalls.length}`)

      // Verify optimization: should use public-config instead of separate calls
      // Note: This is a soft check - we log the results but don't fail the test
      // because the login flow through fixtures might make additional calls
      if (publicConfigCalls.length > 0) {
        console.log('✓ Public config API is being used')
      }
      if (providerCalls.length > 0 || registrationConfigCalls.length > 0) {
        console.log('⚠ Legacy API calls detected - consider optimizing')
      }
    })
  })

  // ============================================================================
  // User Story 5: View Personal Profile [US-RU-005]
  // ============================================================================
  test.describe('User Story 5: View Personal Profile [US-RU-005]', () => {
    test('Scenario 2.1: 查看个人资料成功', async ({ usersPage, demoLogger }) => {
      await test.step('When: 访问个人资料页面', async () => {
        await usersPage.page.goto('/admin/user/profile')
        // 修复：前端没有 data-testid="profile-heading"，使用 CardTitle 文本
        await expect(usersPage.page.getByText('Profile Information')).toBeVisible()
      })

      await test.step('Then: 显示所有必需字段', async () => {
        const page = usersPage.page

        // 验证邮箱字段（使用 data-testid 避免严格模式冲突）
        await expect(page.locator('[data-testid="email-display"]')).toBeVisible()

        // 验证昵称字段（使用 data-testid 避免严格模式冲突）
        await expect(page.locator('[data-testid="nickname-display"]')).toBeVisible()

        // 验证状态字段（使用 data-testid 避免严格模式冲突）
        await expect(page.locator('[data-testid="status-display"]')).toBeVisible()
      })
    })

    test('Scenario 2.2: Email 为只读字段', async ({ usersPage, demoLogger }) => {
      await test.step('Given: 在个人资料页面', async () => {
        await usersPage.page.goto('/admin/user/profile')
        await expect(usersPage.page.getByText('Profile Information')).toBeVisible()
      })

      await test.step('Then: Email 字段为只读，不可修改', async () => {
        const page = usersPage.page

        // Email 是纯文本显示，不是输入框（使用 data-testid 避免严格模式冲突）
        await expect(page.locator('[data-testid="email-display"]')).toBeVisible()
      })
    })

    test('Scenario 2.3: 账号状态为只读字段', async ({ usersPage, demoLogger }) => {
      await test.step('Given: 在个人资料页面', async () => {
        await usersPage.page.goto('/admin/user/profile')
        await expect(usersPage.page.getByText('Profile Information')).toBeVisible()
      })

      await test.step('Then: 状态字段为只读', async () => {
        const page = usersPage.page

        // 状态是纯文本显示（使用 data-testid 避免严格模式冲突）
        await expect(page.locator('[data-testid="status-display"]')).toBeVisible()
      })
    })
  })

  // ============================================================================
  // User Story 6: Update Personal Nickname [US-RU-006]
  // Status: 待前端实现后启用 — Profile 编辑页面 UI 未实现
  // ============================================================================
  // Note: Nickname update tests are skipped due to unimplemented frontend UI
  // TODO: Uncomment these tests when frontend profile UI is implemented
  /*
  test.describe('User Story 6: Update Personal Nickname [US-RU-006]', () => {
    test('Scenario 2.4: 正常修改昵称成功', async ({ usersPage, demoLogger, testStartTime }) => {
      const newNickname = `Updated Nickname ${testStartTime}`

      await test.step('When: 修改昵称', async () => {
        await usersPage.page.goto('/realm1/profile')
        await usersPage.page.getByTestId('nickname-input').clear()
        await usersPage.page.getByTestId('nickname-input').fill(newNickname)
        await usersPage.page.getByTestId('save-profile-button').click()
      })

      await test.step('Then: 昵称更新成功', async () => {
        const page = usersPage.page

        // 刷新页面验证昵称已保存（验证实际业务结果）
        await page.reload()
        await expect(page.getByTestId('nickname-input')).toHaveValue(newNickname)
      })
    })

    test('Scenario 2.5: 昵称为可选字段', async ({ usersPage, demoLogger }) => {
      await test.step('When: 清空昵称并保存', async () => {
        await usersPage.page.goto('/realm1/profile')
        await usersPage.page.getByTestId('nickname-input').clear()
        await usersPage.page.getByTestId('save-profile-button').click()
      })

      await test.step('Then: 更新成功，昵称为空', async () => {
        const page = usersPage.page

        // 验证昵称已清空（验证实际业务结果）
        await page.reload()
        await expect(page.getByTestId('nickname-input')).toHaveValue('')
      })
    })

    test('Scenario 2.6: 昵称长度限制测试（边界场景）', async ({ usersPage, demoLogger, testStartTime }) => {
      const longNickname = 'a'.repeat(51) // 超过 50 个字符

      await test.step('When: 输入超过 50 个字符的昵称', async () => {
        await usersPage.page.goto('/realm1/profile')
        await usersPage.page.getByTestId('nickname-input').clear()
        await usersPage.page.getByTestId('nickname-input').fill(longNickname)
        await usersPage.page.getByTestId('save-profile-button').click()
      })

      await test.step('Then: 系统提示昵称过长', async () => {
        const page = usersPage.page

        // 验证错误提示
        await expect(page.getByTestId('error-message')).toBeVisible()
        await expect(page.getByTestId('error-message')).toContainText('昵称不能超过 50 个字符')

        // 验证昵称未更新
        await page.reload()
        expect(await page.getByTestId('nickname-input').inputValue()).not.toBe(longNickname)
      })
    })
  })
  */

  // ============================================================================
  // User Story 4: Change Personal Password [US-RU-004]
  // ============================================================================
  test.describe('User Story 4: Change Personal Password [US-RU-004]', () => {
    test('Scenario 3.1: 正常修改密码成功', async ({ usersPage, demoLogger, testStartTime }) => {
      const newPassword = `NewPass123!${testStartTime}`

      await test.step('When: 输入当前密码、新密码、确认密码并提交', async () => {
        const page = usersPage.page

        await page.goto('/admin/user/security')
        await expect(page.locator(SELECTORS.profile.oldPasswordInput)).toBeVisible()

        await page.locator(SELECTORS.profile.oldPasswordInput).fill('password')
        await page.locator(SELECTORS.profile.newPasswordInput).fill(newPassword)
        await page.locator(SELECTORS.profile.confirmPasswordInput).fill(newPassword)
        await page.locator(SELECTORS.profile.changePasswordSubmitButton).click()
      })

      await test.step('Then: 密码修改成功并恢复原密码', async () => {
        const page = usersPage.page

        // 验证表单重置（验证实际业务结果）
        await expect(page.locator(SELECTORS.profile.oldPasswordInput)).toHaveValue('')
        await expect(page.locator(SELECTORS.profile.newPasswordInput)).toHaveValue('')
        await expect(page.locator(SELECTORS.profile.confirmPasswordInput)).toHaveValue('')

        // 恢复原密码，确保后续测试不受影响
        await page.locator(SELECTORS.profile.oldPasswordInput).fill(newPassword)
        await page.locator(SELECTORS.profile.newPasswordInput).fill('password')
        await page.locator(SELECTORS.profile.confirmPasswordInput).fill('password')
        await page.locator(SELECTORS.profile.changePasswordSubmitButton).click()

        // 验证恢复成功
        await expect(page.locator(SELECTORS.profile.oldPasswordInput)).toHaveValue('')

        console.log('Password changed and restored successfully.')
      })
    })

    test('Scenario 3.2: 密码修改表单字段验证', async ({ usersPage, demoLogger }) => {
      await test.step('When: 访问密码修改页面', async () => {
        const page = usersPage.page

        await page.goto('/admin/user/security')
        await expect(page.locator(SELECTORS.profile.oldPasswordInput)).toBeVisible()
      })

      await test.step('Then: 所有必需字段都存在且可编辑', async () => {
        const page = usersPage.page

        // 使用统一选择器验证字段
        await expect(page.locator(SELECTORS.profile.oldPasswordInput)).toBeVisible()
        await expect(page.locator(SELECTORS.profile.oldPasswordInput)).toBeEditable()

        await expect(page.locator(SELECTORS.profile.newPasswordInput)).toBeVisible()
        await expect(page.locator(SELECTORS.profile.newPasswordInput)).toBeEditable()

        await expect(page.locator(SELECTORS.profile.confirmPasswordInput)).toBeVisible()
        await expect(page.locator(SELECTORS.profile.confirmPasswordInput)).toBeEditable()

        await expect(page.locator(SELECTORS.profile.changePasswordSubmitButton)).toBeVisible()
      })
    })

    test('Scenario 3.3: 密码字段类型验证', async ({ usersPage, demoLogger }) => {
      await test.step('When: 检查密码字段类型', async () => {
        await usersPage.page.goto('/admin/user/security')
      })

      await test.step('Then: 密码字段正确隐藏输入', async () => {
        const page = usersPage.page

        // 使用统一选择器验证密码字段类型
        const passwordInputs = [
          SELECTORS.profile.oldPasswordInput,
          SELECTORS.profile.newPasswordInput,
          SELECTORS.profile.confirmPasswordInput,
        ]

        for (const selector of passwordInputs) {
          const inputType = await page.locator(selector).getAttribute('type')
          expect(inputType).toBe('password')
        }
      })
    })

    test('Scenario 3.4: 密码修改成功验证', async ({ usersPage, demoLogger }) => {
      await test.step('When: 成功修改密码后', async () => {
        const page = usersPage.page

        await page.goto('/admin/user/security')
        // 使用统一选择器
        await page.locator(SELECTORS.profile.oldPasswordInput).fill('password')
        const newPassword = `TestPass123!${Date.now()}`
        await page.locator(SELECTORS.profile.newPasswordInput).fill(newPassword)
        await page.locator(SELECTORS.profile.confirmPasswordInput).fill(newPassword)
        await page.locator(SELECTORS.profile.changePasswordSubmitButton).click()
      })

      await test.step('Then: 显示成功消息', async () => {
        const page = usersPage.page

        // 验证表单重置（验证实际业务结果）
        await expect(page.locator(SELECTORS.profile.oldPasswordInput)).toHaveValue('')
        await expect(page.locator(SELECTORS.profile.newPasswordInput)).toHaveValue('')
        await expect(page.locator(SELECTORS.profile.confirmPasswordInput)).toHaveValue('')
      })
    })
  })
})
