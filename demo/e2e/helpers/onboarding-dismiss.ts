/**
 * Onboarding welcome-dialog dismissal (leaf helper).
 *
 * Lives in its own module so both the login helpers (helpers/auth.ts) and the
 * login page object (pages/login-page.ts) can share one implementation without
 * a circular import (auth.ts already imports LoginPage).
 */

import { expect, type Page } from '@playwright/test'
import { SELECTORS } from '../selectors'

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
  // Fast path: the orchestrator's trigger is the completion marker alone, so
  // when the current session's realm+user (persisted auth-storage state)
  // already carries 'completed' the welcome dialog can never mount. Skip
  // straight out instead of burning the full visibility timeout — a
  // page-object login into a context that already saw the onboarding (marker
  // survives UI logout) would otherwise pay a fixed 5s on every call. Any
  // parse failure falls through to the wait below unchanged.
  const alreadyCompleted = await page
    .evaluate(() => {
      try {
        const raw = window.localStorage.getItem('auth-storage')
        if (!raw) return false
        const state = JSON.parse(raw)?.state
        if (!state?.realmId || !state?.user?.id) return false
        return (
          window.localStorage.getItem(`herald.onboarding.${state.realmId}.${state.user.id}`) ===
          'completed'
        )
      } catch {
        return false
      }
    })
    .catch(() => false)
  if (alreadyCompleted) return

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
