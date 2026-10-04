/**
 * Realm Admin 综合演示测试 - Email-OTP 配置
 *
 * 用户故事: US-EO-003 - Realm 管理员启用/禁用 Email-OTP 登录并独立控制 autoRegister
 *
 * 测试覆盖（镜像 realm-admin-totp-config-demo.e2e.ts 的 TOTP 配置范式，
 * 将 TOTP Tab 映射为 Email-OTP 控件）：
 * - Phase 0: 配置邮箱通道（Email-OTP 开关在邮箱未配置时为 disabled，
 *            故需先配置 Resend，使 `emailStatus.configured` 为 true，
 *            参考 realm-admin-email-config-demo.e2e.ts 的配置范式）。
 * - Phase 1: 验证初始配置状态（两个开关可见，可读取当前状态）
 * - Phase 2: 启用 Email-OTP + 开启 autoRegister，断言两者均为 true
 * - Phase 3: 保持 OTP 开启，单独关闭 autoRegister，断言 enabled=true 且
 *            auto_register=false（证明两个开关相互独立 — US-EO-003 场景 2）
 * - Phase 4: 关闭 Email-OTP（平滑降级配置侧），断言两者均为 false
 *            （US-EO-003 场景 3 配置侧）
 * - Phase 5: 刷新页面验证配置持久化
 *
 * 前端结构说明（提交 364767b2 "merged email-otp settings"）：
 * Email-OTP 不再是独立 tab，而是作为 `email-otp-section` 子区块并入 `email`
 * tab 内的 EmailConfigForm。SettingsPage.switchToEmailOtpTab() 因此先切到
 * `email-tab` 再等待 `email-otp-section` 可见。
 *
 * 所有交互均通过 SettingsPage 的 Email-OTP 方法驱动，
 * 不在本文件内内联任何 data-testid 字符串。
 *
 * NOT-COVERED（显式声明）: US-EO-003 场景 4（跨 Realm 403 守卫）在本 Demo
 * 中不覆盖。原因：需要一个稳定可用的第二 Realm + 管理员，且需验证
 * `GET /api/realms/{otherRealm}/config/email-otp` 在跨 Realm 访问时稳定返回
 * 403；这些运行时前提无法在编译期确认（本项为 compile-only，不执行测试）。
 * 该场景由后端测试覆盖，验收时亦不重复执行。
 *
 * @see docs/user-stories/auth/email-otp-login.md
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import { verifyTestEnvironment } from '../helpers/environment-setup'
import { loginAsAdmin } from '../helpers/auth'
import { SettingsPage } from '../pages/settings-page'

test.describe('[Realm Admin] Email-OTP 配置综合演示测试', () => {
  let testStartTime: number
  let settingsPage: SettingsPage | undefined
  const realmId = 'realm-001'

  test.afterEach(async ({ page }) => {
    // Best-effort: 关闭 Email-OTP，保证该 Realm 在本测试结束后处于
    // OTP-off 状态，避免影响其它 Demo（例如 email-otp 登录流程）。
    // resetEmailOtpConfig 内部已 try/catch，不会硬失败本测试。
    if (settingsPage) {
      await settingsPage.resetEmailOtpConfig()
    }

    // ⚠️ MANDATORY: 清理测试数据
    await cleanupTestData(page, realmId, {
      keepUsers: ['admin@realm-001.com'],
      timestamp: testStartTime,
    })
  })

  // ============================================================================
  // 用户故事 US-EO-003：Realm 管理员配置邮箱验证码登录与自动注册
  // ============================================================================

  test('US-EO-003: admin enables/disables Email-OTP and toggles autoRegister', async ({ page, demoLogger }) => {
    testStartTime = Date.now()
    settingsPage = new SettingsPage(page, demoLogger, realmId)

    // ⚠️ MANDATORY: 验证环境状态
    await verifyTestEnvironment(page, {
      requiredRealms: [realmId],
      requiredUsers: ['admin@realm-001.com'],
      skipRealmVerification: true, // Optimized: skip deep realm checks
      skipDatabaseCheck: false,    // Keep health check
      skipRedisCheck: false,       // Keep health check
    })

    // 登录为 Realm 管理员
    await loginAsAdmin(page, { realmId })

    // 导航到 Settings 页面（登录后首次进入）
    await settingsPage.goto()
    await settingsPage.waitForReady()

    // ========================================================================
    // Phase 0: 配置邮箱通道（OTP 开关的前置条件）
    //
    // Email-OTP 的两个开关在 `emailStatus.configured === false` 时处于
    // disabled 状态（前端 email-config-form.tsx 的 emailOtpDisabled）。
    // realm-001 种子数据未配置邮箱，因此必须先配置邮箱通道（这里用 Resend
    // 最小字段集：provider + from_address + resend_api_key），使 configured
    // 变为 true，OTP 开关才会被启用。范式参考 realm-admin-email-config-demo。
    // ========================================================================

    await test.step('Phase 0: 配置邮箱通道以启用 OTP 开关', async () => {
      await test.step('切到 Email tab', async () => {
        await settingsPage.switchToEmailTab()
      })

      await test.step('配置 Resend 邮箱提供商并保存', async () => {
        await settingsPage.configureResend({
          provider: 'resend',
          fromAddress: 'otp-demo@example.com',
          resendApiKey: 're_otp_demo_test_key',
        })
        await settingsPage.saveEmailConfig()
      })

      await test.step('断言邮箱已 configured（OTP 开关前置满足）', async () => {
        await expect.poll(async () => settingsPage.isEmailConfigured(), {
          timeout: 15000,
        }).toBe(true)
      })
    })

    // ========================================================================
    // Phase 1: 验证初始配置状态
    // ========================================================================

    await test.step('Phase 1: 验证初始配置状态', async () => {
      await test.step('导航到 Settings -> Email-OTP 页面', async () => {
        await settingsPage.switchToEmailOtpTab()
      })

      await test.step('验证 Email-OTP 配置开关可见', async () => {
        // 通过 SettingsPage 暴露的 Locator 断言可见（不内联 testid 字符串）
        await expect(settingsPage.emailOtpEnabledSwitch).toBeVisible()
        await expect(settingsPage.emailOtpAutoRegisterSwitch).toBeVisible()
      })

      await test.step('获取当前 Email-OTP 配置状态', async () => {
        const config = await settingsPage.getEmailOtpConfig()
        demoLogger.testCode.log('Current Email-OTP Config:', config)

        // 验证配置对象包含必要字段
        expect(config).toHaveProperty('enabled')
        expect(config).toHaveProperty('auto_register')

        demoLogger.testCode.log('Email-OTP Configuration status retrieved successfully')
      })
    })

    // ========================================================================
    // Phase 2: 启用 Email-OTP + 开启 autoRegister
    // ========================================================================

    await test.step('Phase 2: 启用 Email-OTP 并开启 autoRegister', async () => {
      await test.step('启用 Email-OTP 与 autoRegister 并保存', async () => {
        await settingsPage.enableEmailOtp()
        await settingsPage.enableAutoRegister()
        await settingsPage.saveEmailOtpConfig()
      })

      await test.step('断言两者均为 true', async () => {
        const config = await settingsPage.getEmailOtpConfig()
        expect(config.enabled).toBe(true)
        expect(config.auto_register).toBe(true)
        demoLogger.testCode.log('Email-OTP enabled + autoRegister on')
      })
    })

    // ========================================================================
    // Phase 3: 保持 OTP 开启，单独关闭 autoRegister（证明两开关独立 — US-EO-003 场景 2）
    // ========================================================================

    await test.step('Phase 3: 保持 OTP 开启，单独关闭 autoRegister（两开关独立性）', async () => {
      await test.step('关闭 autoRegister 并保存（Email-OTP 保持开启）', async () => {
        await settingsPage.disableAutoRegister()
        await settingsPage.saveEmailOtpConfig()
      })

      await test.step('断言 enabled=true 且 auto_register=false', async () => {
        const config = await settingsPage.getEmailOtpConfig()
        expect(config.enabled).toBe(true)
        expect(config.auto_register).toBe(false)
        demoLogger.testCode.log('Email-OTP still enabled, autoRegister independently disabled')
      })
    })

    // ========================================================================
    // Phase 4: 关闭 Email-OTP（平滑降级配置侧 — US-EO-003 场景 3 配置侧）
    // ========================================================================

    await test.step('Phase 4: 关闭 Email-OTP（平滑降级配置侧）', async () => {
      await test.step('禁用 Email-OTP 并保存', async () => {
        await settingsPage.disableEmailOtp()
        await settingsPage.saveEmailOtpConfig()
      })

      await test.step('断言两者均为 false', async () => {
        const config = await settingsPage.getEmailOtpConfig()
        expect(config.enabled).toBe(false)
        expect(config.auto_register).toBe(false)
        demoLogger.testCode.log('Email-OTP disabled (graceful degradation)')
      })
    })

    // ========================================================================
    // Phase 5: 验证配置持久化（刷新页面后仍为 false/false）
    // ========================================================================

    await test.step('Phase 5: 验证配置持久化', async () => {
      await test.step('刷新页面并重新进入 Email-OTP Tab', async () => {
        await page.reload()
        await settingsPage.waitForReady()
        await settingsPage.switchToEmailOtpTab()
      })

      await test.step('断言刷新后配置仍为 false/false', async () => {
        const config = await settingsPage.getEmailOtpConfig()
        expect(config.enabled).toBe(false)
        expect(config.auto_register).toBe(false)
        demoLogger.testCode.log('Configuration persisted after page reload')
      })
    })

    // NOT-COVERED: US-EO-003 场景 4（跨 Realm 403 守卫）— 见文件头声明，由后端测试覆盖。
  })
})
