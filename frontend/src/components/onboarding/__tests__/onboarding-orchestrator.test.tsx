import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act } from 'react'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useAuthStore } from '@/stores/auth-store'
import { useOnboardingUiStore } from '@/stores/onboarding-store'
import { onboardingCompletionKey } from '../onboarding-storage'
import { OnboardingOrchestrator } from '../onboarding-orchestrator'

// Router stub: the orchestrator resolves the marker-key realm through the
// shared realm-routing hook, which reads the current pathname via useLocation,
// and moves to the dashboard via useNavigate. Hoisted so each test can point
// it at a different realm's console.
const routerState = vi.hoisted(() => ({ pathname: '/admin/manage' }))
const navigateMock = vi.hoisted(() => vi.fn())
vi.mock('@tanstack/react-router', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@tanstack/react-router')>()
  return {
    ...actual,
    useLocation: () => ({ pathname: routerState.pathname }),
    useNavigate: () => navigateMock,
  }
})

// Tour stand-in (driver.js never loads here): records the running tour's
// finish/cancel callbacks so tests can simulate "walked to the end", "exited
// mid-way" and "the page navigated under the tour".
const tourStub = vi.hoisted(() => ({
  onFinish: null as null | (() => void),
  onCancel: null as null | (() => void),
}))
vi.mock('../console-tour', () => ({
  ConsoleTour: ({ onFinish, onCancel }: { onFinish: () => void; onCancel: () => void }) => {
    tourStub.onFinish = onFinish
    tourStub.onCancel = onCancel
    return <div data-testid="console-tour-stub" />
  },
}))

function finishTour() {
  tourStub.onFinish?.()
}

function setUser(userId: string) {
  act(() => {
    useAuthStore.setState({
      user: { id: userId, email: 'admin@example.com', nickname: null, status: 1 },
    })
  })
}

