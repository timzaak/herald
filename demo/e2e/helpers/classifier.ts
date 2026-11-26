/**
 * Problem Classification Decision Tree for Demo Tests
 *
 * Standardizes problem classification based on error patterns
 * Reduces manual judgment time and improves classification accuracy
 *
 * Usage in demo-diagnose:
 * ```typescript
 * import { classifyFailure } from '../helpers/classifier'
 * const result = await classifyFailure(errorMessage, networkLogs, backendLogs)
 * console.log(result.problemType) // 'FRONTEND_SELECTOR_MISMATCH'
 * console.log(result.recommendedAgent) // 'demo-dev'
 * ```
 */

import { ApiRequestLog } from 'playwright-unified-logger'

/**
 * Classification result with detailed diagnostics
 */
export interface ClassificationResult {
  problemType: 'TEST' | 'FRONTEND' | 'BACKEND' | 'ENV' | 'AUTH' | 'DATA'
  problemSubType: string
  severity: 'P0' | 'P1' | 'P2' | 'P3'
  confidence: number // 0-1, higher is more confident
  rootCause: string
  evidence: string[]
  suggestedFix: string
  recommendedAgent: 'demo-dev' | 'frontend-dev' | 'backend-dev' | 'devops'
  relatedFiles: Array<{ type: string; path: string; lines?: string }>
}

/**
 * Classify test failure based on error patterns
 *
 * @param errorMessage The error message from Playwright
 * @param networkLogs Network logs from UnifiedLogger
 * @param backendLogs Backend log file content (optional)
 * @returns Classification result with actionable recommendations
 */
export async function classifyFailure(
  errorMessage: string,
  networkLogs: ApiRequestLog[],
  backendLogs?: string
): Promise<ClassificationResult> {
  const result: ClassificationResult = {
    problemType: 'TEST',
    problemSubType: 'UNKNOWN',
    severity: 'P1',
    confidence: 0.5,
    rootCause: 'Unknown',
    evidence: [],
    suggestedFix: 'Manual analysis required',
    recommendedAgent: 'demo-dev',
    relatedFiles: [],
  }

  // Extract error characteristics
  const errorLower = errorMessage.toLowerCase()
  const failedRequests = networkLogs.filter(log => log.error || (log.status && log.status >= 400))
  const timeoutErrors = errorLower.includes('timeout')
  const selectorErrors = errorLower.includes('selector') || errorLower.includes('waiting for locator')
  const apiErrors = errorLower.includes('api') || errorLower.includes('http')
  const assertionErrors = errorLower.includes('expect') || errorLower.includes('assertion')

  // Step 1: Check for selector issues (most common, ~60%)
  if (selectorErrors) {
    return classifySelectorError(errorMessage, networkLogs)
  }

  // Step 2: Check for API failures
  if (apiErrors || failedRequests.length > 0) {
    return classifyApiError(errorMessage, networkLogs, backendLogs)
  }

  // Step 3: Check for timeout issues
  if (timeoutErrors) {
    return classifyTimeoutError(errorMessage, networkLogs)
  }

  // Step 4: Check for assertion failures
  if (assertionErrors) {
    return classifyAssertionError(errorMessage, networkLogs)
  }

  // Step 5: Default - general test failure
  result.problemType = 'TEST'
  result.problemSubType = 'GENERAL_FAILURE'
  result.severity = 'P2'
  result.confidence = 0.4
  result.rootCause = 'General test failure without clear pattern'
  result.evidence.push(errorMessage)
  result.suggestedFix = 'Review test code and logs for manual diagnosis'
  result.recommendedAgent = 'demo-dev'

  return result
}

/**
 * Classify selector-related errors
 */
