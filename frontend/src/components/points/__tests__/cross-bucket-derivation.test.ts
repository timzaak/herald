/**
 * @vitest-environment jsdom
 */

import { describe, expect, it } from 'vitest'
import type { WalletByBucketResponse } from '@/lib/api-generated'
import { deriveUserPointsView } from '@/components/points/user-points-view'

/**
 * Factory for a single wallet row. After the Gap #2 fix the `listWallets`
 * endpoint is server-scoped to the caller, but `deriveUserPointsView` still
 * narrows defensively by `currentUserId`; these tests feed mixed-user inputs
 * directly to keep that narrowing covered.
 */
function makeWalletByBucket(
  overrides: Partial<WalletByBucketResponse> & { userId: string }
): WalletByBucketResponse {
  return {
    bucketId: 'bucket-a',
    name: 'Default',
    enabled: true,
    bucketTotal: 0,
    balancesByType: {
      freePeriodic: 0,
      granted: 0,
      registration: 0,
      subscription: 0,
      topup: 0,
    },
    ...overrides,
  }
}

const CURRENT_USER = 'user-self'
const OTHER_USER = 'user-other'

describe('deriveUserPointsView', () => {
  describe('currentUserId narrowing of the realm-wide response', () => {
    it('drops other users wallet rows and keeps only the calling user buckets', () => {
      const items: WalletByBucketResponse[] = [
        makeWalletByBucket({ userId: CURRENT_USER, bucketId: 'a', bucketTotal: 10 }),
        makeWalletByBucket({ userId: OTHER_USER, bucketId: 'a', bucketTotal: 999 }),
        makeWalletByBucket({ userId: CURRENT_USER, bucketId: 'b', bucketTotal: 20 }),
        makeWalletByBucket({ userId: OTHER_USER, bucketId: 'b', bucketTotal: 888 }),
      ]

      const result = deriveUserPointsView(items, CURRENT_USER)

      expect(result.cards).toHaveLength(2)
      // Cross-bucket total is recomputed from the FILTERED rows only, so the
      // other users' 999 + 888 must NOT leak into the calling user total.
      expect(result.crossBucketTotal).toBe(30)
    })

    it('returns no cards and a zero total when the user holds no buckets', () => {
      const items: WalletByBucketResponse[] = [
        makeWalletByBucket({ userId: OTHER_USER, bucketId: 'a', bucketTotal: 999 }),
      ]

      const result = deriveUserPointsView(items, CURRENT_USER)

      expect(result.cards).toHaveLength(0)
      expect(result.crossBucketTotal).toBe(0)
    })
  })

  describe('showTotalBar threshold branches', () => {
    it.each([
      // [bucketCountForCurrentUser, expectedShowTotalBar, expectedCardCount]
      [0, false, 0],
      [1, false, 1],
      [2, true, 2],
      [3, true, 3],
    ] as const)(
      'with %i buckets for the current user -> showTotalBar=%s, cards=%i',
      (bucketCount, expectedShowTotalBar, expectedCardCount) => {
        const items: WalletByBucketResponse[] = Array.from({ length: bucketCount }, (_, i) =>
          makeWalletByBucket({
            userId: CURRENT_USER,
            bucketId: `bucket-${i}`,
            bucketTotal: 5,
          })
        )

        const result = deriveUserPointsView(items, CURRENT_USER)

        expect(result.showTotalBar).toBe(expectedShowTotalBar)
        expect(result.cards).toHaveLength(expectedCardCount)
      }
    )
  })

  describe('crossBucketTotal derivation', () => {
    it('sums bucketTotal across the current user filtered rows only', () => {
      const items: WalletByBucketResponse[] = [
        makeWalletByBucket({ userId: CURRENT_USER, bucketId: 'a', bucketTotal: 12 }),
        makeWalletByBucket({ userId: CURRENT_USER, bucketId: 'b', bucketTotal: 8 }),
        makeWalletByBucket({ userId: OTHER_USER, bucketId: 'c', bucketTotal: 1000 }),
      ]

      const result = deriveUserPointsView(items, CURRENT_USER)

      // The function recomputes the total from the (userId-narrowed) rows, so it
      // is unaffected by whatever total the response might carry.
      expect(result.crossBucketTotal).toBe(20)
      expect(result.showTotalBar).toBe(true)
    })

    it('still counts disabled buckets toward the total (design: residual retained)', () => {
      // The filter narrows only on userId; `enabled` does NOT gate inclusion.
      // A disabled bucket with residual balance must still contribute its
      // bucketTotal to crossBucketTotal and still appear as a card.
      const items: WalletByBucketResponse[] = [
        makeWalletByBucket({
          userId: CURRENT_USER,
          bucketId: 'enabled-a',
          enabled: true,
          bucketTotal: 15,
        }),
        makeWalletByBucket({
          userId: CURRENT_USER,
          bucketId: 'disabled-b',
          enabled: false,
          bucketTotal: 7,
        }),
      ]

      const result = deriveUserPointsView(items, CURRENT_USER)

      expect(result.cards).toHaveLength(2)
      expect(result.cards.find((c) => c.bucketId === 'disabled-b')?.enabled).toBe(false)
      expect(result.crossBucketTotal).toBe(22)
      expect(result.showTotalBar).toBe(true)
    })
  })
})
