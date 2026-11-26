/**
 * Unified Log Query Interface for Demo Diagnostics
 *
 * Provides structured querying capabilities for:
 * - Network logs (API requests)
 * - Console logs (browser errors)
 * - Backend logs (from file)
 * - Aggregated queries by timestamp
 *
 * Usage in demo-diagnose:
 * ```typescript
 * import { LogQuery } from '../helpers/log-query'
 * const query = new LogQuery(networkLogs, consoleLogs, backendLogs)
 * const logs = query.aggregateByTimestamp('Test Name')
 * console.log(logs)
 * ```
 */

import { ApiRequestLog } from 'playwright-unified-logger'
import { ConsoleLogEntry } from 'playwright-unified-logger'

/**
 * Aggregated log entry with timestamp correlation
 */
export interface AggregatedLogEntry {
  timestamp: string
  type: 'network' | 'console' | 'backend' | 'test'
  source: string
  message: string
  details?: any
}

/**
 * Backend log entry parsed from log file
 */
export interface BackendLogEntry {
  timestamp: string
  level: 'INFO' | 'WARN' | 'ERROR' | 'DEBUG'
  message: string
  context?: string
  requestId?: string
  duration?: number
}

/**
 * Log aggregation result
 */
export interface AggregatedLogs {
  byTimestamp: AggregatedLogEntry[]
  byRequestId: Map<string, AggregatedLogEntry[]>
  networkLogs: ApiRequestLog[]
  consoleLogs: ConsoleLogEntry[]
  backendLogs: BackendLogEntry[]
  testLogs: string[]
  timeRange: { start: string; end: string }
}

/**
 * Query options
 */
export interface QueryOptions {
  /** Filter by timestamp range */
  timeRange?: { start: Date; end: Date }
  /** Filter by log type */
  type?: 'network' | 'console' | 'backend' | 'test'
  /** Filter by request ID */
  requestId?: string
  /** Filter by error level */
  errorLevel?: 'ERROR' | 'WARN' | 'INFO'
  /** Limit number of results */
  limit?: number
}

/**
 * Log query result
 */
export interface LogQueryResult {
  entries: AggregatedLogEntry[]
  count: number
  hasErrors: boolean
  errorCount: number
  timeRange?: { start: string; end: string }
}

/**
 * Unified log query interface
 */
export class LogQuery {
  private networkLogs: ApiRequestLog[]
  private consoleLogs: ConsoleLogEntry[]
  private backendLogs: BackendLogEntry[]
  private testLogs: string[]

  constructor(
    networkLogs: ApiRequestLog[],
    consoleLogs: ConsoleLogEntry[],
    backendLogs: BackendLogEntry[] = [],
    testLogs: string[] = []
  ) {
    this.networkLogs = networkLogs
    this.consoleLogs = consoleLogs
    this.backendLogs = backendLogs
    this.testLogs = testLogs
  }

  /**
   * Query logs by request ID
   *
   * @param requestId The request ID to search for
   * @returns Array of log entries matching the request ID
   */
  findByRequestId(requestId: string): AggregatedLogEntry[] {
    const entries: AggregatedLogEntry[] = []

    // Find matching network logs
    const networkEntries = this.networkLogs.filter(log => log.requestId === requestId)
    for (const log of networkEntries) {
      entries.push({
        timestamp: log.timestamp,
        type: 'network',
        source: `${log.method} ${log.url}`,
        message: log.error || `HTTP ${log.status}`,
        details: {
          method: log.method,
          url: log.url,
          status: log.status,
          duration: log.duration,
          requestBody: log.requestBody,
          responseBody: log.responseBody,
        },
      })
    }

    // Find matching backend logs
    const backendEntries = this.backendLogs.filter(log => log.requestId === requestId)
    for (const log of backendEntries) {
      entries.push({
        timestamp: log.timestamp,
        type: 'backend',
        source: log.context || 'backend',
        message: log.message,
        details: {
          level: log.level,
          duration: log.duration,
        },
      })
    }

    // Sort by timestamp
    entries.sort((a, b) => a.timestamp.localeCompare(b.timestamp))

    return entries
  }

