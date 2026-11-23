import { m } from '@/paraglide/messages'

// The generated type types status as a bare string (doc comment: `active` /
// `paused` (system) / `disabled`), so unknown values fall back to the raw
// status instead of drifting between badge and toast mappings.
const WEBHOOK_STATUS_LABELS: Record<string, () => string> = {
  active: () => m['webhooks.status_active'](),
  paused: () => m['webhooks.status_paused'](),
  disabled: () => m['webhooks.status_disabled'](),
}

/** Shared by the status badge and the status-change toast. */
export function getWebhookStatusLabel(status: string): string {
  return WEBHOOK_STATUS_LABELS[status]?.() ?? status
}
