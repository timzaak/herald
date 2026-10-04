import type { WalletByBucketResponse } from '@/lib/api-generated'
import { formatDate } from '@/lib/date-utils'
import { m } from '@/paraglide/messages'

/**
 * Derived bucket card for the user points view.
 *
 * Mirrors `WalletByBucketResponse` fields that the UI consumes, typed against
 * the generated contract so contract changes surface at compile time.
 */
export interface DerivedBucketCard {
  bucketId: string
  name: string | null
  enabled: boolean | null
  bucketTotal: number
  balancesByType: WalletByBucketResponse['balancesByType']
  /**
   * Per-window quota view for this (user, bucket) (design §4.2.2). `null`/`undefined`
   * for a pool-only bucket. Passed through verbatim from the backend — the dashboard
   * (FE-D03) consumes `key`/`limit`/`used`/`remaining`/`windowSeconds`/`resetsAt`/
   * `isTightest`/`exhausted` directly.
   */
  quotaWindows: WalletByBucketResponse['quotaWindows']
  /**
   * Window-quota available = minimum `remaining` across `quotaWindows`. `null`/`undefined`
   * for pool-only buckets. Pass-through; `bucketTotal` is the backend-computed total.
   */
  spendableFromQuota: WalletByBucketResponse['spendableFromQuota']
  /**
   * Pool-side balance sum (topup + registration + granted) for this bucket.
   * `null`/`undefined` for window-only buckets. Pass-through.
   */
  spendableFromPool: WalletByBucketResponse['spendableFromPool']
  /**
   * Earliest expiry across this bucket's available pool rows
   * (`MIN(expires_at)` over the availability predicate, points.md §4.1).
   * `null`/`undefined` = permanent-only pool (长期有效). Pass-through.
   */
  expiresAt: WalletByBucketResponse['expiresAt']
}

export interface DerivedUserPointsView {
  /**
   * `true` only when the current user holds >= 2 buckets. The cross-bucket
   * total bar renders conditionally on this flag.
   */
  showTotalBar: boolean
  cards: DerivedBucketCard[]
  /**
   * Sum of the current user's `bucketTotal` across their filtered buckets.
   * Recomputed client-side — NOT the realm-wide `crossBucketTotal` returned
   * by `listWallets` (which spans all realm users).
   */
  crossBucketTotal: number
}

/**
 * Pure derivation of the user-facing points view from the realm-wide
 * `listWallets` response items.
 *
 * LOUD DEVIATION (query-options.ts): the `listWallets` endpoint is
 * realm-wide and `points.view`-gated with no `userId` filter, so `items`
 * contains wallet rows for EVERY user in the realm. This function isolates
 * the calling user's rows, recomputes their cross-bucket total, and decides
 * whether the cross-bucket total bar should render.
 *
 * Framework-agnostic and side-effect-free so it can be unit-tested — the
 * >=2 / =1 / =0 branches directly.
 */
export function deriveUserPointsView(
  items: WalletByBucketResponse[],
  currentUserId: string
): DerivedUserPointsView {
  const cards = items
    .filter((item) => item.userId === currentUserId)
    .map((item) => ({
      bucketId: item.bucketId ?? '',
      name: item.name ?? null,
      enabled: item.enabled ?? null,
      bucketTotal: item.bucketTotal,
      balancesByType: item.balancesByType,
      quotaWindows: item.quotaWindows,
      spendableFromQuota: item.spendableFromQuota,
      spendableFromPool: item.spendableFromPool,
      expiresAt: item.expiresAt ?? null,
    }))

  const crossBucketTotal = cards.reduce((sum, card) => sum + card.bucketTotal, 0)

  return {
    showTotalBar: cards.length >= 2,
    cards,
    crossBucketTotal,
  }
}

/**
 * Display rule for the earliest pool expiry (points.md §4.1), shared by the
 * user-side balance card and the admin wallets row: the formatted
 * `MIN(expires_at)` date, or the 长期有效 label for permanent-only pools.
 */
export function formatPoolExpiry(expiresAt: WalletByBucketResponse['expiresAt']): string {
  return expiresAt
    ? m['points.expires_at_label']({ date: formatDate(expiresAt) })
    : m['points.expires_at_never']()
}
