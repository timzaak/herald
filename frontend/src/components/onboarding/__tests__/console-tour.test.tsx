import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act } from '@testing-library/react'
import { StrictMode } from 'react'
import { ConsoleTour, type ConsoleTourStep } from '../console-tour'

// driver.js stand-in: records the config the wrapper built and exposes the
// drive/destroy calls so tests assert the wrapper's lifecycle contract.
const driverMocks = vi.hoisted(() => {
  const configs: Array<Record<string, unknown>> = []
  return {
    configs,
    drive: vi.fn(),
    destroy: vi.fn(),
  }
})

vi.mock('driver.js', () => ({
  driver: (config: Record<string, unknown>) => {
    driverMocks.configs.push(config)
    return { drive: driverMocks.drive, destroy: driverMocks.destroy }
  },
}))

const steps: ConsoleTourStep[] = [
  { testId: 'anchor-a', title: 'A', description: 'Step A' },
  { testId: 'anchor-b', title: 'B', description: 'Step B' },
  { testId: 'anchor-c', title: 'C', description: 'Step C' },
]

const expandSteps: ConsoleTourStep[] = [
  { testId: 'sidebar-menu-dashboard', title: 'D', description: 'D' },
  {
    testId: 'sidebar-menu-points-wallets',
    expandTestId: 'sidebar-menu-transactions',
    title: 'W',
    description: 'W',
  },
]

// Anchors live in the real DOM (querySelector is the resolution mechanism the
// component uses), so mount them manually and clean up after each test.
const mountedAnchors: HTMLElement[] = []
function mountAnchor(testId: string): HTMLElement {
  const el = document.createElement('div')
  el.setAttribute('data-testid', testId)
  document.body.appendChild(el)
  mountedAnchors.push(el)
  return el
}

// Mounts the Transactions group the way the sidebar ships it: a collapsed
// header (aria-expanded="false") whose submenu — with the child anchor
// inside — joins the DOM on the first header click and detaches on the next,
// toggling like the real openMenus state.
function mountCollapsedTransactionsGroup(): HTMLElement {
  const header = mountAnchor('sidebar-menu-transactions')
  header.setAttribute('aria-expanded', 'false')
  const submenu = document.createElement('div')
  submenu.setAttribute('data-testid', 'sidebar-submenu-transactions')
  const child = document.createElement('div')
  child.setAttribute('data-testid', 'sidebar-menu-points-wallets')
  submenu.appendChild(child)
  mountedAnchors.push(submenu)
  header.addEventListener('click', () => {
    if (submenu.isConnected) submenu.remove()
    else document.body.appendChild(submenu)
  })
  return header
}

