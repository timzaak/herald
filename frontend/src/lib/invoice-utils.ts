import type { InvoiceSource, InvoiceStatus, PaymentAttemptStatus } from '@/lib/api-generated'
import type { InvoiceLineItemFormData } from '@/lib/schemas/invoice-forms'
import { m } from '@/paraglide/messages'

export const INVOICE_PAGE_SIZE = 20

export const PDF_DOWNLOADABLE_STATUSES = new Set(['issued', 'paid', 'overdue'])

const INVOICE_STATUS_LABELS: Record<InvoiceStatus, string> = {
  draft: m['billing.invoice_status_draft'](),
  issued: m['billing.invoice_status_issued'](),
  paid: m['billing.invoice_status_paid'](),
  void: m['billing.invoice_status_void'](),
  overdue: m['billing.invoice_status_overdue'](),
}

export function getInvoiceStatusLabel(status: InvoiceStatus): string {
  return INVOICE_STATUS_LABELS[status]
}

export const INVOICE_STATUS_COLORS: Record<InvoiceStatus, string> = {
  draft: 'secondary',
  issued: 'default',
  paid: 'default',
  void: 'destructive',
  overdue: 'destructive',
}

const INVOICE_SOURCE_LABELS: Record<InvoiceSource, string> = {
  admin_manual: m['billing.invoice_source_manual'](),
  user_application: m['billing.invoice_source_application'](),
  external_sync: m['billing.invoice_source_external'](),
}

export function getInvoiceSourceLabel(source: InvoiceSource): string {
  return INVOICE_SOURCE_LABELS[source]
}

