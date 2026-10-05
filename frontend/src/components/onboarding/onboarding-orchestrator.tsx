import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { useAuthStore } from '@/stores/auth-store'
import { useOnboardingUiStore } from '@/stores/onboarding-store'
import { useCurrentPathname, usePathSegments, useResolvedRealmContext } from '@/lib/realm-routing'
import { m } from '@/paraglide/messages'
import {
  consumeSignupSignal,
  readOnboardingCompletion,
  writeOnboardingCompletion,
} from './onboarding-storage'
import { OnboardingWelcomeDialog } from './onboarding-welcome-dialog'
import type { ConsoleTourStep } from './console-tour'

// driver.js rides in the lazy chunk: the tour runs at most once per
// realm+user, so the admin entry bundle must not carry it.
const ConsoleTour = lazy(() => import('./console-tour').then((m) => ({ default: m.ConsoleTour })))

type OnboardingPhase = 'idle' | 'welcome' | 'tour'

/**
 * Tour steps anchor the platform's selling points on the layout sidebar
 * (permission- or feature-hidden entries are silently dropped by the tour;
 * collapsed groups are auto-expanded via expandTestId) plus the account
 * avatar. Built per tour start so the step copy follows the interface
 * language at the moment the tour launches (no mid-tour hot switch; replay
 * refreshes).
 */
function buildTourSteps(): ConsoleTourStep[] {
  return [
    {
      testId: 'sidebar-menu-dashboard',
      title: m['onboarding.tour_step_overview_title'](),
      description: m['onboarding.tour_step_overview_description'](),
    },
    {
      testId: 'sidebar-menu-permissions',
      expandTestId: 'sidebar-menu-authorization',
      title: m['onboarding.tour_step_permissions_title'](),
      description: m['onboarding.tour_step_permissions_description'](),
    },
    {
      testId: 'sidebar-menu-payment-providers',
      expandTestId: 'sidebar-menu-products-&-payments',
      title: m['onboarding.tour_step_payments_title'](),
      description: m['onboarding.tour_step_payments_description'](),
    },
    {
      testId: 'sidebar-menu-credit-buckets',
      expandTestId: 'sidebar-menu-products-&-payments',
      title: m['onboarding.tour_step_points_title'](),
      description: m['onboarding.tour_step_points_description'](),
    },
    {
      testId: 'sidebar-menu-points-wallets',
      expandTestId: 'sidebar-menu-transactions',
      title: m['onboarding.tour_step_wallets_title'](),
      description: m['onboarding.tour_step_wallets_description'](),
    },
    {
      testId: 'sidebar-menu-settings',
      title: m['onboarding.tour_step_integrations_title'](),
      description: m['onboarding.tour_step_integrations_description'](),
    },
    {
      testId: 'user-avatar',
      title: m['onboarding.tour_step_account_title'](),
      description: m['onboarding.tour_step_account_description'](),
    },
  ]
}

/**
 * Onboarding guidance orchestrator, mounted in the admin dashboard layout.
 * The manage tree only renders for authenticated admins, so audience gating
 * comes from the mount point — no extra permission check here.
 *
 * State flow: mount → consume the one-shot signup signal and read the
 * completion marker (per realm + user). Marker present → nothing renders;
 * absent → welcome dialog. Finishing or dismissing the guidance writes the
 * marker, so it never auto-reappears; a replay request from the tasks card
 * restarts the tour without reading or writing the marker; a route change
 * mid-tour cancels it without writing the marker.
 *
 * Any storage failure degrades to rendering nothing — the console's own
 * functionality is never blocked by the guidance.
 */