  /**
   * Query logs by time range
   *
   * @param start Start time
   * @param end End time
   * @returns Array of log entries within time range
   */
  findByTimeRange(start: Date, end: Date): AggregatedLogEntry[] {
    const entries: AggregatedLogEntry[] = []

    const startTime = start.getTime()
    const endTime = end.getTime()

    // Add network logs
    for (const log of this.networkLogs) {
      const timestamp = new Date(log.timestamp).getTime()
      if (timestamp >= startTime && timestamp <= endTime) {
        entries.push({
          timestamp: log.timestamp,
          type: 'network',
          source: `${log.method} ${log.url}`,
          message: log.error || `HTTP ${log.status}`,
          details: {
            status: log.status,
            duration: log.duration,
          },
        })
      }
    }

    // Add console logs
    for (const log of this.consoleLogs) {
      const timestamp = new Date(log.timestamp).getTime()
      if (timestamp >= startTime && timestamp <= endTime) {
        entries.push({
          timestamp: log.timestamp,
          type: 'console',
          source: log.location || 'browser',
          message: log.text,
          details: {
            level: log.type,
          },
        })
      }
    }

    // Add backend logs
    for (const log of this.backendLogs) {
      const timestamp = new Date(log.timestamp).getTime()
      if (timestamp >= startTime && timestamp <= endTime) {
        entries.push({
          timestamp: log.timestamp,
          type: 'backend',
          source: log.context || 'backend',
          message: log.message,
          details: {
            level: log.level,
            requestId: log.requestId,
          },
        })
      }
    }

    // Sort by timestamp
    entries.sort((a, b) => a.timestamp.localeCompare(b.timestamp))

    return entries
  }

  /**
   * Aggregate logs by timestamp for a test scenario
   *
   * @param testName Test scenario name
   * @param options Optional aggregation options
   * @returns Aggregated logs with correlation
   */
  aggregateByTimestamp(
    testName: string,
    options: { includeTestLogs?: boolean } = {}
  ): AggregatedLogs {
    const { includeTestLogs = false } = options
    const allEntries: AggregatedLogEntry[] = []

    // Add network logs
    for (const log of this.networkLogs) {
      allEntries.push({
        timestamp: log.timestamp,
        type: 'network',
        source: `${log.method} ${log.url}`,
        message: log.error || `HTTP ${log.status}`,
        details: {
          status: log.status,
          duration: log.duration,
          requestId: log.requestId,
        },
      })
    }

    // Add console logs
    for (const log of this.consoleLogs) {
      allEntries.push({
        timestamp: log.timestamp,
        type: 'console',
        source: log.location || 'browser',
        message: log.text,
        details: {
          level: log.type,
        },
      })
    }

    // Add backend logs
    for (const log of this.backendLogs) {
      allEntries.push({
        timestamp: log.timestamp,
        type: 'backend',
        source: log.context || 'backend',
        message: log.message,
        details: {
          level: log.level,
          requestId: log.requestId,
        },
      })
    }

    // Add test logs
    if (includeTestLogs) {
      for (const log of this.testLogs) {
        allEntries.push({
          timestamp: new Date().toISOString(), // Test logs don't have timestamps
          type: 'test',
          source: testName,
          message: log,
        })
      }
    }

    // Sort by timestamp
    allEntries.sort((a, b) => a.timestamp.localeCompare(b.timestamp))

    // Calculate time range
    const startTime = allEntries.length > 0 ? allEntries[0].timestamp : ''
    const endTime = allEntries.length > 0 ? allEntries[allEntries.length - 1].timestamp : ''

    // Build request ID index
    const requestIdIndex = new Map<string, AggregatedLogEntry[]>()
    for (const entry of allEntries) {
      if (entry.details?.requestId) {
        const requestId = entry.details.requestId as string
        if (!requestIdIndex.has(requestId)) {
          requestIdIndex.set(requestId, [])
        }
        requestIdIndex.get(requestId)!.push(entry)
      }
    }

    return {
      byTimestamp: allEntries,
      byRequestId: requestIdIndex,
      networkLogs: this.networkLogs,
      consoleLogs: this.consoleLogs,
      backendLogs: this.backendLogs,
      testLogs: this.testLogs,
      timeRange: {
        start: startTime,
        end: endTime,
      },
    }
  }

