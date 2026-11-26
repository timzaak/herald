/**
 * Demo-Diagnose Diagnostic Workflow
 *
 * Complete diagnostic workflow for analyzing demo test failures
 *
 * Workflow:
 * 1. Extract error characteristics
 * 2. Example verification check
 * 3. Query mistake database
 * 4. Intelligent problem classification
 * 5. Generate diagnostic report
 *
 * Usage:
 * ```typescript
 * import { DiagnosticWorkflow } from '../helpers/diagnostic-workflow'
 * const workflow = new DiagnosticWorkflow(logs, testInfo)
 * const report = await workflow.run()
 * console.log(report)
 * ```
 */

import { readFileSync, existsSync, writeFileSync } from 'fs'
import { join } from 'path'
import { ApiRequestLog } from 'playwright-unified-logger'
import { ConsoleLogEntry } from 'playwright-unified-logger'
import { validateSelector, ValidationResult } from './selector-validator'
import { classifyFailure, ClassificationResult } from './classifier'
import { enhanceError, EnhancedErrorContext, addNetworkContext } from './error-enhancer'
import { LogQuery, BackendLogEntry } from './log-query'
import { detectProblems, getProblemStatistics, DetectedProblem } from './problem-detector'

/**
 * Test information
 */
export interface TestInfo {
  testName: string
  testFile: string
  testScenario?: string
  runId?: string
  startTime?: string
  endTime?: string
}

/**
 * Logs from test execution
 */
export interface TestLogs {
  networkLogs: ApiRequestLog[]
  consoleLogs: ConsoleLogEntry[]
  backendLogs?: BackendLogEntry[]
  testCodeLogs: string[]
  unifiedLogFile?: string
  networkLogFile?: string
}

/**
 * Diagnostic configuration
 */
export interface DiagnosticConfig {
  /** Mistake database path */
  mistakeDbPath?: string
  /** Backend log file path */
  backendLogPath?: string
  /** Output directory for reports */
  outputDir?: string
  /** Enable frontend scanning for selector validation */
  enableFrontendScan?: boolean
}

/**
 * Diagnostic report result
 */
export interface DiagnosticReport {
  testInfo: TestInfo
  errorContext: EnhancedErrorContext
  classification: ClassificationResult
  detectedProblems: DetectedProblem[]
  selectorValidations: ValidationResult[]
  aggregatedLogs: ReturnType<LogQuery['aggregateByTimestamp']>
  recommendations: string[]
  summary: {
    problemType: string
    severity: string
    confidence: number
    recommendedAgent: string
    estimatedFixTime: string
  }
}

/**
 * Complete diagnostic workflow for demo test failures
 */
export class DiagnosticWorkflow {
  private testInfo: TestInfo
  private logs: TestLogs
  private config: Required<DiagnosticConfig>

  constructor(
    testInfo: TestInfo,
    logs: TestLogs,
    config: DiagnosticConfig = {}
  ) {
    this.testInfo = testInfo
    this.logs = logs
    this.config = {
      mistakeDbPath: config.mistakeDbPath || join(process.cwd(), '.claude/mistakes/demo-repair.md'),
      backendLogPath: config.backendLogPath || join(process.cwd(), 'log/backend-demo.log'),
      outputDir: config.outputDir || join(process.cwd(), '.ai/diagnose'),
      enableFrontendScan: config.enableFrontendScan ?? true,
    }
  }

