/**
 * Authentication Store (Zustand)
 *
 * Centralized state management for authentication and authorization.
 * UI authentication state stays in this tab's memory. Only a pending PKCE
 * transaction is persisted in sessionStorage so 2FA/reload can complete it.
 *
 * Token model (Bearer access/refresh token family), owned by the
 * `herald-auth-web` SDK client (`lib/herald-client.ts`):
 * - The rotating **refresh token** is persisted by the SDK's storage adapter
 *   (`herald.refreshToken:{realmId}`, one slot per realm) so the app can
 *   restore the session on startup by refreshing; it is no longer part of
 *   this store.
 * - The short-lived **access token** lives only in the SDK's in-memory holder
 *   and is NEVER persisted. A full page reload clears it and triggers a
 *   refresh-first restore in `initializeAuth`.
 * - User, permissions and product binding are restored from the realm's
 *   credential and are never hydrated from a shared authentication snapshot.
 */

import { create } from 'zustand'
import { persist, devtools, createJSONStorage } from 'zustand/middleware'
import { useShallow } from 'zustand/react/shallow'
import type { UserProfile } from '@/lib/api-generated'
import { AUTH_STORAGE_KEY, AUTH_STORE_NAME } from '@/lib/constants/auth-constants'

/**
 * Shape of the persisted PKCE / pending-auth state. Carried through the
 * FirstParty login → 2FA → `oauthToken` exchange so the verifier survives a
 * reload or a second-factor detour within the PKCE code's TTL.
 */
export interface PersistedPkceState {
  realmId: string
  expiresAt: number
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

  /** Read the pending PKCE state — null when no unexpired flow is in progress. */
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

        /** Read the pending PKCE state — null when no unexpired flow is in
         * progress (expired transactions are dead and must not be reused). */
        getPkceState: () => {
          const pkce = get().pkceState
          return pkce && pkce.expiresAt > Date.now() ? pkce : null
        },

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
        storage: createJSONStorage(() => sessionStorage),
        partialize: (state) => ({
          // Only the current tab's pending flow survives reload. Authentication
          // is restored from the realm's SDK credential, never a UI snapshot.
          pkceState: state.pkceState,
        }),
        merge: (persisted, current) => {
          const pkce = (persisted as { pkceState?: PersistedPkceState } | undefined)?.pkceState
          return {
            ...current,
            pkceState: pkce?.realmId && pkce.expiresAt > Date.now() ? pkce : null,
          }
        },
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
  window.localStorage.removeItem(AUTH_STORAGE_KEY)
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