function classifySelectorError(
  errorMessage: string,
  networkLogs: ApiRequestLog[]
): ClassificationResult {
  const errorLower = errorMessage.toLowerCase()
  const result: ClassificationResult = {
    problemType: 'FRONTEND',
    problemSubType: 'SELECTOR_MISMATCH',
    severity: 'P0',
    confidence: 0.85,
    rootCause: '',
    evidence: [],
    suggestedFix: '',
    recommendedAgent: 'demo-dev',
    relatedFiles: [],
  }

  // Extract selector from error message
  const selectorMatch = errorMessage.match(/selector[\"'\s:=]+\s*([^\)]+)/i)
  const selector = selectorMatch ? selectorMatch[1].trim() : 'unknown'

  result.evidence.push(`Error message: ${errorMessage}`)
  result.evidence.push(`Failing selector: ${selector}`)

  // Check network logs to see if API calls are working
  const apiSuccess = networkLogs.filter(log =>
    log.status && log.status >= 200 && log.status < 300
  ).length

  if (apiSuccess > 0) {
    // API is working, so it's a selector issue
    result.problemSubType = 'SELECTOR_MISMATCH'
    result.problemType = 'TEST' // Test code issue, not frontend
    result.recommendedAgent = 'demo-dev'
    result.rootCause = `Test selector "${selector}" does not match frontend element`
    result.suggestedFix = `Use selector-validator.ts to check if selector exists in frontend. If not, update test selector or add data-testid to frontend component.`
    result.relatedFiles.push({
      type: 'test_page_object',
      path: 'demo/e2e/pages/*.ts',
    })
    result.relatedFiles.push({
      type: 'frontend_component',
      path: 'frontend/src/components/**/*.tsx',
    })
  } else {
    // API also failing, might be frontend rendering issue
    result.problemSubType = 'FRONTEND_RENDER'
    result.problemType = 'FRONTEND'
    result.recommendedAgent = 'frontend-dev'
    result.rootCause = `Frontend element not rendered or visible. Possible rendering issue or missing data-testid.`
    result.suggestedFix = `Check if frontend component is rendering correctly. Verify data-testid attribute is present on the element.`
    result.relatedFiles.push({
      type: 'frontend_component',
      path: 'frontend/src/components/**/*.tsx',
    })
  }

  result.confidence = 0.9

  return result
}

/**
 * Classify API-related errors
 */
function classifyApiError(
  errorMessage: string,
  networkLogs: ApiRequestLog[],
  backendLogs?: string
): ClassificationResult {
  const failedRequests = networkLogs.filter(log => log.error || (log.status && log.status >= 400))

  const result: ClassificationResult = {
    problemType: 'BACKEND',
    problemSubType: 'API_FAILURE',
    severity: 'P0',
    confidence: 0.85,
    rootCause: '',
    evidence: [],
    suggestedFix: '',
    recommendedAgent: 'backend-dev',
    relatedFiles: [],
  }

  // Check for specific HTTP status codes
  for (const log of failedRequests) {
    if (log.status === 401) {
      return {
        problemType: 'AUTH',
        problemSubType: 'AUTH_FAILURE',
        severity: 'P0',
        confidence: 0.95,
        rootCause: 'Authentication failed - invalid credentials or expired token',
        evidence: [
          `API endpoint: ${log.url}`,
          `HTTP status: 401 Unauthorized`,
          `Error: ${log.error || log.responseBody || 'No details'}`,
        ],
        suggestedFix: 'Check login credentials. Verify frontend is sending correct fields (email vs username). Check X-Auth cookie.',
        recommendedAgent: 'frontend-dev',
        relatedFiles: [
          { type: 'frontend_route', path: 'frontend/src/routes/**/*.tsx' },
          { type: 'backend_service', path: 'backend/api/src/handlers/auth.rs' },
        ],
      }
    }

    if (log.status === 403) {
      return {
        problemType: 'AUTH',
        problemSubType: 'PERMISSION_DENIED',
        severity: 'P0',
        confidence: 0.95,
        rootCause: 'Permission denied - user lacks required permissions',
        evidence: [
          `API endpoint: ${log.url}`,
          `HTTP status: 403 Forbidden`,
          `Error: ${log.error || log.responseBody || 'No details'}`,
        ],
        suggestedFix: 'Check user roles and permissions. Verify RBAC configuration.',
        recommendedAgent: 'backend-dev',
        relatedFiles: [
          { type: 'backend_service', path: 'backend/core/src/domain/rbac/**/*.rs' },
        ],
      }
    }

    if (log.status === 404) {
      return {
        problemType: 'FRONTEND',
        problemSubType: 'WRONG_ENDPOINT',
        severity: 'P0',
        confidence: 0.9,
        rootCause: 'API endpoint not found - wrong URL or resource deleted',
        evidence: [
          `API endpoint: ${log.url}`,
          `HTTP status: 404 Not Found`,
        ],
        suggestedFix: 'Check if API endpoint URL is correct. Verify resource exists in database.',
        recommendedAgent: 'frontend-dev',
        relatedFiles: [
          { type: 'frontend_component', path: 'frontend/src/**/*.tsx' },
          { type: 'backend_service', path: 'backend/api/src/routes.rs' },
        ],
      }
    }

    if (log.status === 500 || log.status === 502) {
      return {
        problemType: 'BACKEND',
        problemSubType: 'BACKEND_ERROR',
        severity: 'P0',
        confidence: 0.95,
        rootCause: 'Backend internal error - check backend logs for stack trace',
        evidence: [
          `API endpoint: ${log.url}`,
          `HTTP status: ${log.status}`,
          `Error: ${log.error || log.responseBody || 'Server error'}`,
        ],
        suggestedFix: 'Check backend logs for error details. Fix backend bug.',
        recommendedAgent: 'backend-dev',
        relatedFiles: [
          { type: 'backend_service', path: 'backend/core/src/**/*.rs' },
        ],
      }
    }

    if (log.status === 400) {
      return {
        problemType: 'FRONTEND',
        problemSubType: 'INVALID_REQUEST',
        severity: 'P0',
        confidence: 0.85,
        rootCause: 'Invalid request - frontend sending wrong data',
        evidence: [
          `API endpoint: ${log.url}`,
          `HTTP status: 400 Bad Request`,
          `Request body: ${log.requestBody || 'Not logged'}`,
          `Response: ${log.responseBody || 'Not logged'}`,
        ],
        suggestedFix: 'Check request body format. Verify field names match backend API specification.',
        recommendedAgent: 'frontend-dev',
        relatedFiles: [
          { type: 'frontend_component', path: 'frontend/src/**/*.tsx' },
          { type: 'api_spec', path: 'docs/api/**/*.md' },
        ],
      }
    }
  }

  // Generic API error
  result.problemSubType = 'API_FAILURE'
  result.problemType = 'BACKEND'
  result.recommendedAgent = 'backend-dev'
  result.rootCause = 'API request failed with unknown error'
  result.evidence.push(
    ...failedRequests.map(log => `API error: ${log.method} ${log.url} - ${log.status || 'ERR'}`)
  )
  result.suggestedFix = 'Check backend logs for error details.'
  result.relatedFiles.push({
    type: 'backend_service',
    path: 'backend/core/src/**/*.rs',
  })

  return result
}

/**
 * Classify timeout errors
 */
function classifyTimeoutError(
  errorMessage: string,
  networkLogs: ApiRequestLog[]
): ClassificationResult {
  const result: ClassificationResult = {
    problemType: 'TEST',
    problemSubType: 'TIMEOUT',
    severity: 'P0',
    confidence: 0.8,
    rootCause: '',
    evidence: [],
    suggestedFix: '',
    recommendedAgent: 'demo-dev',
    relatedFiles: [],
  }

  result.evidence.push(`Error message: ${errorMessage}`)

  // Check if it's element timeout
  if (errorMessage.toLowerCase().includes('locator') || errorMessage.toLowerCase().includes('waiting for')) {
    result.problemSubType = 'ELEMENT_TIMEOUT'
    result.recommendedAgent = 'demo-dev'
    result.rootCause = 'Element not visible or not rendered within timeout period'
    result.suggestedFix = 'Check if element is being waited for correctly. Use expect().toBeVisible() instead of waitForTimeout(). Verify page has loaded completely.'
    result.relatedFiles.push({
      type: 'test_page_object',
      path: 'demo/e2e/pages/*.ts',
    })
  } else if (errorMessage.toLowerCase().includes('navigation')) {
    result.problemSubType = 'NAVIGATION_TIMEOUT'
    result.recommendedAgent = 'demo-dev'
    result.rootCause = 'Page navigation did not complete within timeout period'
    result.suggestedFix = 'Check if page URL is correct. Verify page is loading. Check for infinite redirects or slow loading pages.'
    result.relatedFiles.push({
      type: 'test',
      path: 'demo/e2e/**/*.e2e.ts',
    })
  } else {
    result.problemSubType = 'GENERAL_TIMEOUT'
    result.recommendedAgent = 'demo-dev'
    result.rootCause = 'Operation timed out - slow response or infinite wait'
    result.suggestedFix = 'Increase timeout or check why operation is slow. Verify no blocking operations.'
  }

  // Check if network requests are slow
  const slowRequests = networkLogs.filter(log => log.duration && log.duration > 5000)
  if (slowRequests.length > 0) {
    result.evidence.push(
      ...slowRequests.map(log => `Slow request: ${log.method} ${log.url} - ${log.duration}ms`)
    )
    result.problemType = 'BACKEND'
    result.recommendedAgent = 'backend-dev'
    result.suggestedFix += ' Backend API is responding slowly. Check backend performance.'
  }

  return result
}

/**
 * Classify assertion errors
 */
function classifyAssertionError(
  errorMessage: string,
  networkLogs: ApiRequestLog[]
): ClassificationResult {
  const result: ClassificationResult = {
    problemType: 'TEST',
    problemSubType: 'ASSERTION_FAILED',
    severity: 'P1',
    confidence: 0.75,
    rootCause: '',
    evidence: [],
    suggestedFix: '',
    recommendedAgent: 'demo-dev',
    relatedFiles: [],
  }

  result.evidence.push(`Error message: ${errorMessage}`)

  // Check specific assertion types
  if (errorMessage.toLowerCase().includes('tobevisible')) {
    result.problemSubType = 'VISIBILITY_ASSERTION_FAILED'
    result.rootCause = 'Expected element to be visible but it was not'
    result.suggestedFix = 'Check if element is actually visible. Verify CSS display property. Check if element is in viewport.'
  } else if (errorMessage.toLowerCase().includes('tocontaintext') || errorMessage.toLowerCase().includes('tocontain')) {
    result.problemSubType = 'TEXT_ASSERTION_FAILED'
    result.rootCause = 'Expected text not found in element or page'
    result.suggestedFix = 'Verify text content is correct. Check for case sensitivity. Verify element has loaded.'
  } else if (errorMessage.toLowerCase().includes('tohaveurl')) {
    result.problemSubType = 'URL_ASSERTION_FAILED'
    result.rootCause = 'Expected URL did not match actual URL'
    result.suggestedFix = 'Check if page navigated correctly. Verify URL pattern. Check for unexpected redirects.'
  } else if (errorMessage.toLowerCase().includes('toast')) {
    // Special case: toast assertions are unstable
    result.problemSubType = 'TOAST_ASSERTION_FAILED'
    result.rootCause = 'Using toast as validation condition (unstable)'
    result.severity = 'P0'
    result.confidence = 0.95
    result.suggestedFix = 'Do NOT use toast as validation condition. Verify actual business results: list update, page navigation, dialog closure, data existence. See .claude/mistakes/demo-repair.md'
  } else {
    result.problemSubType = 'GENERAL_ASSERTION_FAILED'
    result.rootCause = 'Assertion condition not met'
    result.suggestedFix = 'Check if test expectation is correct. Verify test data is properly initialized.'
  }

  result.relatedFiles.push({
    type: 'test',
    path: 'demo/e2e/**/*.e2e.ts',
  })

  return result
}

/**
 * Format classification result for diagnostic report
 */
export function formatClassificationResult(result: ClassificationResult): string {
  const lines: string[] = []

  lines.push('## 问题分类')
  lines.push('')
  lines.push(`**问题类型**: ${result.problemType}`)
  lines.push(`**子类型**: ${result.problemSubType}`)
  lines.push(`**严重级别**: ${result.severity}`)
  lines.push(`**置信度**: ${(result.confidence * 100).toFixed(0)}%`)
  lines.push(`**推荐 Agent**: ${result.recommendedAgent}`)
  lines.push('')
  lines.push('### 根本原因')
  lines.push('')
  lines.push(result.rootCause)
  lines.push('')
  lines.push('### 证据')
  lines.push('')
  for (const evidence of result.evidence) {
    lines.push(`- ${evidence}`)
  }
  lines.push('')
  lines.push('### 建议修复')
  lines.push('')
  lines.push(result.suggestedFix)
  lines.push('')
  lines.push('### 相关文件')
  lines.push('')

  for (const file of result.relatedFiles) {
    lines.push(`- **${file.type}**: \`${file.path}\`${file.lines ? ` (lines ${file.lines})` : ''}`)
  }

  return lines.join('\n')
}