  /**
   * Run complete diagnostic workflow
   *
   * @returns Diagnostic report with analysis and recommendations
   */
  async run(): Promise<DiagnosticReport> {
    console.log(`[DiagnosticWorkflow] Starting diagnosis for: ${this.testInfo.testName}`)
    console.log(`[DiagnosticWorkflow] Test file: ${this.testInfo.testFile}`)

    const report: Partial<DiagnosticReport> = {
      testInfo: this.testInfo,
    }

    // Step 1: Extract error characteristics
    const errorMessage = this.extractErrorMessage()
    console.log(`[DiagnosticWorkflow] Step 1: Error characteristics extracted`)

    // Step 2: Example verification check
    const verificationResults = await this.verifyTestExample(errorMessage)
    console.log(`[DiagnosticWorkflow] Step 2: Test example verification completed`)

    // Step 3: Query mistake database
    const mistakeEntries = this.queryMistakeDatabase(errorMessage)
    console.log(`[DiagnosticWorkflow] Step 3: Found ${mistakeEntries.length} relevant mistake entries`)

    // Step 4: Intelligent problem classification
    const classification = await classifyFailure(
      errorMessage,
      this.logs.networkLogs,
      this.logs.backendLogs?.map(log => `[${log.timestamp}] [${log.level}] ${log.message}`).join('\n')
    )
    report.classification = classification
    console.log(`[DiagnosticWorkflow] Step 4: Problem classified as ${classification.problemType}/${classification.problemSubType}`)

    // Step 5: Detect common problems
    const detectedProblems = detectProblems(errorMessage)
    report.detectedProblems = detectedProblems
    console.log(`[DiagnosticWorkflow] Step 5: Detected ${detectedProblems.length} common problems`)

    // Step 6: Validate selectors
    const selectorValidations = await this.validateSelectors(errorMessage)
    report.selectorValidations = selectorValidations
    console.log(`[DiagnosticWorkflow] Step 6: Validated ${selectorValidations.length} selectors`)

    // Step 7: Aggregate logs
    const logQuery = new LogQuery(
      this.logs.networkLogs,
      this.logs.consoleLogs,
      this.logs.backendLogs || [],
      this.logs.testCodeLogs
    )
    const aggregatedLogs = logQuery.aggregateByTimestamp(this.testInfo.testName, {
      includeTestLogs: true,
    })
    report.aggregatedLogs = aggregatedLogs
    console.log(`[DiagnosticWorkflow] Step 7: Aggregated ${aggregatedLogs.byTimestamp.length} log entries`)

    // Step 8: Enhance error context
    // Note: Actual Page object is not available in this workflow context
    // So we'll create a minimal context from logs
    const minimalContext: EnhancedErrorContext = {
      originalError: errorMessage,
      timestamp: new Date().toISOString(),
      currentPageUrl: this.getCurrentUrlFromLogs(),
      visibleElements: [],
      recentApiCalls: this.logs.networkLogs.slice(-5).map(log => ({
        url: log.url,
        method: log.method,
        status: log.status || 0,
        responseTime: log.duration || 0,
        requestId: log.requestId,
      })),
      failedApiCalls: this.logs.networkLogs
        .filter(log => log.error || (log.status && log.status >= 400))
        .map(log => ({
          url: log.url,
          method: log.method,
          error: log.error || `HTTP ${log.status}`,
          responseTime: log.duration,
        })),
      consoleErrors: this.logs.consoleLogs
        .filter(log => log.type === 'error')
        .map(log => ({
          message: log.text,
          stack: log.location,
        })),
      cookies: [],
    }

    report.errorContext = addNetworkContext(minimalContext, this.logs.networkLogs)
    console.log(`[DiagnosticWorkflow] Step 8: Enhanced error context`)

    // Step 9: Generate recommendations
    const recommendations = this.generateRecommendations(
      classification,
      detectedProblems,
      selectorValidations,
      verificationResults
    )
    report.recommendations = recommendations
    console.log(`[DiagnosticWorkflow] Step 9: Generated ${recommendations.length} recommendations`)

    // Step 10: Generate summary
    report.summary = this.generateSummary(
      classification,
      detectedProblems,
      recommendations
    )
    console.log(`[DiagnosticWorkflow] Step 10: Summary generated`)
    console.log(`[DiagnosticWorkflow] Problem type: ${report.summary.problemType}`)
    console.log(`[DiagnosticWorkflow] Severity: ${report.summary.severity}`)
    console.log(`[DiagnosticWorkflow] Recommended agent: ${report.summary.recommendedAgent}`)

    const fullReport = report as DiagnosticReport

    // Step 11: Write diagnostic report
    await this.writeDiagnosticReport(fullReport)

    return fullReport
  }

