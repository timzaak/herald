/**
 * Error Context Enhancement for Demo Tests
 *
 * Provides comprehensive error context for better diagnosis
 * Captures element snapshots, page state, and recent API calls
 *
 * Usage in demo-diagnose:
 * ```typescript
 * import { enhanceError } from '../helpers/error-enhancer'
 * const context = await enhanceError(error, page, selector)
 * // console.log(context.elementSnapshot?.html)
 * // console.log(context.recentApiCalls)
 * ```
 */

import { Page, Locator } from '@playwright/test'
import { ApiRequestLog } from 'playwright-unified-logger'

/**
 * Element snapshot with detailed information
 */
export interface ElementSnapshot {
  html: string
  text: string
  attributes: Record<string, string>
  boundingBox?: { x: number; y: number; width: number; height: number }
  isVisible: boolean
  isDetached: boolean
  cssDisplay: string
  cssVisibility: string
  testId?: string
}

/**
 * Enhanced error context for diagnosis
 */
export interface EnhancedErrorContext {
  originalError: string
  timestamp: string
  currentPageUrl: string
  pageTitle?: string
  elementSnapshot?: ElementSnapshot
  visibleElements: Array<{
    selector: string
    text: string
    isVisible: boolean
    testId?: string
  }>
  recentApiCalls: Array<{
    url: string
    method: string
    status: number
    responseTime: number
    requestId?: string
  }>
  failedApiCalls: Array<{
    url: string
    method: string
    error: string
    responseTime?: number
  }>
  consoleErrors: Array<{
    message: string
    stack?: string
  }>
  cookies: Array<{
    name: string
    value: string
    domain: string
  }>
  screenshot?: string // base64
}

/**
 * Enhance error with comprehensive context
 *
 * @param error The original error
 * @param page Playwright Page object
 * @param selector Optional selector that caused the error
 * @returns Enhanced error context
 */
export async function enhanceError(
  error: Error,
  page: Page,
  selector?: string
): Promise<EnhancedErrorContext> {
  const context: EnhancedErrorContext = {
    originalError: error.message,
    timestamp: new Date().toISOString(),
    currentPageUrl: page.url(),
    pageTitle: await page.title().catch(() => undefined),
    visibleElements: [],
    recentApiCalls: [],
    failedApiCalls: [],
    consoleErrors: [],
    cookies: [],
  }

  try {
    // Capture element snapshot if selector provided
    if (selector) {
      try {
        const locator = page.locator(selector)
        context.elementSnapshot = await captureElementSnapshot(locator, page)
      } catch (snapshotError) {
        context.visibleElements.push({
          selector,
          text: `<snapshot failed: ${snapshotError.message}>`,
          isVisible: false,
        })
      }
    }

    // Capture visible elements with data-testid
    try {
      const elementsWithTestId = await page
        .locator('[data-testid]')
        .all()

      for (const element of elementsWithTestId.slice(0, 20)) { // Limit to 20 elements
        try {
          const testId = await element.getAttribute('data-testid')
          const text = await element.textContent()
          const isVisible = await element.isVisible().catch(() => false)

          context.visibleElements.push({
            selector: `[data-testid="${testId}"]`,
            text: text?.substring(0, 50) || '',
            isVisible,
            testId: testId || undefined,
          })
        } catch {
          // Skip elements that can't be accessed
        }
      }
    } catch {
      // Ignore errors when capturing visible elements
    }

    // Capture cookies (for auth diagnosis)
    try {
      const cookies = await page.context().cookies()
      context.cookies = cookies
        .filter(c => c.name.includes('auth') || c.name.includes('session') || c.name.includes('token'))
        .map(c => ({
          name: c.name,
          value: c.value ? `${c.value.substring(0, 20)}...` : '',
          domain: c.domain,
        }))
    } catch {
      // Ignore errors when capturing cookies
    }

    // Capture console errors (already logged by ConsoleLogger, but we can add context here)
    // Note: This is a placeholder - actual console errors are captured by ConsoleLogger
  } catch (contextError) {
    // If context capture fails, at least keep the basic error info
    // console.error('[ErrorEnhancer] Failed to capture full context:', contextError)
  }

  return context
}

/**
 * Capture element snapshot for detailed analysis
 *
 * @param locator The element locator
 * @param page Playwright Page object
 * @returns Element snapshot
 */