// Anchors mount after the tour does — data-gated dashboard sections, or
// sidebar children revealed by the group auto-expand — so the tour must
// await late anchors within its budget and only then fall back to dropping
// the still-missing steps; teardowns the component itself initiates
// (unmount, route change, StrictMode remount) must never report the run as
// finished.
describe('ConsoleTour anchor waiting and lifecycle', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    driverMocks.configs.length = 0
    driverMocks.drive.mockClear()
    driverMocks.destroy.mockClear()
  })

  afterEach(() => {
    mountedAnchors.splice(0).forEach((el) => el.remove())
    vi.useRealTimers()
  })

  it('GIVEN anchors are still mounting (dashboard skeletons) WHEN the tour starts THEN it waits for them within the poll budget instead of dropping them', () => {
    const onFinish = vi.fn()
    mountAnchor('anchor-a')
    mountAnchor('anchor-c')
    render(<ConsoleTour steps={steps} pathname="/manage" onFinish={onFinish} onCancel={vi.fn()} />)

    // Missing anchor-b: keep waiting, don't drive a truncated tour.
    act(() => {
      vi.advanceTimersByTime(600)
    })
    expect(driverMocks.drive).not.toHaveBeenCalled()

    // The skeleton resolves: the step must join the tour, not be dropped.
    mountAnchor('anchor-b')
    act(() => {
      vi.advanceTimersByTime(150)
    })
    expect(driverMocks.drive).toHaveBeenCalledTimes(1)
    const config = driverMocks.configs[0] as {
      steps: Array<{ element: string; popover: { title: string } }>
    }
    expect(config.steps).toHaveLength(3)
    expect(config.steps.map((step) => step.element)).toEqual([
      '[data-testid="anchor-a"]',
      '[data-testid="anchor-b"]',
      '[data-testid="anchor-c"]',
    ])
    expect(onFinish).not.toHaveBeenCalled()
  })

  it('GIVEN an anchor never appears (permission-filtered, error state) WHEN the poll budget expires THEN the tour drives with the resolvable steps', () => {
    mountAnchor('anchor-a')
    mountAnchor('anchor-c')
    render(<ConsoleTour steps={steps} pathname="/manage" onFinish={vi.fn()} onCancel={vi.fn()} />)

    act(() => {
      vi.advanceTimersByTime(3000)
    })
    expect(driverMocks.drive).toHaveBeenCalledTimes(1)
    const config = driverMocks.configs[0] as {
      steps: Array<{ element: string; popover: { title: string } }>
    }
    expect(config.steps).toHaveLength(2)
    expect(config.steps.map((step) => step.popover.title)).toEqual(['A', 'C'])
  })

  it('GIVEN every anchor is missing WHEN the poll budget expires THEN driver.js is never invoked and the tour reports finished', () => {
    const onFinish = vi.fn()
    render(<ConsoleTour steps={steps} pathname="/manage" onFinish={onFinish} onCancel={vi.fn()} />)

    act(() => {
      vi.advanceTimersByTime(3000)
    })
    expect(driverMocks.configs).toHaveLength(0)
    expect(driverMocks.drive).not.toHaveBeenCalled()
    expect(onFinish).toHaveBeenCalledTimes(1)
  })

  // Selling-point steps anchor sidebar children that only render while their
  // group is expanded, and the sidebar ships those groups collapsed. Uses the
  // Transactions group: jsdom's selector engine fails to match attribute
  // values containing "&" (the Products & Payments group), though real
  // browsers match them fine — that group's expansion is exercised by the
  // e2e demo in Chromium.
  it('GIVEN a step anchors inside a collapsed sidebar group WHEN the tour starts THEN the group header is clicked open and the revealed step joins the tour', () => {
    mountCollapsedTransactionsGroup()
    mountAnchor('sidebar-menu-dashboard')

    render(
      <ConsoleTour steps={expandSteps} pathname="/manage" onFinish={vi.fn()} onCancel={vi.fn()} />
    )

    // The opening click reveals the child before the availability check, so
    // the tour drives immediately with both steps.
    expect(driverMocks.drive).toHaveBeenCalledTimes(1)
    const config = driverMocks.configs[0] as { steps: Array<{ element: string }> }
    expect(config.steps.map((step) => step.element)).toEqual([
      '[data-testid="sidebar-menu-dashboard"]',
      '[data-testid="sidebar-menu-points-wallets"]',
    ])
  })

  it('GIVEN StrictMode double-invokes the mount effects WHEN a step needs a collapsed group expanded THEN the header is clicked exactly once instead of toggled open and shut', () => {
    const header = mountCollapsedTransactionsGroup()
    const clickSpy = vi.spyOn(header, 'click')
    mountAnchor('sidebar-menu-dashboard')

    render(
      <StrictMode>
        <ConsoleTour steps={expandSteps} pathname="/manage" onFinish={vi.fn()} onCancel={vi.fn()} />
      </StrictMode>
    )

    // A second setup click would collapse the group before the anchor poll
    // ever resolves it, silently degrading the tour — the expansion must be
    // once per component lifetime, not once per effect run.
    expect(clickSpy).toHaveBeenCalledTimes(1)
    expect(driverMocks.drive).toHaveBeenCalledTimes(2)
    const config = driverMocks.configs[1] as { steps: Array<{ element: string }> }
    expect(config.steps.map((step) => step.element)).toEqual([
      '[data-testid="sidebar-menu-dashboard"]',
      '[data-testid="sidebar-menu-points-wallets"]',
    ])
  })

  it('GIVEN the group is already open but the anchor is permission-hidden WHEN the tour starts THEN the group is not toggled shut and the step drops after the budget', () => {
    const header = mountAnchor('sidebar-menu-transactions')
    header.setAttribute('aria-expanded', 'true')
    mountAnchor('sidebar-menu-dashboard')
    const clickSpy = vi.spyOn(header, 'click')

    render(
      <ConsoleTour steps={expandSteps} pathname="/manage" onFinish={vi.fn()} onCancel={vi.fn()} />
    )

    // Toggling an open group would collapse it and take other steps' anchors
    // with it, so an open group must never be clicked.
    expect(clickSpy).not.toHaveBeenCalled()
    act(() => {
      vi.advanceTimersByTime(3000)
    })
    const config = driverMocks.configs[0] as { steps: Array<{ popover: { title: string } }> }
    expect(config.steps.map((step) => step.popover.title)).toEqual(['D'])
  })

  it('GIVEN the whole sidebar group is filtered out WHEN the tour starts THEN nothing is clicked and the step drops after the budget', () => {
    mountAnchor('sidebar-menu-settings')

    const expandSteps: ConsoleTourStep[] = [
      { testId: 'sidebar-menu-settings', title: 'S', description: 'S' },
      {
        testId: 'sidebar-menu-payment-providers',
        expandTestId: 'sidebar-menu-products-&-payments',
        title: 'P',
        description: 'P',
      },
    ]
    render(
      <ConsoleTour steps={expandSteps} pathname="/manage" onFinish={vi.fn()} onCancel={vi.fn()} />
    )

    act(() => {
      vi.advanceTimersByTime(3000)
    })
    const config = driverMocks.configs[0] as { steps: Array<{ popover: { title: string } }> }
    expect(config.steps.map((step) => step.popover.title)).toEqual(['S'])
  })

  it('GIVEN the tour is running WHEN the user tears it down THEN the finish callback fires exactly once and unmounting destroys the instance', () => {
    const onFinish = vi.fn()
    steps.forEach((step) => mountAnchor(step.testId))
    const { unmount } = render(
      <ConsoleTour steps={steps} pathname="/manage" onFinish={onFinish} onCancel={vi.fn()} />
    )
    expect(driverMocks.drive).toHaveBeenCalledTimes(1)

    const config = driverMocks.configs[0] as { onDestroyed: () => void }
    act(() => {
      config.onDestroyed()
    })
    expect(onFinish).toHaveBeenCalledTimes(1)

    unmount()
    expect(driverMocks.destroy).toHaveBeenCalledTimes(1)

    // A late teardown callback (the real destroy() fires onDestroyed) must
    // not re-report completion.
    act(() => {
      config.onDestroyed()
    })
    expect(onFinish).toHaveBeenCalledTimes(1)
  })

  it('GIVEN the tour is running WHEN an in-layout navigation changes the page THEN the run is cancelled and destroyed without reporting finish', () => {
    const onFinish = vi.fn()
    const onCancel = vi.fn()
    steps.forEach((step) => mountAnchor(step.testId))
    const { rerender, unmount } = render(
      <ConsoleTour steps={steps} pathname="/manage" onFinish={onFinish} onCancel={onCancel} />
    )
    expect(driverMocks.drive).toHaveBeenCalledTimes(1)

    rerender(
      <ConsoleTour steps={steps} pathname="/manage/users" onFinish={onFinish} onCancel={onCancel} />
    )
    expect(driverMocks.destroy).toHaveBeenCalledTimes(1)
    expect(onCancel).toHaveBeenCalledTimes(1)
    expect(onFinish).not.toHaveBeenCalled()

    // Unmount after the cancellation must not destroy a second time.
    unmount()
    expect(driverMocks.destroy).toHaveBeenCalledTimes(1)
  })

  it('GIVEN StrictMode double-invokes the mount effects WHEN the tour starts THEN the cleanup destroy stays silent and only the surviving run reports finish', () => {
    const onFinish = vi.fn()
    steps.forEach((step) => mountAnchor(step.testId))
    render(
      <StrictMode>
        <ConsoleTour steps={steps} pathname="/manage" onFinish={onFinish} onCancel={vi.fn()} />
      </StrictMode>
    )

    // setup → cleanup → setup: two driver instances, one destroy from the
    // aborted first setup — and no finish reported for it.
    expect(driverMocks.configs).toHaveLength(2)
    expect(driverMocks.drive).toHaveBeenCalledTimes(2)
    expect(driverMocks.destroy).toHaveBeenCalledTimes(1)
    expect(onFinish).not.toHaveBeenCalled()

    // The surviving run finishes normally.
    const config = driverMocks.configs[1] as { onDestroyed: () => void }
    act(() => {
      config.onDestroyed()
    })
    expect(onFinish).toHaveBeenCalledTimes(1)
  })
})
