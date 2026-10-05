/**
 * Browser-local storage for the admin-console onboarding guidance.
 *
 * Completion marker: localStorage `herald.onboarding.{realmId}.{userId}` —
 * keyed per realm AND user so an admin managing multiple realms gets the
 * guidance once per realm. Written when the tour completes or the welcome is
 * dismissed, never deleted; clearing browser data naturally re-shows the
 * guidance once (accepted product behavior).
 *
 * Signup signal: sessionStorage `herald.onboarding.signup-signal` — a
 * consume-once flag whose value is the NEW realm's id, written by the
 * self-service signup page right before the existing navigation into that
 * realm's console and consumed only by that realm's orchestrator (a stray
 * signal must not flavor an unrelated realm's welcome). It only selects the
 * welcome-copy variant; whether the guidance triggers at all is decided by
 * the completion marker alone (both entry paths share that check).
 */

const COMPLETION_KEY_PREFIX = 'herald.onboarding.'
const SIGNUP_SIGNAL_KEY = 'herald.onboarding.signup-signal'

export function onboardingCompletionKey(realmId: string, userId: string): string {
  return `${COMPLETION_KEY_PREFIX}${realmId}.${userId}`
}

export type OnboardingCompletionStatus = 'complete' | 'incomplete' | 'unavailable'

export function readOnboardingCompletion(
  realmId: string,
  userId: string
): OnboardingCompletionStatus {
  try {
    return window.localStorage.getItem(onboardingCompletionKey(realmId, userId)) === 'completed'
      ? 'complete'
      : 'incomplete'
  } catch {
    // Storage unavailable (blocked cookies, privacy mode, quota): the caller
    // degrades to rendering no guidance at all.
    return 'unavailable'
  }
}

export function writeOnboardingCompletion(realmId: string, userId: string): void {
  try {
    window.localStorage.setItem(onboardingCompletionKey(realmId, userId), 'completed')
  } catch {
    // A failed write only means the guidance re-appears next session; never
    // surface an error for it.
  }
}

export function markSignupSignal(realmId: string): void {
  try {
    window.sessionStorage.setItem(SIGNUP_SIGNAL_KEY, realmId)
  } catch {
    // Best-effort: without the signal the welcome copy falls back to the
    // generic variant, which is the same shape of guidance.
  }
}

export function consumeSignupSignal(realmId: string): boolean {
  try {
    if (window.sessionStorage.getItem(SIGNUP_SIGNAL_KEY) !== realmId) return false
    window.sessionStorage.removeItem(SIGNUP_SIGNAL_KEY)
    return true
  } catch {
    return false
  }
}