export function OnboardingOrchestrator() {
  const realmId = useResolvedRealmContext().realmId
  const userId = useAuthStore((state) => state.user?.id) ?? null
  const pathname = useCurrentPathname()
  const segments = usePathSegments()
  const navigate = useNavigate()

  const [phase, setPhase] = useState<OnboardingPhase>('idle')
  const [freshSignup, setFreshSignup] = useState(false)
  /** False once storage proved unavailable — render nothing from then on. */
  const [storageUsable, setStorageUsable] = useState(true)
  /** Whether finishing the current tour run should record the marker. */
  const tourIsInitialGuidance = useRef(true)

  // Steps exist exactly while the tour phase does — deriving them keeps the
  // pair from drifting apart, and useMemo holds one stable reference per run.
  const tourSteps = useMemo(() => (phase === 'tour' ? buildTourSteps() : null), [phase])

  const tourRestartRequested = useOnboardingUiStore((state) => state.tourRestartRequested)
  const clearTourRestartRequest = useOnboardingUiStore((state) => state.clearTourRestartRequest)

  // Runs on mount and whenever the realm or user identity changes (an admin
  // managing several realms gets the guidance once per realm). Guarded by the
  // identity key because the evaluation consumes the one-shot signup signal —
  // StrictMode's double-invoked effects must not run it twice.
  const evaluatedKeyRef = useRef<string | null>(null)

  useEffect(() => {
    if (!userId) return
    const evaluatedKey = `${realmId}:${userId}`
    if (evaluatedKeyRef.current === evaluatedKey) return
    evaluatedKeyRef.current = evaluatedKey

    // eslint-disable-next-line react-hooks/set-state-in-effect -- one-shot mount evaluation (marker read + signal consume) drives the whole guidance state
    setPhase('idle')
    const completion = readOnboardingCompletion(realmId, userId)
    if (completion === 'unavailable') {
      setStorageUsable(false)
      return
    }
    setStorageUsable(true)
    // The signal only selects the welcome-copy variant (fresh signup vs
    // existing admin); the trigger itself is decided by the marker above.
    setFreshSignup(consumeSignupSignal(realmId))
    if (completion === 'incomplete') {
      setPhase('welcome')
    }
  }, [realmId, userId])

  // Every anchor is layout-level (sidebar entries + header avatar), so the
  // tour could run from any page; it still starts from the dashboard for a
  // consistent overview context, keeping the visitor's URL form
  // (realm-prefixed vs session-scoped mirror) so the route match is preserved.
  const goToDashboard = useCallback(() => {
    if (segments[0] === 'manage') {
      if (segments.length === 1) return
      navigate({ to: '/manage' })
      return
    }
    if (segments[1] === 'manage' && segments.length === 2) return
    navigate({ to: '/$realmId/manage', params: { realmId: segments[0] } })
  }, [segments, navigate])

  const startTour = useCallback(
    (initialGuidance: boolean) => {
      tourIsInitialGuidance.current = initialGuidance
      goToDashboard()
      setPhase('tour')
    },
    [goToDashboard]
  )

  const handleStartTour = useCallback(() => {
    startTour(true)
  }, [startTour])

  const handleDismiss = useCallback(() => {
    if (userId) writeOnboardingCompletion(realmId, userId)
    setPhase('idle')
  }, [realmId, userId])

  const handleTourFinished = useCallback(() => {
    if (tourIsInitialGuidance.current && userId) {
      writeOnboardingCompletion(realmId, userId)
    }
    setPhase('idle')
  }, [realmId, userId])

  const handleTourCancelled = useCallback(() => {
    setPhase('idle')
  }, [])

  // Replay requests from the tasks card: restart the tour, never touching the
  // completion marker (replay stays available forever).
  useEffect(() => {
    if (!tourRestartRequested) return
    clearTourRestartRequest()
    // eslint-disable-next-line react-hooks/set-state-in-effect -- consume the cross-component replay request once
    startTour(false)
  }, [tourRestartRequested, clearTourRestartRequest, startTour])

  if (!storageUsable) {
    return null
  }

  return (
    <>
      {phase === 'welcome' && (
        <OnboardingWelcomeDialog
          freshSignup={freshSignup}
          onStartTour={handleStartTour}
          onDismiss={handleDismiss}
        />
      )}
      {phase === 'tour' && tourSteps && (
        <Suspense fallback={null}>
          <ConsoleTour
            steps={tourSteps}
            pathname={pathname}
            onFinish={handleTourFinished}
            onCancel={handleTourCancelled}
          />
        </Suspense>
      )}
    </>
  )
}