  /**
   * Extract error message from logs
   */
  private extractErrorMessage(): string {
    // Try to find error in console logs
    const consoleErrors = this.logs.consoleLogs
      .filter(log => log.type === 'error')
      .map(log => log.text)

    if (consoleErrors.length > 0) {
      return consoleErrors.join('\n')
    }

    // Try to find error in failed API requests
    const apiErrors = this.logs.networkLogs
      .filter(log => log.error || (log.status && log.status >= 400))
      .map(log => `API Error: ${log.method} ${log.url} - ${log.error || `HTTP ${log.status}`}`)

    if (apiErrors.length > 0) {
      return apiErrors.join('\n')
    }

    // Try to find error in test code logs
    if (this.logs.testCodeLogs.length > 0) {
      return this.logs.testCodeLogs.join('\n')
    }

    return 'No error message found in logs'
  }

  /**
   * Verify test example (Step 2 - Example Verification Check)
   *
   * This is the highest priority step - verifies test code itself
   * before classifying as runtime issue
   */
  private async verifyTestExample(errorMessage: string): Promise<{
    selectorsValid: boolean
    testDataValid: boolean
    assertionsValid: boolean
    issues: string[]
  }> {
    const results = {
      selectorsValid: true,
      testDataValid: true,
      assertionsValid: true,
      issues: [] as string[],
    }

    // Check for common test code issues
    if (errorMessage.includes('toast')) {
      results.assertionsValid = false
      results.issues.push('Using toast as validation condition (P0 error)')
    }

    if (errorMessage.includes('selector') && errorMessage.includes('not found')) {
      results.selectorsValid = false
      results.issues.push('Selector not found - check frontend data-testid or test selector')
    }

    if (errorMessage.includes('expect') && errorMessage.includes('not defined')) {
      results.issues.push('Missing import statement (expect, test, etc.)')
    }

    if (errorMessage.includes('demoLogger') && errorMessage.includes('is not a function')) {
      results.issues.push('Using demoLogger.log() - use console.log() instead')
    }

    if (errorMessage.includes('login') && errorMessage.includes('not found')) {
      results.issues.push('Possible login state conflict - user may already be authenticated')
    }

    // Check if backend is healthy
    const failedRequests = this.logs.networkLogs.filter(log =>
      log.error || (log.status && log.status >= 500)
    )
    if (failedRequests.length > 0) {
      results.issues.push(`${failedRequests.length} backend API requests failed`)
    }

    // Check if data was initialized
    const initRequests = this.logs.networkLogs.filter(log =>
      log.url.includes('/init') || log.url.includes('/setup')
    )
    if (initRequests.length === 0) {
      results.testDataValid = false
      results.issues.push('No initialization requests found - test data may not be set up')
    }

    return results
  }

  /**
   * Query mistake database (Step 3)
   */
  private queryMistakeDatabase(errorMessage: string): Array<{
    entry: string
    matchScore: number
  }> {
    if (!existsSync(this.config.mistakeDbPath)) {
      return []
    }

    try {
      const content = readFileSync(this.config.mistakeDbPath, 'utf-8')
      const lines = content.split('\n')

      const matched: Array<{ entry: string; matchScore: number }> = []
      const errorLower = errorMessage.toLowerCase()
      const errorWords = errorLower.split(/\s+/)

      for (const line of lines) {
        if (!line.trim() || line.startsWith('#')) {
          continue
        }

        const lineLower = line.toLowerCase()
        let matchScore = 0

        // Count matching words
        for (const word of errorWords) {
          if (lineLower.includes(word)) {
            matchScore++
          }
        }

        if (matchScore > 0) {
          matched.push({
            entry: line.trim(),
            matchScore,
          })
        }
      }

      // Sort by match score and return top 10
      return matched.sort((a, b) => b.matchScore - a.matchScore).slice(0, 10)
    } catch (error) {
      console.error('[DiagnosticWorkflow] Failed to query mistake database:', error)
      return []
    }
  }

  /**
   * Validate selectors (Step 6)
   */
  private async validateSelectors(errorMessage: string): Promise<ValidationResult[]> {
    const selectors = this.extractSelectorsFromError(errorMessage)
    const validations: ValidationResult[] = []

    for (const selector of selectors) {
      try {
        const result = await validateSelector(selector, {
          similarityThreshold: 0.7,
          spellingThreshold: 0.8,
        })
        validations.push(result)
      } catch (error) {
        console.error('[DiagnosticWorkflow] Selector validation failed:', error)
      }
    }

    return validations
  }

