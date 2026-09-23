import { queryOptions } from '@tanstack/react-query'
import {
  listInvoices,
  getInvoice,
  listMyInvoices,
  getMyInvoiceScoped,
  getSellerConfig,
  listRealmConfigsByType,
  getInvoiceApplyEligibility,
} from '@/lib/api-generated'
import type {
  InvoiceListResponse,
  InvoiceDetailResponse,
  SellerConfigResponse,
  RealmConfigResponse,
  InvoiceApplyEligibilityResponse,
} from '@/lib/api-generated'
import { QUERY_TIMING } from '@/lib/constants'

const { GC_TIME_5_MIN, RETRY_COUNT, STALE_TIME_2_MIN, STALE_TIME_5_MIN } = QUERY_TIMING

export const invoiceKeys = {
  all: (realmId: string) => ['invoices', realmId] as const,
  list: (realmId: string, query?: Record<string, unknown>) =>
    ['invoices', realmId, 'list', query] as const,
  detail: (realmId: string, invoiceId: string) =>
    ['invoices', realmId, 'detail', invoiceId] as const,
  sellerConfig: (realmId: string) => ['invoices', realmId, 'seller-config'] as const,
  myAll: (realmId: string) => ['invoices', realmId, 'my'] as const,
  myList: (realmId: string, query?: Record<string, unknown>) =>
    ['invoices', realmId, 'my', 'list', query] as const,
  myDetail: (realmId: string, invoiceId: string) =>
    ['invoices', realmId, 'my', 'detail', invoiceId] as const,
  policyConfig: (realmId: string) => ['invoices', realmId, 'policy-config'] as const,
  applyEligibility: (
    realmId: string,
    referenceType: 'payment_attempt' | 'subscription',
    referenceId: string
  ) => ['invoices', realmId, 'apply-eligibility', referenceType, referenceId] as const,
}

export function invoiceListQueryOptions(
  realmId: string,
  query?: {
    status?: string
    source?: string
    search?: string
    dateFrom?: string
    dateTo?: string
    page?: number
    pageSize?: number
    provider?: string
    attribution?: string
  }
) {
  return queryOptions({
    queryKey: invoiceKeys.list(realmId, query),
    queryFn: async () => {
      const response = await listInvoices({
        query,
      })
      if (response.error) throw response.error
      return response.data as InvoiceListResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })
}

export function invoiceDetailQueryOptions(realmId: string, invoiceId: string) {
  return queryOptions({
    queryKey: invoiceKeys.detail(realmId, invoiceId),
    queryFn: async () => {
      const response = await getInvoice({
        path: { invoiceId },
      })
      if (response.error) throw response.error
      return response.data as InvoiceDetailResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })
}

export function sellerConfigQueryOptions(realmId: string) {
  return queryOptions({
    queryKey: invoiceKeys.sellerConfig(realmId),
    queryFn: async () => {
      const response = await getSellerConfig({})
      // 404 is expected when no config exists yet (first-time setup)
      if (response.error) {
        const status = (response.error as { status?: number }).status
        if (status === 404) return null
        throw response.error
      }
      return response.data as SellerConfigResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })
}

export function myInvoiceListQueryOptions(
  realmId: string,
  query?: {
    status?: string
    source?: string
    search?: string
    dateFrom?: string
    dateTo?: string
    page?: number
    pageSize?: number
    provider?: string
  }
) {
  return queryOptions({
    queryKey: invoiceKeys.myList(realmId, query),
    queryFn: async () => {
      const response = await listMyInvoices({ query })
      if (response.error) throw response.error
      return response.data as InvoiceListResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })
}

export function myInvoiceDetailQueryOptions(realmId: string, invoiceId: string) {
  return queryOptions({
    queryKey: invoiceKeys.myDetail(realmId, invoiceId),
    queryFn: async () => {
      const response = await getMyInvoiceScoped({ path: { invoiceId } })
      if (response.error) throw response.error
      return response.data as InvoiceDetailResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })
}

export function invoicePolicyConfigQueryOptions(realmId: string) {
  return queryOptions({
    queryKey: invoiceKeys.policyConfig(realmId),
    queryFn: async () => {
      const response = await listRealmConfigsByType({
        path: { configType: 'invoice_policy' },
      })
      if (response.error) {
        const status = (response.error as { status?: number }).status
        if (status === 404) return []
        throw response.error
      }
      return response.data as RealmConfigResponse[]
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })
}

/**
 * Per-resource invoice apply-eligibility. Used to gate the per-row Invoice
 * button on history lists BEFORE submit (P1-4). The query is enabled by the
 * caller (typically when `invoicesVisible` is true and the row is rendered).
 *
 * `referenceType` is snake_case `payment_attempt` (API contract), even though
 * the prefilled-reference form type uses camelCase `paymentAttempt`.
 */
export function invoiceApplyEligibilityQueryOptions(
  realmId: string,
  referenceType: 'payment_attempt' | 'subscription',
  referenceId: string
) {
  return queryOptions({
    queryKey: invoiceKeys.applyEligibility(realmId, referenceType, referenceId),
    queryFn: async () => {
      const response = await getInvoiceApplyEligibility({ query: { referenceType, referenceId } })
      if (response.error) throw response.error
      return response.data as InvoiceApplyEligibilityResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })
}