// First-login guidance turns on the completion marker alone (per realm +
// user), finishes or dismissals record it, and replays never touch it. Any
// storage failure must degrade to rendering nothing — never block the console.
describe('OnboardingOrchestrator first-login guidance', () => {
  beforeEach(() => {
    window.localStorage.clear()
    window.sessionStorage.clear()
    routerState.pathname = '/admin/manage'
    tourStub.onFinish = null
    tourStub.onCancel = null
    navigateMock.mockClear()
    act(() => {
      useAuthStore.setState({ user: null, realmId: null })
      useOnboardingUiStore.setState({ tourRestartRequested: false })
    })
  })

  it('GIVEN no completion marker for this realm+user WHEN the console mounts THEN the welcome dialog opens', () => {
    setUser('user-1')
    render(<OnboardingOrchestrator />)

    expect(screen.getByTestId('onboarding-welcome-dialog')).toBeInTheDocument()
  })

  it('GIVEN a signup signal keyed to this realm is pending WHEN the console mounts THEN the fresh-signup welcome variant renders and the signal is consumed once', () => {
    window.sessionStorage.setItem('herald.onboarding.signup-signal', 'admin')
    setUser('user-1')
    render(<OnboardingOrchestrator />)

    expect(screen.getByText('Welcome to Your New Realm')).toBeInTheDocument()
    expect(window.sessionStorage.getItem('herald.onboarding.signup-signal')).toBeNull()
  })

  it('GIVEN the pending signup signal belongs to another realm WHEN this realm console mounts THEN the generic variant renders and the signal is left for its own realm', () => {
    // A signal stranded by a signup whose navigation never reached the new
    // realm must not flavor an unrelated realm's welcome.
    window.sessionStorage.setItem('herald.onboarding.signup-signal', 'acme')
    setUser('user-1')
    render(<OnboardingOrchestrator />)

    expect(screen.getByText('Welcome to the Console')).toBeInTheDocument()
    expect(window.sessionStorage.getItem('herald.onboarding.signup-signal')).toBe('acme')
  })

  it('GIVEN a completion marker exists WHEN the console mounts THEN no guidance renders and the pending signup signal is still consumed', () => {
    window.localStorage.setItem(onboardingCompletionKey('admin', 'user-1'), 'completed')
    window.sessionStorage.setItem('herald.onboarding.signup-signal', 'admin')
    setUser('user-1')
    const { container } = render(<OnboardingOrchestrator />)

    expect(screen.queryByTestId('onboarding-welcome-dialog')).not.toBeInTheDocument()
    expect(screen.queryByTestId('console-tour-stub')).not.toBeInTheDocument()
    expect(container.textContent).toBe('')
    expect(window.sessionStorage.getItem('herald.onboarding.signup-signal')).toBeNull()
  })

  it('GIVEN the admin completed onboarding in another realm WHEN they enter this realm console THEN the guidance appears for the new realm', () => {
    window.localStorage.setItem(onboardingCompletionKey('admin', 'user-1'), 'completed')
    routerState.pathname = '/acme/manage'
    setUser('user-1')
    render(<OnboardingOrchestrator />)

    expect(screen.getByTestId('onboarding-welcome-dialog')).toBeInTheDocument()
  })

  it('GIVEN the welcome is showing WHEN it is dismissed THEN the completion marker is written for this realm+user and the dialog closes', async () => {
    const user = userEvent.setup({ delay: null })
    setUser('user-1')
    render(<OnboardingOrchestrator />)

    await user.click(screen.getByTestId('onboarding-welcome-dismiss-button'))

    expect(window.localStorage.getItem(onboardingCompletionKey('admin', 'user-1'))).toBe(
      'completed'
    )
    expect(screen.queryByTestId('onboarding-welcome-dialog')).not.toBeInTheDocument()
  })

  it('GIVEN the tour runs from the welcome WHEN it finishes or is exited THEN the completion marker is written and the guidance closes', async () => {
    const user = userEvent.setup({ delay: null })
    setUser('user-1')
    render(<OnboardingOrchestrator />)

    await user.click(screen.getByTestId('onboarding-tour-start-button'))
    expect(await screen.findByTestId('console-tour-stub')).toBeInTheDocument()
    // The dashboard anchors the tour; starting there must not navigate.
    expect(navigateMock).not.toHaveBeenCalled()

    act(() => {
      finishTour()
    })

    expect(window.localStorage.getItem(onboardingCompletionKey('admin', 'user-1'))).toBe(
      'completed'
    )
    expect(screen.queryByTestId('console-tour-stub')).not.toBeInTheDocument()
  })

  it('GIVEN the welcome opened on a non-dashboard admin page WHEN the tour starts THEN the console first moves to the realm-prefixed dashboard route', async () => {
    routerState.pathname = '/acme/manage/users'
    const user = userEvent.setup({ delay: null })
    setUser('user-1')
    render(<OnboardingOrchestrator />)

    await user.click(screen.getByTestId('onboarding-tour-start-button'))
    expect(await screen.findByTestId('console-tour-stub')).toBeInTheDocument()

    // All but two anchors live on the dashboard page; the tour must go get
    // them instead of silently running a 2-step truncation.
    expect(navigateMock).toHaveBeenCalledWith({
      to: '/$realmId/manage',
      params: { realmId: 'acme' },
    })
  })

  it('GIVEN the welcome opened on the mirror (session-scoped) manage tree WHEN the tour starts THEN the console moves to the mirror dashboard route', async () => {
    routerState.pathname = '/manage/users'
    const user = userEvent.setup({ delay: null })
    setUser('user-1')
    render(<OnboardingOrchestrator />)

    await user.click(screen.getByTestId('onboarding-tour-start-button'))
    expect(await screen.findByTestId('console-tour-stub')).toBeInTheDocument()

    expect(navigateMock).toHaveBeenCalledWith({ to: '/manage' })
  })

  it('GIVEN the tour is running WHEN the page navigates under it (route-change cancel) THEN no marker is written and the guidance closes', async () => {
    const user = userEvent.setup({ delay: null })
    setUser('user-1')
    render(<OnboardingOrchestrator />)

    await user.click(screen.getByTestId('onboarding-tour-start-button'))
    expect(await screen.findByTestId('console-tour-stub')).toBeInTheDocument()

    act(() => {
      tourStub.onCancel?.()
    })

    // A cancelled run is neither finished nor dismissed: the marker stays
    // unwritten so the guidance can be offered again.
    expect(window.localStorage.getItem(onboardingCompletionKey('admin', 'user-1'))).toBeNull()
    expect(screen.queryByTestId('console-tour-stub')).not.toBeInTheDocument()
  })

  it('GIVEN a replay request from the tasks card WHEN it arrives THEN the tour restarts without reading or writing the completion marker', async () => {
    const setItemSpy = vi.spyOn(window.localStorage, 'setItem')
    window.localStorage.setItem(onboardingCompletionKey('admin', 'user-1'), 'completed')
    setUser('user-1')
    render(<OnboardingOrchestrator />)
    setItemSpy.mockClear()

    // Completed marker does not block a replay (the request flag is the only
    // input), and no welcome appears on mount.
    expect(screen.queryByTestId('onboarding-welcome-dialog')).not.toBeInTheDocument()

    act(() => {
      useOnboardingUiStore.getState().requestTourRestart()
    })
    expect(await screen.findByTestId('console-tour-stub')).toBeInTheDocument()

    act(() => {
      finishTour()
    })

    // Finishing a replay records nothing and does not resurrect the welcome.
    expect(setItemSpy).not.toHaveBeenCalledWith(
      onboardingCompletionKey('admin', 'user-1'),
      'completed'
    )
    expect(screen.queryByTestId('onboarding-welcome-dialog')).not.toBeInTheDocument()
    setItemSpy.mockRestore()
  })

  it('GIVEN localStorage is unavailable WHEN the console mounts THEN the guidance degrades to nothing and the console tree is unaffected', () => {
    // jsdom exposes the storage methods on Storage.prototype; an instance-level
    // spyOn silently fails to intercept, so target the prototype.
    const getItemSpy = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('storage blocked')
    })
    setUser('user-1')

    render(
      <>
        <div data-testid="console-outlet-stub" />
        <OnboardingOrchestrator />
      </>
    )

    expect(screen.getByTestId('console-outlet-stub')).toBeInTheDocument()
    expect(screen.queryByTestId('onboarding-welcome-dialog')).not.toBeInTheDocument()
    expect(screen.queryByTestId('console-tour-stub')).not.toBeInTheDocument()
    getItemSpy.mockRestore()
  })
})
