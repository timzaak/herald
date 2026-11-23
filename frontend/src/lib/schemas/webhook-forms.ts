import { z } from 'zod'
import { m } from '@/paraglide/messages'

export const WEBHOOK_EVENT_CATEGORIES = [
  'user_lifecycle',
  'role_events',
  'subscription_lifecycle',
] as const

export type WebhookEventCategory = (typeof WEBHOOK_EVENT_CATEGORIES)[number]

// Single source for category display metadata; consumers (table badges, form
// checkboxes) key by the raw category string so unknown backend values still
// render via their `?? category` fallbacks.
export const WEBHOOK_CATEGORY_LABEL: Record<string, () => string> = {
  user_lifecycle: () => m['webhooks.category_user_lifecycle'](),
  role_events: () => m['webhooks.category_role_events'](),
  subscription_lifecycle: () => m['webhooks.category_subscription_lifecycle'](),
}

export const WEBHOOK_CATEGORY_DESCRIPTION: Record<string, () => string> = {
  user_lifecycle: () => m['webhooks.category_user_lifecycle_description'](),
  role_events: () => m['webhooks.category_role_events_description'](),
  subscription_lifecycle: () => m['webhooks.category_subscription_lifecycle_description'](),
}

// IPv4 ranges that must never be registered as callback targets. Mirrors the
// backend's registration-time rejection list; the frontend check is an
// early-reject optimization only — the server stays authoritative (a hostname
// resolving into a private IP is invisible here and caught by the 400).
const PRIVATE_IPV4_RANGES: Array<{ prefix: number; bits: number }> = [
  { prefix: 0x00000000, bits: 8 }, // 0.0.0.0/8
  { prefix: 0x0a000000, bits: 8 }, // 10.0.0.0/8
  { prefix: 0x64400000, bits: 10 }, // 100.64.0.0/10
  { prefix: 0x7f000000, bits: 8 }, // 127.0.0.0/8
  { prefix: 0xa9fe0000, bits: 16 }, // 169.254.0.0/16
  { prefix: 0xac100000, bits: 12 }, // 172.16.0.0/12
  { prefix: 0xc0a80000, bits: 16 }, // 192.168.0.0/16
  { prefix: 0xc6120000, bits: 15 }, // 198.18.0.0/15
  { prefix: 0xe0000000, bits: 4 }, // 224.0.0.0/4
  { prefix: 0xf0000000, bits: 4 }, // 240.0.0.0/4
]

function ipv4ToNumber(host: string): number | null {
  const parts = host.split('.')
  if (parts.length !== 4) return null
  let value = 0
  for (const part of parts) {
    if (!/^\d{1,3}$/.test(part)) return null
    const octet = Number(part)
    if (octet > 255) return null
    value = (value << 8) | octet
  }
  return value >>> 0
}

function isPrivateIpv4(host: string): boolean {
  const value = ipv4ToNumber(host)
  if (value === null) return false
  return PRIVATE_IPV4_RANGES.some(({ prefix, bits }) => {
    const mask = (0xffffffff << (32 - bits)) >>> 0
    return (value & mask) === (prefix & mask)
  })
}

// Handles bracketed IPv6 (as `new URL().hostname` yields it), the `::1`
// loopback, unique-local fc00::/7, link-local fe80::/10, and IPv4-mapped
// forms (the URL parser normalizes `::ffff:192.0.2.1` to hex hextets).
function isPrivateIpv6(hostname: string): boolean {
  let host = hostname
  if (host.startsWith('[') && host.endsWith(']')) host = host.slice(1, -1)
  const zoneIndex = host.indexOf('%')
  if (zoneIndex !== -1) host = host.slice(0, zoneIndex)
  if (host === '::1' || host === '::') return true

  const mapped = host.match(/^::ffff:(.+)$/)
  if (mapped) {
    const groups = mapped[1].split(':')
    if (groups.length === 2) {
      const high = parseInt(groups[0], 16)
      const low = parseInt(groups[1], 16)
      if (Number.isNaN(high) || Number.isNaN(low)) return false
      const ipv4 = `${high >> 8}.${high & 0xff}.${low >> 8}.${low & 0xff}`
      return isPrivateIpv4(ipv4)
    }
    return false
  }

  const hextet = host.split(':').filter(Boolean)[0]
  if (hextet) {
    const first = parseInt(hextet, 16)
    if (!Number.isNaN(first)) {
      // fc00::/7 → 0xfc00..0xfdff, fe80::/10 → 0xfe80..0xfebf, ff00::/8 multicast
      if ((first & 0xfe00) === 0xfc00) return true
      if ((first & 0xffc0) === 0xfe80) return true
      if ((first & 0xff00) === 0xff00) return true
    }
  }
  return false
}

function isPublicCallbackHost(hostname: string): boolean {
  const host = hostname.toLowerCase()
  if (host === 'localhost' || host.endsWith('.localhost')) return false
  if (host.includes(':')) return !isPrivateIpv6(host)
  return !isPrivateIpv4(host)
}

export const webhookEndpointUrlSchema = z
  .string()
  .trim()
  .min(1, { error: () => m['webhooks.validation_endpoint_url_public_only']() })
  .max(2048, { error: () => m['webhooks.validation_endpoint_url_public_only']() })
  .refine(
    (value) => {
      try {
        const url = new URL(value)
        if (url.protocol !== 'https:' && url.protocol !== 'http:') return false
        if (url.username !== '' || url.password !== '') return false
        if (url.hash !== '') return false
        return isPublicCallbackHost(url.hostname)
      } catch {
        return false
      }
    },
    { error: () => m['webhooks.validation_endpoint_url_public_only']() }
  )

export const webhookEventCategoriesSchema = z
  .array(z.enum(WEBHOOK_EVENT_CATEGORIES))
  .min(1, { error: () => m['webhooks.validation_categories_required']() })

export const webhookSubscriptionSchema = z.object({
  endpointUrl: webhookEndpointUrlSchema,
  eventCategories: webhookEventCategoriesSchema,
})

export type WebhookSubscriptionFormData = z.infer<typeof webhookSubscriptionSchema>