export function formatInvoiceAmount(amount: number, currency: string): string {
  const value = amount / 100
  try {
    return new Intl.NumberFormat(undefined, {
      style: 'currency',
      currency,
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(value)
  } catch {
    return `${currency.toUpperCase()} ${value.toFixed(2)}`
  }
}

export function displayPriceToCents(value: string): number {
  return Math.round(parseFloat(value) * 100)
}

export function centsToDisplayPrice(cents: number): string {
  return (cents / 100).toFixed(2)
}

export function getPaymentStatusBadgeVariant(
  status: PaymentAttemptStatus
): 'default' | 'secondary' | 'destructive' | 'outline' {
  switch (status) {
    case 'Succeeded':
      return 'default'
    case 'Pending':
    case 'RequiresAction':
      return 'secondary'
    case 'Failed':
      return 'destructive'
    case 'Cancelled':
    case 'Expired':
      return 'outline'
  }
}

/**
 * Extract display price from providerProductInfo for both Stripe and Creem.
 * Handles both string and number price fields.
 * Returns null if price cannot be determined.
 */
export function extractProviderPrice(
  providerProductInfo: unknown
): { amount: number; currency: string } | null {
  if (!providerProductInfo || typeof providerProductInfo !== 'object') return null
  const info = providerProductInfo as Record<string, unknown>

  // Stripe: { price_data: { unit_amount, currency } }
  if (info.price_data && typeof info.price_data === 'object') {
    const priceData = info.price_data as Record<string, unknown>
    if (typeof priceData.unit_amount === 'number' && typeof priceData.currency === 'string') {
      return { amount: priceData.unit_amount, currency: priceData.currency }
    }
  }

  // Creem / generic: { price, currency }
  // Creem may send price as a string like "9.99"
  if (typeof info.currency === 'string') {
    let amount: number | undefined
    if (typeof info.price === 'number') {
      amount = info.price
    } else if (typeof info.price === 'string') {
      const parsed = parseFloat(info.price)
      if (!isNaN(parsed)) {
        amount = parsed
      }
    }
    if (amount !== undefined) {
      // If amount looks like a display value (e.g. 9.99), convert to cents
      return { amount: Math.round(amount * 100), currency: info.currency }
    }
  }

  return null
}

export function downloadInvoicePdf(url: string, filename: string) {
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  document.body.appendChild(link)
  try {
    link.click()
  } finally {
    document.body.removeChild(link)
  }
}

export type InvoiceAction = 'view' | 'edit' | 'issue' | 'void' | 'markPaid' | 'downloadPdf'

const ACTION_RULES: Record<string, InvoiceAction[]> = {
  draft: ['view', 'edit', 'issue', 'void'],
  issued: ['view', 'void', 'markPaid', 'downloadPdf'],
  paid: ['view', 'downloadPdf'],
  overdue: ['view', 'void', 'markPaid', 'downloadPdf'],
  void: ['view'],
}

export function isExternalInvoice(provider: string | undefined): boolean {
  return !!provider && provider !== 'manual'
}

export function getProviderLabel(provider: string): string {
  switch (provider) {
    case 'manual':
      return m['billing.invoice_provider_manual']()
    case 'stripe':
      return 'Stripe'
    case 'creem':
      return 'Creem'
    default:
      return provider
  }
}

export function getViewInProviderUrl(invoice: {
  provider: string
  externalHostedUrl?: string | null
}): string | null {
  if (invoice.provider === 'manual') return null
  return invoice.externalHostedUrl ?? null
}

export function getAvailableActions(status: string, provider?: string): InvoiceAction[] {
  if (provider && provider !== 'manual') return ['view']
  return ACTION_RULES[status] ?? ['view']
}

/**
 * Parse payment terms string into number of days.
 * "Net 30" → 30, "Net 60" → 60, "Due on Receipt" → 0
 * Returns undefined if parsing fails.
 */
export function parsePaymentTermsDays(terms: string | null | undefined): number | undefined {
  if (!terms) return undefined
  const netMatch = /Net (\d+)/i.exec(terms)
  if (netMatch) return parseInt(netMatch[1], 10)
  if (/Due on Receipt/i.test(terms)) return 0
  return undefined
}

export function calculateLineSubtotal(quantity: string, unitPrice: string): number {
  const qty = parseFloat(quantity)
  const price = parseFloat(unitPrice)
  if (isNaN(qty) || qty <= 0 || isNaN(price)) return 0
  return Math.round(qty * price * 100)
}

interface ModifierInput {
  discountMode?: string | null
  discountValue?: string | null
  taxMode?: string | null
  taxValue?: string | null
  shippingMode?: string | null
  shippingValue?: string | null
}

interface CalculatedTotals {
  subtotal: number
  discountAmount: number
  taxAmount: number
  shippingAmount: number
  total: number
}

/**
 * Gate rendering of refund-related sections (breakdown, Credit Note list,
 * refund chips/pills). Creem never renders refund dimensions; other providers
 * only render when there is a non-zero refund amount.
 */
export function shouldRenderRefundDimension(provider: string, amountRefunded: number): boolean {
  return amountRefunded > 0 && (provider === 'stripe' || provider === 'manual')
}

/**
 * Detect cumulative refund exceeding invoice total. This is an abnormal state
 * that should only occur on the Stripe side (Manual creation rejects over-remaining).
 */
export function isRefundOverTotal(amountRefunded: number, total: number): boolean {
  return amountRefunded > total
}

/**
 * Source-aware color tokens for Credit Note badges and track top-bars.
 *
 * Manual and Stripe both map to the info token (matching the external-provider
 * badge in `invoice-admin-page.tsx`); the semantic palette has no second
 * provider hue, so the two sources are not distinguished by color. Unknown
 * providers fall back to muted.
 */
export function getCreditNoteSourceColorClass(source: string): {
  badge: string
  bar: string
} {
  switch (source) {
    case 'manual':
      return {
        badge: 'bg-info/10 text-info border-info/20',
        bar: 'border-t-info',
      }
    case 'stripe':
      return {
        badge: 'bg-info/10 text-info border-info/20',
        bar: 'border-t-info',
      }
    default:
      return {
        badge: 'bg-muted text-muted-foreground border-border',
        bar: 'border-t-border',
      }
  }
}

/**
 * Format a refund summary pill such as "Refunded $50.00/$100.00".
 * The label is passed by callers (typically an i18n message factory) so this
 * helper stays pure and presentation-only.
 */
export function formatRefundSummaryPill(
  amountRefunded: number,
  total: number,
  currency: string,
  label: string
): string {
  return `${label} ${formatInvoiceAmount(amountRefunded, currency)}/${formatInvoiceAmount(total, currency)}`
}

export function calculateTotals(
  lineItems: Pick<InvoiceLineItemFormData, 'quantity' | 'unitPrice'>[],
  modifiers: ModifierInput
): CalculatedTotals {
  const {
    discountMode: dm,
    discountValue: dv,
    taxMode: tm,
    taxValue: tv,
    shippingMode: sm,
    shippingValue: sv,
  } = modifiers

  const subtotal = lineItems.reduce(
    (sum, item) => sum + calculateLineSubtotal(item.quantity, item.unitPrice),
    0
  )

  let discountAmount = 0
  if (dm && dv) {
    const parsed = parseFloat(dv)
    if (!isNaN(parsed) && parsed >= 0) {
      if (dm === 'fixed') {
        discountAmount = Math.round(parsed * 100)
      } else if (dm === 'percent') {
        discountAmount = Math.round((subtotal * parsed) / 100)
      }
    }
  }

  let taxAmount = 0
  const afterDiscount = subtotal - discountAmount
  if (tm && tv) {
    const parsed = parseFloat(tv)
    if (!isNaN(parsed) && parsed >= 0) {
      if (tm === 'fixed') {
        taxAmount = Math.round(parsed * 100)
      } else if (tm === 'percent') {
        taxAmount = Math.round((afterDiscount * parsed) / 100)
      }
    }
  }

  let shippingAmount = 0
  if (sm && sv) {
    const parsed = parseFloat(sv)
    if (!isNaN(parsed) && parsed >= 0) {
      shippingAmount = Math.round(parsed * 100)
    }
  }

  const total = Math.max(0, afterDiscount + taxAmount + shippingAmount)

  return { subtotal, discountAmount, taxAmount, shippingAmount, total }
}
