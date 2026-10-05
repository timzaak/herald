import { useCallback, useEffect, useRef } from 'react'
import { driver, type Driver } from 'driver.js'
import 'driver.js/dist/driver.css'
import './console-tour.css'
import { m } from '@/paraglide/messages'

/** Class hook used to re-skin the driver.js popover with the console tokens. */
const ONBOARDING_POPOVER_CLASS = 'herald-onboarding-popover'

/**
 * Anchors mount after the tour does — dashboard sections still loading their
 * data, or sidebar children revealed by the group auto-expand — so give them
 * this budget to appear before falling back to dropping the still-missing
 * steps.
 */
const ANCHOR_POLL_INTERVAL_MS = 150
const ANCHOR_POLL_TIMEOUT_MS = 3000

export interface ConsoleTourStep {
  /** data-testid of the anchor element; the step is dropped when absent. */
  testId: string
  /**
   * data-testid of the sidebar group header whose submenu must be open for
   * the anchor to render. The header carries `aria-expanded` with the
   * group's open state; a closed group is clicked open once before the
   * anchor poll starts, an already-open group is never clicked (its missing
   * anchor is permission-hidden, and toggling would collapse it).
   */
  expandTestId?: string
  title: string
  description: string
}

interface ConsoleTourProps {
  steps: ConsoleTourStep[]
  /** Layout pathname: an in-layout navigation swaps the page under the anchors. */
  pathname: string
  onFinish: () => void
  /** Torn down without the user finishing (route change): no completion. */
  onCancel: () => void
}

/**
 * Single-page console tour on top of driver.js. Anchors resolve to existing
 * stable testids; anchors that are merely late are awaited up to the poll
 * budget, steps whose anchor is still missing afterwards (permission-filtered
 * menus, error-state placeholders) are silently dropped, and when every anchor
 * is missing the tour never starts — the console stays fully usable either
 * way. Navigating within the admin layout unmounts the anchors out from under
 * driver.js, so such a route change cancels the run instead of leaving the
 * popover floating over detached elements.
 */
export function ConsoleTour({ steps, pathname, onFinish, onCancel }: ConsoleTourProps) {
  const finishedRef = useRef(false)
  const finishOnce = useCallback(() => {
    if (finishedRef.current) return
    finishedRef.current = true
    onFinish()
  }, [onFinish])
  const cancelOnce = useCallback(() => {
    if (finishedRef.current) return
    finishedRef.current = true
    onCancel()
  }, [onCancel])

  // Lets the route-change effect tear the current run down; the effect owns
  // `disposed` so neither its own cleanup destroy nor a cancellation destroy
  // reports the run as finished.
  const teardownRef = useRef<() => void>(() => {})
  const startPathnameRef = useRef(pathname)

  // StrictMode double-invokes the mount effect (setup → cleanup → setup)
  // before the first expansion click's state flushes to the DOM, so an
  // unguarded run would toggle a collapsed group open and shut again. Each
  // group header is clicked at most once per component lifetime — the ref
  // survives the double invocation on the same component instance.
  const expandedGroupsRef = useRef<Set<string>>(new Set())

  useEffect(() => {
    let disposed = false
    let driverObj: Driver | null = null
    let timer: number | null = null

    const availableSteps = () =>
      steps.filter((step) => document.querySelector(`[data-testid="${step.testId}"]`))

    // Expand the collapsed groups the tour anchors into (group toggling never
    // navigates, so the route-change cancel is unaffected). The header's
    // aria-expanded is the group's state: a closed group gets one opening
    // click, an open group whose anchor is missing stays untouched.
    const expandCollapsedGroups = () => {
      for (const step of steps) {
        const groupId = step.expandTestId
        if (!groupId || expandedGroupsRef.current.has(groupId)) continue
        expandedGroupsRef.current.add(groupId)
        if (document.querySelector(`[data-testid="${step.testId}"]`)) continue
        const header = document.querySelector(`[data-testid="${groupId}"]`)
        if (header?.getAttribute('aria-expanded') === 'true') continue
        ;(header as HTMLElement | null)?.click()
      }
    }

    expandCollapsedGroups()

    const launch = () => {
      const available = availableSteps()
      if (available.length === 0) {
        finishOnce()
        return
      }
      try {
        driverObj = driver({
          popoverClass: ONBOARDING_POPOVER_CLASS,
          nextBtnText: m['onboarding.tour_next'](),
          prevBtnText: m['onboarding.tour_prev'](),
          doneBtnText: m['onboarding.tour_done'](),
          // A user-driven teardown — finished, closed or exited mid-way —
          // reports once; teardowns initiated by this component stay silent.
          onDestroyed: () => {
            if (!disposed) finishOnce()
          },
          steps: available.map((step) => ({
            element: `[data-testid="${step.testId}"]`,
            popover: { title: step.title, description: step.description },
          })),
        })
        driverObj.drive()
      } catch {
        // driver.js failed to initialize: degrade to "tour over" instead of
        // breaking the console.
        finishOnce()
      }
    }

    const teardownTimer = () => {
      if (timer === null) return
      window.clearInterval(timer)
      timer = null
    }

    if (availableSteps().length === steps.length) {
      launch()
    } else {
      const deadline = Date.now() + ANCHOR_POLL_TIMEOUT_MS
      timer = window.setInterval(() => {
        if (availableSteps().length < steps.length && Date.now() < deadline) return
        teardownTimer()
        launch()
      }, ANCHOR_POLL_INTERVAL_MS)
    }

    teardownRef.current = () => {
      disposed = true
      teardownTimer()
      driverObj?.destroy()
      driverObj = null
    }

    return teardownRef.current
  }, [steps, finishOnce])

  // In-layout navigation replaces the page under the tour: cancel without
  // reporting the run as finished.
  useEffect(() => {
    if (pathname === startPathnameRef.current) return
    startPathnameRef.current = pathname
    teardownRef.current()
    cancelOnce()
  }, [pathname, cancelOnce])

  return null
}
