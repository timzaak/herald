/**
 * Payment Simulation Helper
 *
 * Simulates payment fulfillment for demo tests without real payment processing.
 * Uses the internal fulfillment API to complete payment flows.
 */

import { type APIRequestContext } from '@playwright/test'

// Constants
const API_TIMEOUT = 10000
const POLL_TIMEOUT = 5000
const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

export interface SimulatedPointGrant {
  ruleId: string
  bucketId: string
  resultId: string
  pointsType: string
  points?: number | null
  description: string
}

function getInternalApiKey(): string {
  const apiKey = process.env.INTERNAL_API_KEY?.trim()
  if (!apiKey) {
    throw new Error('INTERNAL_API_KEY is required for payment simulation')
  }
  return apiKey
}

/**
 * Common payment status update helper
 */
async function updatePaymentStatus(
  request: APIRequestContext,
  realmId: string,
  attemptId: string,
  action: 'fulfill' | 'fail'
): Promise<{
  success: boolean
  status?: string
  transactionId?: string
  points?: number
  pointGrants?: SimulatedPointGrant[]
  error?: string
}> {
  try {
    const endpoint = action === 'fulfill' ? '/fulfill' : '/fail'

    const response = await request.post(
      `${BASE_URL}/api/internal/bill/purchase/payment-attempts/${attemptId}${endpoint}`,
      {
        headers: {
          'Content-Type': 'application/json',
          'X-Internal-API-Key': getInternalApiKey(),
        },
        data: {
          realmId,
          providerStatus: action === 'fulfill' ? 'succeeded' : 'failed',
          providerTransactionId: `demo-${action}-${attemptId}`,
          completedAt: new Date().toISOString(),
        },
        timeout: API_TIMEOUT,
      }
    )

    if (response.ok()) {
      const data = await response.json()
      return {
        success: true,
        status: data.status,
        ...data,
      }
    } else {
      const error = await response.text()
      return {
        success: false,
        error: `${action} failed: ${response.status()} - ${error}`,
      }
    }
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : 'Unknown error',
    }
  }
}

/**
 * Simulates successful payment fulfillment
 *
 * This calls the internal fulfillment endpoint which:
 * 1. Validates the payment attempt exists
 * 2. Updates payment attempt status to Succeeded
 * 3. Grants points to user account
 * 4. Creates transaction record
 * 5. Triggers webhook notification
 *
 * @param request - APIRequestContext for making HTTP requests
 * @param realmId - Realm ID
 * @param attemptId - Payment attempt ID to fulfill
 * @returns Response data
 */
export async function fulfillPayment(
  request: APIRequestContext,
  realmId: string,
  attemptId: string
): Promise<{
  success: boolean
  status?: string
  transactionId?: string
  points?: number
  pointGrants?: SimulatedPointGrant[]
  error?: string
}> {
  const result = await updatePaymentStatus(request, realmId, attemptId, 'fulfill')

  if (!result.success) {
    return { success: false, error: result.error }
  }

  if (result.transactionId || result.points || result.pointGrants) {
    return {
      success: true,
      status: result.status,
      transactionId: result.transactionId,
      points: result.points,
      pointGrants: result.pointGrants,
    }
  }

  // Fetch additional details
  try {
    const response = await request.get(
      `${BASE_URL}/api/bill/${realmId}/purchase/payment-attempts/${attemptId}`,
      {
        headers: {
          'Content-Type': 'application/json',
        },
        timeout: POLL_TIMEOUT,
      }
    )

    if (response.ok()) {
      const data = await response.json()
      return {
        success: true,
        status: data.status,
        transactionId: data.fulfillment?.transactionId,
        points: data.fulfillment?.points,
        pointGrants: data.fulfillment?.pointGrants,
      }
    }

    return { success: true, status: result.status }
  } catch {
    return { success: true, status: result.status }
  }
}

/**
 * Simulates failed payment
 *
 * Updates payment attempt status to Failed.
 *
 * @param request - APIRequestContext for making HTTP requests
 * @param realmId - Realm ID
 * @param attemptId - Payment attempt ID to fail
 * @returns Response data
 */
export async function failPayment(
  request: APIRequestContext,
  realmId: string,
  attemptId: string
): Promise<{
  success: boolean
  error?: string
}> {
  const result = await updatePaymentStatus(request, realmId, attemptId, 'fail')
  return {
    success: result.success,
    error: result.error,
  }
}

/**
 * Waits for payment attempt to reach a specific status
 *
 * @param request - APIRequestContext for making HTTP requests
 * @param realmId - Realm ID
 * @param attemptId - Payment attempt ID
 * @param targetStatus - Target status to wait for
 * @param timeout - Maximum time to wait (ms)
 * @returns Final status
 */
export async function waitForPaymentStatus(
  request: APIRequestContext,
  realmId: string,
  attemptId: string,
  targetStatus: string,
  timeout = 30000
): Promise<string> {
  const startTime = Date.now()
  let delay = 100 // Start at 100ms
  const maxDelay = 2000 // Max 2s between polls

  while (Date.now() - startTime < timeout) {
    try {
      const response = await request.get(
        `${BASE_URL}/api/bill/${realmId}/purchase/payment-attempts/${attemptId}`,
        {
          headers: {
            'Content-Type': 'application/json',
          },
          timeout: POLL_TIMEOUT,
        }
      )

      if (response.ok()) {
        const data = await response.json()
        if (data.status === targetStatus) {
          return data.status
        }
      }
    } catch {
      // Ignore errors and retry
    }

    // Exponential backoff with jitter
    await new Promise((resolve) => setTimeout(resolve, delay))
    delay = Math.min(delay * 1.5, maxDelay)
  }

  throw new Error(`Payment did not reach status ${targetStatus} within ${timeout}ms`)
}

/**
 * Gets current payment attempt status
 *
 * @param request - APIRequestContext for making HTTP requests
 * @param realmId - Realm ID
 * @param attemptId - Payment attempt ID
 * @returns Payment status data
 */
export async function getPaymentStatus(
  request: APIRequestContext,
  realmId: string,
  attemptId: string
): Promise<{
  status: string
  createdAt: string
  expiresAt: string
  fulfillment?: {
    transactionId: string
    points: number
  }
} | null> {
  try {
    const response = await request.get(
      `${BASE_URL}/api/bill/${realmId}/purchase/payment-attempts/${attemptId}`,
      {
        headers: {
          'Content-Type': 'application/json',
        },
        timeout: POLL_TIMEOUT,
      }
    )

    if (response.ok()) {
      return await response.json()
    }

    return null
  } catch {
    return null
  }
}
