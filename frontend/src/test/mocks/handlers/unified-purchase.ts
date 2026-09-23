/**
 * MSW handlers for unified-purchase feature
 *
 * Provides mock API responses for payment attempts
 */

import { http, HttpResponse } from 'msw'
import { mockPaymentAttempts } from '@/test/fixtures/unified-purchase'

/**
 * Payment Attempt handlers
 */
export const paymentAttemptHandlers = [
  // Create payment attempt
  http.post('/api/bill/purchase/payment-attempts', async ({ request }) => {
    const body = (await request.json()) as any
    const attempt = {
      ...mockPaymentAttempts.pending,
      id: `attempt-${Date.now()}`,
      ...body,
      createdAt: new Date().toISOString(),
    }
    return HttpResponse.json(attempt, { status: 201 })
  }),

  // Get payment attempt status
  http.get('/api/bill/purchase/payment-attempts/:attemptId', ({ params }) => {
    const attemptId = params.attemptId as string
    const attempt = Object.values(mockPaymentAttempts).find((a) => a.id === attemptId)

    if (!attempt) {
      return HttpResponse.json({ message: 'Payment attempt not found' }, { status: 404 })
    }

    return HttpResponse.json(attempt)
  }),

  // Cancel payment attempt
  http.post('/api/bill/purchase/payment-attempts/:attemptId/cancel', ({ params }) => {
    const attemptId = params.attemptId as string
    const attempt = Object.values(mockPaymentAttempts).find((a) => a.id === attemptId)

    if (!attempt) {
      return HttpResponse.json({ message: 'Payment attempt not found' }, { status: 404 })
    }

    // Return cancelled attempt
    return HttpResponse.json({
      ...attempt,
      status: 'Cancelled',
      completedAt: new Date().toISOString(),
    })
  }),
]

/**
 * Combined handlers for unified-purchase feature
 */
export const unifiedPurchaseHandlers = [...paymentAttemptHandlers]