async function captureElementSnapshot(
  locator: Locator,
  page: Page
): Promise<ElementSnapshot> {
  const snapshot: ElementSnapshot = {
    html: '',
    text: '',
    attributes: {},
    isVisible: false,
    isDetached: false,
    cssDisplay: '',
    cssVisibility: '',
  }

  try {
    // Check if element is attached
    const isAttached = await locator.count().then(count => count > 0)
    if (!isAttached) {
      snapshot.isDetached = true
      return snapshot
    }

    // Capture visibility
    snapshot.isVisible = await locator.isVisible().catch(() => false)

    // Capture bounding box
    const box = await locator.boundingBox().catch(() => null)
    if (box) {
      snapshot.boundingBox = box
    }

    // Capture CSS properties
    const display = await locator.evaluate(el => {
      return window.getComputedStyle(el).display
    }).catch(() => 'unknown')
    snapshot.cssDisplay = display

    const visibility = await locator.evaluate(el => {
      return window.getComputedStyle(el).visibility
    }).catch(() => 'unknown')
    snapshot.cssVisibility = visibility

    // Capture text content
    const text = await locator.textContent().catch(() => '')
    snapshot.text = text?.substring(0, 200) || ''

    // Capture HTML (only first 500 chars to avoid huge output)
    const html = await locator.evaluate(el => el.outerHTML).catch(() => '')
    snapshot.html = html?.substring(0, 500) || ''

    // Capture data-testid and other attributes
    const testId = await locator.getAttribute('data-testid').catch(() => null)
    if (testId) {
      snapshot.attributes['data-testid'] = testId
    }

    const role = await locator.getAttribute('role').catch(() => null)
    if (role) {
      snapshot.attributes['role'] = role
    }

    const ariaLabel = await locator.getAttribute('aria-label').catch(() => null)
    if (ariaLabel) {
      snapshot.attributes['aria-label'] = ariaLabel
    }
  } catch (error) {
    // Return partial snapshot even if some properties fail
    // console.error('[ErrorEnhancer] Failed to capture element snapshot:', error)
  }

  return snapshot
}

/**
 * Add network context to enhanced error
 *
 * @param context Existing enhanced error context
 * @param networkLogs Network logs from UnifiedLogger
 * @returns Updated context with network information
 */
export function addNetworkContext(
  context: EnhancedErrorContext,
  networkLogs: ApiRequestLog[],
  options: {
    /** Number of recent API calls to include */
    includeRecent?: number
    /** Include all failed calls */
    includeFailed?: boolean
  } = {}
): EnhancedErrorContext {
  const { includeRecent = 10, includeFailed = true } = options

  // Add recent API calls
  const recentLogs = networkLogs.slice(-includeRecent)
  context.recentApiCalls = recentLogs.map(log => ({
    url: log.url,
    method: log.method,
    status: log.status || 0,
    responseTime: log.duration || 0,
    requestId: log.requestId,
  }))

  // Add failed API calls
  if (includeFailed) {
    const failedLogs = networkLogs.filter(log =>
      log.error || (log.status && log.status >= 400)
    )
    context.failedApiCalls = failedLogs.map(log => ({
      url: log.url,
      method: log.method,
      error: log.error || `HTTP ${log.status}`,
      responseTime: log.duration,
    }))
  }

  return context
}

/**
 * Format enhanced error context for diagnostic report
 *
 * @param context Enhanced error context
 * @returns Formatted markdown string
 */
