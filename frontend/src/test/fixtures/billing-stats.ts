import type { PaymentStatsResponse, PointsConsumptionStatsResponse } from '@/lib/api-generated'

// Empty realms still receive a fully zero-filled series from the backend, so
// the defaults mirror that shape; tests override only the fields they assert.
export function makePaymentStats(
  overrides: Partial<PaymentStatsResponse> = {}
): PaymentStatsResponse {
  return {
    windowDays: 7,
    succeededCount: 0,
    failedCount: 0,
    amountsByCurrency: [],
    providers: [],
    paymentTrend: Array.from({ length: 7 }, (_, i) => ({
      date: `2026-09-0${i + 1}`,
      succeededCount: 0,
      failedCount: 0,
    })),
    ...overrides,
  }
}

export function makePointsStats(
  overrides: Partial<PointsConsumptionStatsResponse> = {}
): PointsConsumptionStatsResponse {
  return {
    windowDays: 7,
    totalConsumedPoints: 0,
    consumingUsers: 0,
    buckets: [],
    consumptionTrend: Array.from({ length: 7 }, (_, i) => ({
      date: `2026-09-0${i + 1}`,
      consumedPoints: 0,
    })),
    ...overrides,
  }
}