  /**
   * Query logs with filters
   *
   * @param options Query options
   * @returns Filtered log entries
   */
  query(options: QueryOptions = {}): LogQueryResult {
    const entries: AggregatedLogEntry[] = []

    for (const log of this.aggregateByTimestamp('query').byTimestamp) {
      // Apply filters
      if (options.type && log.type !== options.type) {
        continue
      }

      if (options.requestId) {
        if (log.details?.requestId !== options.requestId) {
          continue
        }
      }

      if (options.errorLevel) {
        if (log.details?.level !== options.errorLevel) {
          continue
        }
      }

      if (options.timeRange) {
        const timestamp = new Date(log.timestamp).getTime()
        const start = options.timeRange.start.getTime()
        const end = options.timeRange.end.getTime()
        if (timestamp < start || timestamp > end) {
          continue
        }
      }

      entries.push(log)
    }

    // Apply limit
    const limitedEntries = options.limit
      ? entries.slice(0, options.limit)
      : entries

    // Calculate statistics
    const errorCount = limitedEntries.filter(
      entry =>
        entry.type === 'console' && entry.details?.level === 'error' ||
        entry.type === 'backend' && entry.details?.level === 'ERROR' ||
        entry.type === 'network' && entry.message.includes('error')
    ).length

    return {
      entries: limitedEntries,
      count: limitedEntries.length,
      hasErrors: errorCount > 0,
      errorCount,
    }
  }

  /**
   * Get failed requests only
   *
   * @returns Array of failed API requests
   */
  getFailedRequests(): ApiRequestLog[] {
    return this.networkLogs.filter(log =>
      log.error || (log.status && log.status >= 400)
    )
  }

  /**
   * Get console errors only
   *
   * @returns Array of console error logs
   */
  getConsoleErrors(): ConsoleLogEntry[] {
    return this.consoleLogs.filter(log => log.type === 'error')
  }

  /**
   * Get backend errors only
   *
   * @returns Array of backend error logs
   */
  getBackendErrors(): BackendLogEntry[] {
    return this.backendLogs.filter(log => log.level === 'ERROR')
  }

  /**
   * Find errors related to a specific request
   *
   * @param requestId Request ID
   * @returns Error entries related to the request
   */
  findErrorsByRequestId(requestId: string): AggregatedLogEntry[] {
    return this.findByRequestId(requestId).filter(
      entry =>
        entry.type === 'console' && entry.details?.level === 'error' ||
        entry.type === 'backend' && entry.details?.level === 'ERROR' ||
        entry.type === 'network' && entry.message.includes('error')
    )
  }