export function formatErrorContext(context: EnhancedErrorContext): string {
  const lines: string[] = []

  lines.push('## 错误上下文')
  lines.push('')
  lines.push(`**原始错误**: ${context.originalError}`)
  lines.push(`**时间戳**: ${context.timestamp}`)
  lines.push(`**当前 URL**: ${context.currentPageUrl}`)
  if (context.pageTitle) {
    lines.push(`**页面标题**: ${context.pageTitle}`)
  }
  lines.push('')

  // Element snapshot
  if (context.elementSnapshot) {
    lines.push('### 失败元素快照')
    lines.push('')
    lines.push('```html')
    lines.push(context.elementSnapshot.html)
    lines.push('```')
    lines.push('')
    lines.push(`**文本内容**: ${context.elementSnapshot.text}`)
    lines.push(`**可见性**: ${context.elementSnapshot.isVisible ? '是' : '否'}`)
    lines.push(`**是否分离**: ${context.elementSnapshot.isDetached ? '是' : '否'}`)
    lines.push(`**CSS display**: ${context.elementSnapshot.cssDisplay}`)
    lines.push(`**CSS visibility**: ${context.elementSnapshot.cssVisibility}`)
    if (Object.keys(context.elementSnapshot.attributes).length > 0) {
      lines.push('**属性**:')
      for (const [key, value] of Object.entries(context.elementSnapshot.attributes)) {
        lines.push(`  - ${key}: ${value}`)
      }
    }
    if (context.elementSnapshot.boundingBox) {
      const { x, y, width, height } = context.elementSnapshot.boundingBox
      lines.push(`**位置**: x=${x}, y=${y}, width=${width}, height=${height}`)
    }
    lines.push('')
  }

  // Visible elements
  if (context.visibleElements.length > 0) {
    lines.push('### 可见元素 (data-testid)')
    lines.push('')
    for (const elem of context.visibleElements.slice(0, 10)) {
      const status = elem.isVisible ? '✓' : '✗'
      lines.push(`- ${status} ${elem.selector}: ${elem.text}`)
    }
    lines.push('')
  }

  // Recent API calls
  if (context.recentApiCalls.length > 0) {
    lines.push('### 最近的 API 调用')
    lines.push('')
    for (const api of context.recentApiCalls) {
      const statusIcon = api.status >= 200 && api.status < 300 ? '✓' : '✗'
      lines.push(`- ${statusIcon} ${api.method} ${api.url} [${api.status}] ${api.responseTime}ms`)
      if (api.requestId) {
        lines.push(`  Request ID: ${api.requestId}`)
      }
    }
    lines.push('')
  }

  // Failed API calls
  if (context.failedApiCalls.length > 0) {
    lines.push('### 失败的 API 调用')
    lines.push('')
    for (const api of context.failedApiCalls) {
      lines.push(`- ✗ ${api.method} ${api.url}`)
      lines.push(`  错误: ${api.error}`)
      if (api.responseTime) {
        lines.push(`  响应时间: ${api.responseTime}ms`)
      }
    }
    lines.push('')
  }

  // Cookies
  if (context.cookies.length > 0) {
    lines.push('### 认证 Cookies')
    lines.push('')
    for (const cookie of context.cookies) {
      lines.push(`- ${cookie.name}: ${cookie.value}`)
    }
    lines.push('')
  }

  return lines.join('\n')
}

/**
 * Find similar selectors based on element snapshot
 *
 * @param context Enhanced error context
 * @param allTestIds All available testids from frontend
 * @returns Array of similar selectors
 */
export function findSimilarSelectors(
  context: EnhancedErrorContext,
  allTestIds: string[]
): Array<{ testId: string; similarity: number }> {
  if (!context.elementSnapshot?.testId) {
    return []
  }

  const targetTestid = context.elementSnapshot.testId || ''
  const targetText = context.elementSnapshot.text.toLowerCase()

  // Calculate similarity based on testid and text
  const similar = allTestIds
    .map(testId => {
      let similarity = 0

      // Compare testid similarity
      const testIdSimilarity = calculateLevenshteinSimilarity(targetTestid, testId)
      similarity = Math.max(similarity, testIdSimilarity)

      return { testId, similarity }
    })
    .filter(item => item.similarity >= 0.5) // Minimum 50% similarity
    .sort((a, b) => b.similarity - a.similarity)
    .slice(0, 5) // Return top 5 matches

  return similar
}

/**
 * Calculate Levenshtein similarity (0-1)
 */
function calculateLevenshteinSimilarity(str1: string, str2: string): number {
  const len1 = str1.length
  const len2 = str2.length

  if (len1 === 0) return len2 === 0 ? 1 : 0
  if (len2 === 0) return 0

  const matrix: number[][] = []
  for (let i = 0; i <= len1; i++) {
    matrix[i] = [i]
  }
  for (let j = 0; j <= len2; j++) {
    matrix[0][j] = j
  }

  for (let i = 1; i <= len1; i++) {
    for (let j = 1; j <= len2; j++) {
      const cost = str1[i - 1] === str2[j - 1] ? 0 : 1
      matrix[i][j] = Math.min(
        matrix[i - 1][j] + 1,
        matrix[i][j - 1] + 1,
        matrix[i - 1][j - 1] + cost
      )
    }
  }

  const distance = matrix[len1][len2]
  const maxLen = Math.max(len1, len2)

  return 1 - distance / maxLen
}
