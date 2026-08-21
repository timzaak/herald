/**
 * Authentication Store (Zustand)
 *
 * Centralized state management for authentication and authorization.
 * Uses DevTools integration for debugging and persist middleware for localStorage.
 *
 * Token model (Bearer access/refresh token family), owned by the
 * `@herald/web` SDK client (`lib/herald-client.ts`):
 * - The rotating **refresh token** is persisted by the SDK's storage adapter
 *   (`herald.refreshToken`) so the app can restore the session on startup by
 *   refreshing; it is no longer part of this store.
 * - The short-lived **access token** lives only in the SDK's in-memory holder
 *   and is NEVER persisted. A full page reload clears it and triggers a
 *   refresh-first restore in `initializeAuth`.
 * - This store keeps only UI/routing auth state (user, permissions, the
 *   `refreshClientId` the token family binds to) plus the transient **PKCE
 *   verifier** and pending auth state, persisted so an in-progress PKCE login
 *   interrupted by a 2FA step (or a reload during the PKCE window) can still
 *   complete the token exchange.
 */

import { create } from 'zustand'
import { persist, devtools } from 'zustand/middleware'
import { useShallow } from 'zustand/react/shallow'
import type { UserProfile } from '@/lib/api-generated'
import { AUTH_STORAGE_KEY, AUTH_STORE_NAME } from '@/lib/constants/auth-constants'

/**
 * Shape of the persisted PKCE / pending-auth state. Carried through the
 * FirstParty login → 2FA → `oauthToken` exchange so the verifier survives a
 * reload or a second-factor detour within the PKCE code's TTL.
 */
export interface PersistedPkceState {
  /** The PKCE `code_verifier` used to complete the `oauthToken` exchange. */
  codeVerifier: string
  /** The OAuth `client_id` (e.g. `admin-web-console`) bound to this flow. */
  clientId: string
  /** The pre-registered `redirect_uri` the code was issued for. */
  redirectUri: string
  /** The CSRF `state` token used when seeding the OAuth authorize call. */
  state: string
}

/**
 * Authentication state
 */
export interface AuthState {
  // Authentication status
  isAuthenticated: boolean
  isLoading: boolean
  realmId: string | null
  clientAppId: string | null

  // User data
  user: UserProfile | null
  permissions: string[]
  roles: string[]

  /**
   * The `clientId` the current token family was issued for (admin console vs
   * account center) — routing state only; the tokens themselves live in the
   * Herald SDK client.
   */
  refreshClientId: string | null
  /** Transient PKCE + pending-auth state — persisted across reloads. */
  pkceState: PersistedPkceState | null
}

/**
 * Authentication actions
 */
export interface AuthActions {
  // Status actions
  setAuthStatus: (authenticated: boolean, realmId?: string, clientAppId?: string) => void
  setIsLoading: (isLoading: boolean) => void

  // User data actions
  setUserPermissions: (permissions: string[], roles: string[]) => void
  setUserProfile: (user: UserProfile | null) => void

  // Auth flow actions
  login: (realmId: string) => void
  logout: () => void

  /** Remember which product client the current token family binds to. */
  setRefreshClientId: (clientId: string | null) => void

  /** Persist the PKCE verifier + bound OAuth params for the active flow. */
  setPkceState: (state: PersistedPkceState | null) => void

  /** Read the persisted PKCE state (or null if no flow is in progress). */
  getPkceState: () => PersistedPkceState | null

  // Store actions
  reset: () => void
  clearStorage: () => void
}

/**
 * Initial state
 */
const initialState: AuthState = {
  isAuthenticated: false,
  isLoading: false,
  realmId: null,
  clientAppId: null,
  user: null,
  permissions: [],
  roles: [],
  refreshClientId: null,
  pkceState: null,
}

/**
 * Create the authentication store
 */
export const useAuthStore = create<AuthState & AuthActions>()(
  devtools(
    persist(
      (set, get) => ({
        ...initialState,

        // Status actions
        setAuthStatus: (authenticated, realmId, clientAppId) =>
          set({
            isAuthenticated: authenticated,
            realmId: realmId ?? get().realmId,
            clientAppId: clientAppId ?? get().clientAppId,
          }),

        setIsLoading: (isLoading) => set({ isLoading }),

        // User data actions
        setUserPermissions: (permissions, roles) => set({ permissions, roles }),

        setUserProfile: (user) => set({ user }),

        // Auth flow actions
        login: (realmId) =>
          set({
            isAuthenticated: true,
            realmId,
          }),

        logout: () => {
          set({
            isAuthenticated: false,
            isLoading: false,
            clientAppId: null,
            user: null,
            permissions: [],
            roles: [],
            refreshClientId: null,
            pkceState: null,
          })
        },

        setRefreshClientId: (refreshClientId) => set({ refreshClientId }),

        setPkceState: (pkceState) => set({ pkceState }),

        getPkceState: () => get().pkceState,

        // Store actions
        reset: () => {
          set(initialState)
        },

        clearStorage: () => {
          set(initialState)
        },
      }),
      {
        name: AUTH_STORAGE_KEY,
        partialize: (state) => ({
          // Persist UI/auth routing state and the in-flight PKCE state so a
          // reload can restore/complete the flow. The token family itself
          // (access in memory, refresh token) lives in the Herald SDK client
          // and must not be duplicated here.
          isAuthenticated: state.isAuthenticated,
          realmId: state.realmId,
          clientAppId: state.clientAppId,
          user: state.user,
          permissions: state.permissions,
          roles: state.roles,
          refreshClientId: state.refreshClientId,
          pkceState: state.pkceState,
        }),
      }
    ),
    { name: AUTH_STORE_NAME }
  )
)

/**
 * Get the persist storage instance to clear storage
 * This is needed for proper logout that clears both state and storage
 */
const persistStorage = useAuthStore.persist

/**
 * Clear all persisted auth data from storage
 */
export function clearAuthStorage(): void {
  persistStorage.clearStorage()
}

/**
 * Selector hooks for optimized re-renders
 */

/**
 * Get authentication status
 */
export const useIsAuthenticated = () => useAuthStore((state) => state.isAuthenticated)

/**
 * Get loading state
 */
export const useIsLoading = () => useAuthStore((state) => state.isLoading)

/**
 * Get user data
 */
export const useUser = () => useAuthStore((state) => state.user)

/**
 * Get permissions
 */
export const usePermissions = () => useAuthStore((state) => state.permissions)

/**
 * Get roles
 */
export const useRoles = () => useAuthStore((state) => state.roles)

/**
 * Get realm ID
 */
export const useRealmId = () => useAuthStore((state) => state.realmId || 'admin')

/**
 * Get actions
 */
export const useAuthActions = () =>
  useAuthStore(
    useShallow((state) => ({
      setAuthStatus: state.setAuthStatus,
      setIsLoading: state.setIsLoading,
      setUserPermissions: state.setUserPermissions,
      setUserProfile: state.setUserProfile,
      login: state.login,
      logout: state.logout,
      setRefreshClientId: state.setRefreshClientId,
      setPkceState: state.setPkceState,
      reset: state.reset,
    }))
  )