  /**
   * Format aggregated logs for diagnostic report
   *
   * @param aggregated Aggregated logs
   * @param testName Test scenario name
   * @returns Formatted markdown string
   */
  formatAggregatedLogs(aggregated: AggregatedLogs, testName: string): string {
    const lines: string[] = []

    lines.push(`## 日志聚合: ${testName}`)
    lines.push('')
    lines.push(`**时间范围**: ${aggregated.timeRange.start} → ${aggregated.timeRange.end}`)
    lines.push('')
    lines.push(`**日志统计**:`)
    lines.push(`- 网络日志: ${aggregated.networkLogs.length} 条`)
    lines.push(`- 控制台日志: ${aggregated.consoleLogs.length} 条`)
    lines.push(`- 后端日志: ${aggregated.backendLogs.length} 条`)
    lines.push(`- 测试日志: ${aggregated.testLogs.length} 条`)
    lines.push(`- 总计: ${aggregated.byTimestamp.length} 条`)
    lines.push('')

    // Show failed requests
    const failedRequests = this.getFailedRequests()
    if (failedRequests.length > 0) {
      lines.push('### 失败的 API 请求')
      lines.push('')
      for (const req of failedRequests) {
        lines.push(`- ${req.method} ${req.url}`)
        lines.push(`  状态: ${req.status || 'ERR'}`)
        if (req.error) {
          lines.push(`  错误: ${req.error}`)
        }
        if (req.requestId) {
          lines.push(`  Request ID: ${req.requestId}`)
        }
      }
      lines.push('')
    }

    // Show console errors
    const consoleErrors = this.getConsoleErrors()
    if (consoleErrors.length > 0) {
      lines.push('### 控制台错误')
      lines.push('')
      for (const err of consoleErrors.slice(0, 10)) { // Limit to 10
        lines.push(`- ${err.text}`)
        if (err.location) {
          lines.push(`  位置: ${err.location}`)
        }
      }
      lines.push('')
    }

    // Show backend errors
    const backendErrors = this.getBackendErrors()
    if (backendErrors.length > 0) {
      lines.push('### 后端错误')
      lines.push('')
      for (const err of backendErrors.slice(0, 10)) { // Limit to 10
        lines.push(`- [${err.timestamp}] ${err.message}`)
        if (err.context) {
          lines.push(`  上下文: ${err.context}`)
        }
        if (err.requestId) {
          lines.push(`  Request ID: ${err.requestId}`)
        }
      }
      lines.push('')
    }

    // Show recent logs (last 20 entries)
    lines.push('### 最近日志 (最后 20 条)')
    lines.push('')
    for (const entry of aggregated.byTimestamp.slice(-20)) {
      const icon = {
        network: entry.message.includes('error') ? '✗' : '📡',
        console: entry.details?.level === 'error' ? '✗' : '📝',
        backend: entry.details?.level === 'ERROR' ? '✗' : '⚙️',
        test: '🧪',
      }[entry.type]

      lines.push(`${icon} [${entry.timestamp}] [${entry.type.toUpperCase()}] ${entry.source}`)
      lines.push(`  ${entry.message}`)
    }
    lines.push('')

    return lines.join('\n')
  }

  /**
   * Parse backend logs from file content
   *
   * @param content Backend log file content
   * @returns Array of parsed backend log entries
   */
  static parseBackendLogs(content: string): BackendLogEntry[] {
    const entries: BackendLogEntry[] = []
    const lines = content.split('\n')

    for (const line of lines) {
      if (!line.trim()) {
        continue
      }

      // Try to parse log format: [timestamp] [LEVEL] message [request_id=xxx] [duration=xxxms]
      const match = line.match(
        /\[([^\]]+)\]\s+\[([A-Z]+)\]\s+(.+?)(?:\s+\[request_id=([^\]]+)\])?(?:\s+\[duration=(\d+)ms\])?$/
      )

      if (match) {
        const [, timestamp, level, message, requestId, duration] = match
        entries.push({
          timestamp,
          level: level as 'INFO' | 'WARN' | 'ERROR' | 'DEBUG',
          message: message.trim(),
          requestId,
          duration: duration ? parseInt(duration, 10) : undefined,
        })
      }
    }

    return entries
  }

  /**
   * Read backend log file
   *
   * @param logFilePath Path to backend log file
   * @returns Array of parsed backend log entries
   */
  static async readBackendLog(logFilePath: string): Promise<BackendLogEntry[]> {
    try {
      const fs = await import('fs/promises')
      const content = await fs.readFile(logFilePath, 'utf-8')
      return this.parseBackendLogs(content)
    } catch (error) {
      console.error('[LogQuery] Failed to read backend log:', error)
      return []
    }
  }
}
