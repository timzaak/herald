/**
 * Onboarding UI Store (Zustand)
 *
 * Cross-component UI state for the console onboarding guidance: the tasks
 * card's "replay tour" button lives in the dashboard page while the tour
 * runner lives in the layout, so the request travels through this store.
 * Pure request flag — no server data, not persisted.
 */

import { create } from 'zustand'

export interface OnboardingUiState {
  /** True while a replay-tour request is pending consumption by the runner. */
  tourRestartRequested: boolean
  requestTourRestart: () => void
  clearTourRestartRequest: () => void
}

export const useOnboardingUiStore = create<OnboardingUiState>()((set) => ({
  tourRestartRequested: false,
  requestTourRestart: () => set({ tourRestartRequested: true }),
  clearTourRestartRequest: () => set({ tourRestartRequested: false }),
}))
