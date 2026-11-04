/**
 * Billing and Subscription Types
 *
 * Type definitions for billing-related functionality including subscription history.
 */
import { m } from '@/paraglide/messages'

// Subscription history event types
export type SubscriptionHistoryEventType =
  | 'created'
  | 'upgraded'
  | 'downgraded'
  | 'canceled'
  | 'expired'
  | 'renewed'
  | 'reactivated'
  | 'billing_period_changed'
  | 'past_due'
  | 'disputed'
  | 'payment_succeeded'
  | 'payment_failed'
  | 'invoice_created'

// Re-exported from the generated OpenAPI types so the vocabulary cannot drift from the backend enum.
export type { SubscriptionStatus } from '@/lib/api-generated'
import type { SubscriptionStatus } from '@/lib/api-generated'

// Subscription history event
export interface SubscriptionHistoryEvent {
  id: string
  subscriptionId: string
  eventType: SubscriptionHistoryEventType
  timestamp: string
  actor?: string
  changes?: SubscriptionChanges
  previousState?: SubscriptionState
  newState?: SubscriptionState
}

// Change details for subscription modifications
export interface SubscriptionChanges {
  changedFields?: string[]
  previousEntitlementKey?: string
  newEntitlementKey?: string
  previousStatus?: string
  newStatus?: string
}

// Subscription state snapshot
export interface SubscriptionState {
  id: string
  realmId: string
  status: SubscriptionStatus
  entitlementKey: string
  paymentProvider?: string
  externalPriceId?: string
  clientAppId?: string
  currentPeriodStart?: string
  currentPeriodEnd?: string
  cancelAtPeriodEnd: boolean
  cancelAt?: string
}

// User information in subscription history
export interface UserInfo {
  id: string
  email: string
}

// `status` may be absent: history outlives its subscription row.
export interface SubscriptionSummary {
  id: string
  status?: SubscriptionStatus
  entitlementKey?: string
}

// Subscription history event with user and subscription details
export interface SubscriptionHistoryEventWithUser extends SubscriptionHistoryEvent {
  user?: UserInfo
  subscription: SubscriptionSummary
}

// Filter conditions for history queries
export interface HistoryFilters {
  userId?: string
  entitlementKey?: string
  eventType?: SubscriptionHistoryEventType
  subscriptionStatus?: SubscriptionStatus
  fromDate?: string
  toDate?: string
  sortBy?: string
  sortOrder?: 'asc' | 'desc'
}

// Pagination information
export interface PaginationInfo {
  page: number
  pageSize: number
  totalCount: number
  totalPages: number
}

// Single subscription history response
export interface SingleSubscriptionHistoryResponse {
  subscriptionId: string
  events: SubscriptionHistoryEvent[]
  total: number
}

// Global subscription history response
export interface GlobalSubscriptionHistoryResponse {
  events: SubscriptionHistoryEventWithUser[]
  pagination: PaginationInfo
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const messages = m as any

const eventTypeKeyMap: Record<SubscriptionHistoryEventType, string> = {
  created: 'billing.subscription_event_created',
  upgraded: 'billing.subscription_event_upgraded',
  downgraded: 'billing.subscription_event_downgraded',
  canceled: 'billing.subscription_event_canceled',
  expired: 'billing.subscription_event_expired',
  renewed: 'billing.subscription_event_renewed',
  reactivated: 'billing.subscription_event_reactivated',
  billing_period_changed: 'billing.subscription_event_billing_period_changed',
  past_due: 'billing.subscription_event_past_due',
  disputed: 'billing.subscription_event_disputed',
  payment_succeeded: 'billing.subscription_event_payment_succeeded',
  payment_failed: 'billing.subscription_event_payment_failed',
  invoice_created: 'billing.subscription_event_invoice_created',
}

export function getEventTypeLabel(type: SubscriptionHistoryEventType): string {
  return messages[eventTypeKeyMap[type]]()
}

export function getEventTypeLabels(): Readonly<Record<SubscriptionHistoryEventType, string>> {
  return Object.fromEntries(
    (Object.keys(eventTypeKeyMap) as SubscriptionHistoryEventType[]).map((k) => [
      k,
      messages[eventTypeKeyMap[k]](),
    ])
  ) as Readonly<Record<SubscriptionHistoryEventType, string>>
}

const statusKeyMap: Record<SubscriptionStatus, string> = {
  active: 'billing.subscription_status_label_active',
  pending: 'billing.subscription_status_label_pending',
  trialing: 'billing.subscription_status_label_trialing',
  past_due: 'billing.subscription_status_label_past_due',
  canceled: 'billing.subscription_status_label_canceled',
  incomplete: 'billing.subscription_status_label_incomplete',
  expired: 'billing.subscription_status_label_expired',
  paused: 'billing.subscription_status_label_paused',
  dispute: 'billing.subscription_status_label_disputed',
  scheduled_cancel: 'billing.subscription_status_label_scheduled_cancel',
}

export function getSubscriptionStatusLabel(status: SubscriptionStatus): string {
  return messages[statusKeyMap[status]]()
}

export function getSubscriptionStatusLabels(): Readonly<Record<SubscriptionStatus, string>> {
  return Object.fromEntries(
    (Object.keys(statusKeyMap) as SubscriptionStatus[]).map((k) => [k, messages[statusKeyMap[k]]()])
  ) as Readonly<Record<SubscriptionStatus, string>>
}

/**
 * Returns the badge variant for a subscription status
 * @param status - The subscription status
 * @returns The badge variant ('default', 'secondary', or 'destructive')
 */
export function getStatusBadgeVariant(
  status: SubscriptionStatus
): 'default' | 'secondary' | 'destructive' {
  switch (status) {
    case 'active':
    case 'trialing':
      return 'default'
    case 'past_due':
    case 'dispute':
    case 'paused':
      return 'destructive'
    default:
      return 'secondary'
  }
}

/**
 * Returns a user-friendly message for a subscription status
 * @param status - The subscription status
 * @returns The status message or empty string if no message
 */
const statusMessageKeyMap: Partial<Record<SubscriptionStatus, string>> = {
  past_due: 'billing.subscription_status_message_past_due',
  dispute: 'billing.subscription_status_message_disputed',
  scheduled_cancel: 'billing.subscription_status_message_scheduled_cancel',
  paused: 'billing.subscription_status_message_paused',
}

export function getStatusMessage(status: SubscriptionStatus): string {
  const key = statusMessageKeyMap[status]
  return key ? messages[key]() : ''
}