  /**
   * Extract selectors from error message
   */
  private extractSelectorsFromError(errorMessage: string): string[] {
    const selectors: string[] = []

    // Match data-testid patterns
    const testidMatches = errorMessage.matchAll(/data-testid\s*=\s*["']([^"']+)["']/g)
    for (const match of testidMatches) {
      selectors.push(`[data-testid="${match[1]}"]`)
    }

    // Match CSS selectors
    const cssMatches = errorMessage.matchAll(/[["']([.#][a-zA-Z][a-zA-Z0-9_-]*)["']/g)
    for (const match of cssMatches) {
      selectors.push(match[1])
    }

    return selectors
  }

  /**
   * Get current URL from logs
   */
  private getCurrentUrlFromLogs(): string {
    // Try to find URL in network logs (last GET request)
    const getLastGetRequest = () => {
      for (let i = this.logs.networkLogs.length - 1; i >= 0; i--) {
        if (this.logs.networkLogs[i].method === 'GET') {
          return this.logs.networkLogs[i].url
        }
      }
      return ''
    }

    return getLastGetRequest()
  }

  /**
   * Generate recommendations
   */
  private generateRecommendations(
    classification: ClassificationResult,
    detectedProblems: DetectedProblem[],
    selectorValidations: ValidationResult[],
    verificationResults: any
  ): string[] {
    const recommendations: string[] = []

    // Add classification-based recommendation
    recommendations.push(`**Classification**: ${classification.problemType}/${classification.problemSubType}`)
    recommendations.push(`**Root cause**: ${classification.rootCause}`)
    recommendations.push(`**Suggested fix**: ${classification.suggestedFix}`)
    recommendations.push(`**Recommended agent**: ${classification.recommendedAgent}`)
    recommendations.push('')

    // Add common problem recommendations
    if (detectedProblems.length > 0) {
      recommendations.push('**Detected common problems**:')
      for (const problem of detectedProblems) {
        recommendations.push(`- [${problem.severity}] ${problem.category}: ${problem.fix.substring(0, 100)}...`)
        if (problem.reference) {
          recommendations.push(`  Reference: ${problem.reference}`)
        }
      }
      recommendations.push('')
    }

    // Add selector validation recommendations
    const invalidSelectors = selectorValidations.filter(v => v.recommendation !== 'OK')
    if (invalidSelectors.length > 0) {
      recommendations.push('**Selector validation issues**:')
      for (const validation of invalidSelectors) {
        recommendations.push(`- ${validation.testSelector}: ${validation.recommendation}`)
        if (validation.suggestedFix) {
          recommendations.push(`  Suggested: ${validation.suggestedFix}`)
        }
      }
      recommendations.push('')
    }

    // Add verification recommendations
    if (verificationResults.issues.length > 0) {
      recommendations.push('**Test code verification issues**:')
      for (const issue of verificationResults.issues) {
        recommendations.push(`- ${issue}`)
      }
      recommendations.push('')
    }

    return recommendations
  }

  /**
   * Generate summary
   */
  private generateSummary(
    classification: ClassificationResult,
    detectedProblems: DetectedProblem[],
    recommendations: string[]
  ): DiagnosticReport['summary'] {
    // Determine problem type
    const problemType = detectedProblems.length > 0
      ? detectedProblems[0].category
      : classification.problemType

    // Determine severity
    const severity = detectedProblems.length > 0
      ? detectedProblems[0].severity
      : classification.severity

    // Determine confidence
    const confidence = Math.max(
      classification.confidence,
      detectedProblems.length > 0 ? 0.95 : 0
    )

    // Determine recommended agent
    const recommendedAgent = detectedProblems.length > 0
      ? classification.recommendedAgent
      : classification.recommendedAgent

    // Estimate fix time based on problem type and severity
    const fixTimeMap: Record<string, Record<string, string>> = {
      TEST: {
        P0: '5-15 minutes',
        P1: '15-30 minutes',
        P2: '30-60 minutes',
        P3: '1-2 hours',
      },
      FRONTEND: {
        P0: '15-30 minutes',
        P1: '30-60 minutes',
        P2: '1-2 hours',
        P3: '2-4 hours',
      },
      BACKEND: {
        P0: '30-60 minutes',
        P1: '1-2 hours',
        P2: '2-4 hours',
        P3: '4-8 hours',
      },
      ENV: {
        P0: '5-10 minutes',
        P1: '10-15 minutes',
        P2: '15-30 minutes',
        P3: '30-60 minutes',
      },
    }

    const estimatedFixTime = fixTimeMap[problemType]?.[severity] || 'Unknown'

    return {
      problemType: `${problemType}/${classification.problemSubType}`,
      severity,
      confidence,
      recommendedAgent,
      estimatedFixTime,
    }
  }

  /**
   * Write diagnostic report to file
   */
  private async writeDiagnosticReport(report: DiagnosticReport): Promise<void> {
    const fs = await import('fs/promises')

    // Ensure output directory exists
    await fs.mkdir(this.config.outputDir, { recursive: true })

    // Generate filename
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-')
    const filename = join(
      this.config.outputDir,
      `${this.testInfo.testName}-${timestamp}.md`
    )

    // Generate report content
    const content = this.formatReport(report)

    // Write report
    await fs.writeFile(filename, content, 'utf-8')

    console.log(`[DiagnosticWorkflow] Report written to: ${filename}`)
  }

  /**
   * Format report as markdown
   */
  private formatReport(report: DiagnosticReport): string {
    const lines: string[] = []

    lines.push(`# 诊断报告: ${this.testInfo.testName}`)
    lines.push('')
    lines.push(`**生成时间**: ${new Date().toISOString()}`)
    lines.push(`**测试文件**: ${this.testInfo.testFile}`)
    if (this.testInfo.testScenario) {
      lines.push(`**测试场景**: ${this.testInfo.testScenario}`)
    }
    if (this.testInfo.runId) {
      lines.push(`**Run ID**: ${this.testInfo.runId}`)
    }
    lines.push('')

    lines.push('---')
    lines.push('')

    // Summary
    lines.push('## 概要')
    lines.push('')
    lines.push(`**问题类型**: ${report.summary.problemType}`)
    lines.push(`**严重级别**: ${report.summary.severity}`)
    lines.push(`**置信度**: ${(report.summary.confidence * 100).toFixed(0)}%`)
    lines.push(`**推荐 Agent**: ${report.summary.recommendedAgent}`)
    lines.push(`**预计修复时间**: ${report.summary.estimatedFixTime}`)
    lines.push('')

    // Error context
    lines.push(report.errorContext ? report.errorContext.originalError : 'No error context available')
    lines.push('')

    // Classification
    lines.push(`**问题类型**: ${report.classification.problemType}`)
    lines.push(`**子类型**: ${report.classification.problemSubType}`)
    lines.push(`**根本原因**: ${report.classification.rootCause}`)
    lines.push('')

    // Recommendations
    lines.push('## 修复建议')
    lines.push('')
    for (const rec of report.recommendations) {
      lines.push(rec)
    }
    lines.push('')

    // Related files
    lines.push('## 相关文件')
    lines.push('')
    for (const file of report.classification.relatedFiles) {
      lines.push(`- **${file.type}**: \`${file.path}\`${file.lines ? ` (lines ${file.lines})` : ''}`)
    }
    lines.push('')

    // Aggregated logs summary
    lines.push('## 日志摘要')
    lines.push('')
    lines.push(`- 网络日志: ${report.aggregatedLogs.networkLogs.length} 条`)
    lines.push(`- 控制台日志: ${report.aggregatedLogs.consoleLogs.length} 条`)
    lines.push(`- 后端日志: ${report.aggregatedLogs.backendLogs?.length || 0} 条`)
    lines.push(`- 测试日志: ${report.aggregatedLogs.testLogs.length} 条`)
    lines.push('')

    return lines.join('\n')
  }
}
