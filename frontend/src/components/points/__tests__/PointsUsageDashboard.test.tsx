import { render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { PointsUsageDashboard } from '../PointsUsageDashboard'
import type { DerivedBucketCard } from '../user-points-view'
import type { QuotaWindowViewResponse, WalletByBucketResponse } from '@/lib/api-generated/types.gen'

/**
 * Factory for a single backend-precomputed quota window (design §4.2.2).
 * Centralised so each test reads as "a tightest monthly window with 70 left"
 * rather than a magic literal, and so the consumed field shape is asserted in
 * one place. The dashboard MUST consume these verbatim (no client recompute —
 * FE-T01 pins the pass-through intent one layer up).
 */
function makeWindow(
  overrides: Partial<QuotaWindowViewResponse> & { key: string }
): QuotaWindowViewResponse {
  return {
    limit: 100,
    used: 0,
    remaining: 100,
    windowSeconds: 30 * 24 * 60 * 60,
    isTightest: false,
    exhausted: false,
    resetsAt: null,
    ...overrides,
  }
}

/**
 * Factory for a DerivedBucketCard. `bucketTotal` is the BACKEND-computed
 * spendable total — the dashboard renders it directly as `points-spendable-now`
 * and does NOT recompute `spendableFromQuota + spendableFromPool` client-side.
 * Defaults reflect a healthy quota+pool bucket with one window.
 */
function makeCard(overrides: Partial<DerivedBucketCard> = {}): DerivedBucketCard {
  return {
    bucketId: 'bucket-a',
    name: 'Default',
    enabled: true,
    bucketTotal: 120,
    balancesByType: {
      freePeriodic: 0,
      granted: 0,
      registration: 0,
      subscription: 0,
      topup: 0,
    } satisfies WalletByBucketResponse['balancesByType'],
    quotaWindows: [makeWindow({ key: 'monthly', remaining: 70, isTightest: true })],
    spendableFromQuota: 70,
    spendableFromPool: 50,
    ...overrides,
  }
}

const BUCKET = 'bucket-a'

/**
 * PointsUsageDashboard is the read-only projection of the backend-precomputed
 * wallet/quota view (design §4.2.2 / §4.4.3). It exists to surface, per bucket,
 * how much is spendable RIGHT NOW, which window is the limiting factor, and
 * whether the user is at/over a window limit. Because `bucketTotal`,
 * `spendableFromQuota`, `spendableFromPool`, and every `isTightest`/`exhausted`
 * flag are authoritative backend outputs, the dashboard's contract is "render
 * the truth, flag the constraints, never recompute". These tests pin that
 * contract across every state a user can be in.
 */
describe('PointsUsageDashboard', () => {
  describe('loading state', () => {
    it('renders skeleton placeholders and the loading root testid (no bucketId suffix) when loading', () => {
      // INTENT: while the wallet query is in flight the user must see a stable
      // card outline, not a flash of empty/insufficient content that would
      // falsely signal "you have no balance". The loading root testid is
      // intentionally bucket-agnostic so the FE-T07 runner can target the
      // placeholder before any card data resolves.
      render(<PointsUsageDashboard card={makeCard()} loading />)

      expect(screen.getByTestId('points-usage-dashboard')).toBeInTheDocument()
      // Loading root must NOT carry a bucketId suffix — distinguishable from
      // the resolved root testid.
      expect(screen.queryByTestId(`points-usage-dashboard-${BUCKET}`)).not.toBeInTheDocument()
      // Skeletons render via the Card chrome — assert at least one is present
      // so a future removal surfaces here.
      expect(
        document.querySelectorAll('[class*="animate-pulse"], [class*="skeleton"]').length
      ).toBeGreaterThan(0)
    })
  })

  describe('empty state (pool-only with no balance at all)', () => {
    it('renders the muted empty-state alert when there are no windows and both quota and pool sides are 0', () => {
      // INTENT: a brand-new user with neither a subscription window nor any
      // pool balance must see the "subscribe / top up" empty-state so they
      // know how to get points. The component fires `empty` when there are
      // no windows AND both spendable sides are 0.
      //
      // The empty-state and insufficient guards are mutually exclusive: a
      // brand-new user with no windows and no balance sees the onboarding
      // empty-state, NOT the destructive "transaction rejected" insufficient
      // alert (there is no transaction in progress to reject). `insufficient`
      // is reserved for a bucket that HAD a balance model (windows present)
      // but whose total is now drained.
      render(
        <PointsUsageDashboard
          card={makeCard({
            bucketId: 'empty-bucket',
            bucketTotal: 0,
            quotaWindows: [],
            spendableFromQuota: 0,
            spendableFromPool: 0,
          })}
        />
      )

      expect(screen.getByTestId('points-empty-state')).toBeInTheDocument()
      // The insufficient alert must NOT render alongside the empty-state.
      expect(screen.queryByTestId('points-insufficient-alert')).not.toBeInTheDocument()
    })
  })

  describe('normal state (healthy quota + pool bucket)', () => {
    const monthly = makeWindow({
      key: 'monthly',
      limit: 100,
      used: 30,
      remaining: 70,
      isTightest: true,
    })
    const daily = makeWindow({ key: 'daily', limit: 20, used: 5, remaining: 15, isTightest: false })

    it('renders each window row keyed by bucketId+winKey, with progress bars; the spendable total and formula caption are no longer surfaced', () => {
      // INTENT: winKey is the backend's stable config-derived key (not a row
      // ordinal) so downstream Playwright runs and the FE-T07 runner can target
      // a window across reloads. The "current spendable total" big number and
      // the formula caption were intentionally removed from this card to keep it
      // compact — the per-window rows remain the authoritative usage view.
      render(<PointsUsageDashboard card={makeCard({ quotaWindows: [monthly, daily] })} />)

      // Root testid carries the bucketId once resolved.
      expect(screen.getByTestId(`points-usage-dashboard-${BUCKET}`)).toBeInTheDocument()

      // One row per window, addressed by stable backend key.
      expect(screen.getByTestId(`points-window-row-${BUCKET}-monthly`)).toBeInTheDocument()
      expect(screen.getByTestId(`points-window-row-${BUCKET}-daily`)).toBeInTheDocument()

      // Progress bars exist for each window.
      expect(screen.getByTestId(`points-window-bar-${BUCKET}-monthly`)).toBeInTheDocument()
      expect(screen.getByTestId(`points-window-bar-${BUCKET}-daily`)).toBeInTheDocument()

      // The spendable total big number and formula caption are removed (keep
      // the card compact). Pin their absence so a regression surfaces here.
      expect(screen.queryByTestId('points-spendable-now')).not.toBeInTheDocument()
      expect(screen.queryByTestId('points-spendable-formula')).not.toBeInTheDocument()
    })

    it('renders the progress bar fill as the backend remaining/limit ratio, capped at 100, via aria-valuenow', () => {
      // INTENT: the bar visualises how much of each window's limit is STILL
      // available (remaining/limit), NOT how much has been used. The dashboard
      // takes remaining & limit straight from the backend; the displayed fill
      // must reflect the backend's authoritative numbers. Asserting via the
      // progressbar role's aria-valuenow keeps the test robust to CSS-width
      // formatting changes while still pinning the computed ratio.
      render(<PointsUsageDashboard card={makeCard({ quotaWindows: [monthly] })} />)

      const bar = screen.getByTestId(`points-window-bar-${BUCKET}-monthly`)
      expect(bar).toHaveAttribute('aria-valuenow', '70') // 70/100
      expect(bar).toHaveAttribute('aria-valuemin', '0')
      expect(bar).toHaveAttribute('aria-valuemax', '100')
    })
  })

  describe('tightest-constraint surfacing', () => {
    it('renders the backend-flagged tightest window above a non-tightest one with larger remaining, and badges it as the active constraint', () => {
      // INTENT: the backend OWNS the tightest decision (it may weigh factors
      // beyond raw remaining). The dashboard sorts exhausted→tightest→limit
      // desc, so a tightest window must surface to the user even when another
      // window has a numerically smaller remaining — otherwise the user would
      // misread the limiting factor. We deliberately give a non-tightest
      // window the SMALLER remaining to assert no client-side min recompute.
      const tightest = makeWindow({ key: 'monthly', limit: 100, remaining: 70, isTightest: true })
      const notTightest = makeWindow({
        key: 'daily',
        limit: 100,
        remaining: 10, // numerically smaller, but backend did NOT flag tightest
        isTightest: false,
      })

      render(<PointsUsageDashboard card={makeCard({ quotaWindows: [tightest, notTightest] })} />)

      const rows = screen.getAllByTestId(/^points-window-row-/)
      // Tightest sorts ahead of non-tightest (exhausted→tightest→limit desc).
      expect(rows[0]).toHaveAttribute('data-testid', `points-window-row-${BUCKET}-monthly`)
      // The tightest window carries the active-constraint badge text.
      expect(within(rows[0]).getByText('Current active constraint')).toBeInTheDocument()
    })
  })

  describe('exhausted window', () => {
    it('renders the exhausted alert, danger styling on the row, and a 0 fill when a window has remaining===0', () => {
      // INTENT: an exhausted window means a hard quota wall. The user must
      // immediately see (a) a destructive alert that ANY window is exhausted,
      // (b) the offending row visually marked as dangerous, and (c) a zeroed
      // bar so it's unmistakable which window is the wall. spendableFromQuota
      // in the card is the backend's min-remaining — when the tightest window
      // is exhausted that is 0, and the dashboard shows the backend total as-is.
      const exhausted = makeWindow({
        key: 'monthly',
        limit: 100,
        used: 100,
        remaining: 0,
        isTightest: true,
        exhausted: true,
      })

      render(
        <PointsUsageDashboard
          card={makeCard({
            quotaWindows: [exhausted],
            spendableFromQuota: 0,
            spendableFromPool: 0,
            bucketTotal: 0,
          })}
        />
      )

      // Destructive alert for any exhausted window.
      expect(screen.getByTestId('points-window-exhausted-alert')).toBeInTheDocument()

      const row = screen.getByTestId(`points-window-row-${BUCKET}-monthly`)
      // Danger styling — the row border switches to destructive when exhausted.
      expect(row.className).toContain('destructive')

      // Bar fill is 0.
      const bar = screen.getByTestId(`points-window-bar-${BUCKET}-monthly`)
      expect(bar).toHaveAttribute('aria-valuenow', '0')

      // The spendable total big number was removed from this card; pin its
      // absence so the removal intent survives.
      expect(screen.queryByTestId('points-spendable-now')).not.toBeInTheDocument()
    })
  })

  describe('overspend topup alert', () => {
    it('renders the overspend-topup info alert when a window is exhausted but pool keeps the total positive', () => {
      // INTENT: when one window is exhausted the user could still spend via
      // pool topup — but that means quota no longer caps spend and overage
      // silently eats recharge balance. The dashboard must WARN about this
      // (so users aren't surprised when topup drains) rather than treating
      // "window exhausted" as a hard stop. Trigger: anyWindowExhausted AND
      // spendableFromPool > 0 AND bucketTotal > 0.
      const exhausted = makeWindow({
        key: 'monthly',
        limit: 100,
        used: 100,
        remaining: 0,
        isTightest: true,
        exhausted: true,
      })

      render(
        <PointsUsageDashboard
          card={makeCard({
            quotaWindows: [exhausted],
            spendableFromQuota: 0,
            spendableFromPool: 50,
            bucketTotal: 50, // pool keeps total > 0 despite exhausted window
          })}
        />
      )

      expect(screen.getByTestId('points-window-exhausted-alert')).toBeInTheDocument()
      expect(screen.getByTestId('points-overspend-topup-alert')).toBeInTheDocument()
      // When overspend-topup fires, insufficient must NOT (total > 0).
      expect(screen.queryByTestId('points-insufficient-alert')).not.toBeInTheDocument()
    })
  })

  describe('insufficient alert', () => {
    it('renders the insufficient danger alert when the backend total is <=0 and no window is exhausted', () => {
      // INTENT: insufficient means "you had points but the whole bucket is
      // drained" — the system will REJECT the transaction wholesale (no
      // partial deduction). This differs from exhausted (a single window
      // wall, possibly still covered by pool) and from empty (never had
      // points). Trigger pinned to the REAL component behavior:
      // spendableTotal <= 0 AND no window exhausted. The drained-pool-only
      // case (no windows, pool 0) routes to the empty-state branch FIRST
      // because empty also requires no windows — so to assert insufficient
      // in isolation we need at least one (non-exhausted) window present.
      const liveWindow = makeWindow({
        key: 'monthly',
        limit: 100,
        used: 100,
        remaining: 0,
        exhausted: false, // remaining 0 but backend hasn't flagged exhausted
      })

      render(
        <PointsUsageDashboard
          card={makeCard({
            quotaWindows: [liveWindow],
            spendableFromQuota: 0,
            spendableFromPool: 0,
            bucketTotal: 0,
          })}
        />
      )

      expect(screen.getByTestId('points-insufficient-alert')).toBeInTheDocument()
      // No exhausted window -> no exhausted alert, no overspend topup.
      expect(screen.queryByTestId('points-window-exhausted-alert')).not.toBeInTheDocument()
      expect(screen.queryByTestId('points-overspend-topup-alert')).not.toBeInTheDocument()
    })
  })

  describe('sort order across mixed windows', () => {
    it('orders windows exhausted -> tightest -> limit desc, independent of array order', () => {
      // INTENT: the user's eye should land on the most constraining window
      // first. The fixed sort (exhausted, then tightest, then larger limit
      // first) makes the wall/limiting factor visually dominant regardless of
      // how the backend happened to order windows in the array.
      const bigLimit = makeWindow({ key: 'big', limit: 500, remaining: 500, isTightest: false })
      const tightest = makeWindow({ key: 'tight', limit: 100, remaining: 40, isTightest: true })
      const exhausted = makeWindow({
        key: 'wall',
        limit: 50,
        used: 50,
        remaining: 0,
        exhausted: true,
      })

      render(
        <PointsUsageDashboard
          card={makeCard({
            quotaWindows: [bigLimit, tightest, exhausted],
          })}
        />
      )

      const rows = screen.getAllByTestId(/^points-window-row-/)
      expect(rows.map((r) => r.getAttribute('data-testid'))).toEqual([
        `points-window-row-${BUCKET}-wall`, // exhausted first
        `points-window-row-${BUCKET}-tight`, // tightest next
        `points-window-row-${BUCKET}-big`, // then limit desc
      ])
    })
  })
})
