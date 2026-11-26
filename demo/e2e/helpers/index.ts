/**
 * Demo Diagnostics Helper Modules
 *
 * Central export point for all diagnostic helper modules
 * Used by demo-diagnose agent for test failure analysis
 *
 * Main workflow:
 * ```typescript
 * import { DiagnosticWorkflow } from './helpers/diagnostic-workflow'
 * const workflow = new DiagnosticWorkflow(testInfo, logs, config)
 * const report = await workflow.run()
 * ```
 */

// Main diagnostic workflow
export { DiagnosticWorkflow } from './diagnostic-workflow'
export type {
  TestInfo,
  TestLogs,
  DiagnosticConfig,
  DiagnosticReport,
} from './diagnostic-workflow'

// Problem classification
export { classifyFailure, formatClassificationResult } from './classifier'
export type {
  ClassificationResult,
} from './classifier'

// Error enhancement
export { enhanceError, addNetworkContext, formatErrorContext } from './error-enhancer'
export type {
  EnhancedErrorContext,
  ElementSnapshot,
} from './error-enhancer'

// Log query
export { LogQuery } from './log-query'
export type {
  AggregatedLogs,
  QueryOptions,
  LogQueryResult,
} from './log-query'
export type { BackendLogEntry } from './log-query'

// Problem detector
export {
  detectProblem,
  detectProblems,
  getProblemStatistics,
  formatDetectedProblem,
  hasKnownProblem,
  getHighestPriorityProblem,
} from './problem-detector'
export type {
  DetectedProblem,
  PatternMatch,
} from './problem-detector'

// Selector validator
export {
  validateSelector,
  validateSelectors,
  generateDiagnosticMessage,
  calculateSimilarity,
  extractTestid,
  findSimilarSelectors,
  extractSelectorsFromTs,
  extractFrontendTestids,
  checkFrontendCode,
} from './selector-validator'
export type {
  ValidationResult,
  RecommendationType,
} from './selector-validator'

export { validateBackendHealth, formatValidationErrors, quickHealthCheck } from './api-validator'
export type {
  BackendHealthResponse,
  ValidationResult as ApiValidationResult,
  ValidationError as ApiValidationError,
  ValidationWarning as ApiValidationWarning,
  ValidationOptions as ApiValidationOptions,
} from './api-validator'
