import type { WebhookSubscriptionItem } from '@/lib/api-generated'

/**
 * Defaults describe an active subscription on the public example callback
 * (the shape every webhook list/form test composes from); `overrides`
 * mutates any field, most notably `status` for the three-state mapping tests.
 */
export function makeWebhookSubscription(
  overrides: Partial<WebhookSubscriptionItem> = {}
): WebhookSubscriptionItem {
  return {
    id: 'sub-uuid-1',
    realmId: 'test-realm',
    endpointUrl: 'https://example.com/webhooks/herald',
    eventCategories: ['role_events'],
    status: 'active',
    consecutiveFailures: 0,
    lastError: null,
    pausedAt: null,
    secretHint: '…wxyz',
    createdAt: '2026-11-20T12:00:00Z',
    updatedAt: '2026-11-20T12:00:00Z',
    ...overrides,
  }
}
