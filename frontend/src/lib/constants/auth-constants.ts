/**
 * Authentication Constants
 *
 * Centralized constants and utility functions for authentication and authorization.
 * This file contains the admin permissions list and related helper functions
 * that were previously duplicated across multiple files.
 */

// Permission string constants - single source of truth matching backend rbac_init/services.rs
export const PERMISSION = {
  REALM_VIEW: 'realm.view',
  DASHBOARD_VIEW: 'dashboard.view',
  REALM_MANAGE: 'realm.manage',
  USERS_VIEW: 'users.view',
  USERS_MANAGE: 'users.manage',
  CLIENTS_VIEW: 'clients.view',
  CLIENTS_MANAGE: 'clients.manage',
  ROLES_VIEW: 'roles.view',
  ROLES_MANAGE: 'roles.manage',
  PERMISSIONS_VIEW: 'permissions.view',
  PERMISSIONS_MANAGE: 'permissions.manage',
  POLICIES_VIEW: 'policies.view',
  POLICIES_MANAGE: 'policies.manage',
  SETTINGS_VIEW: 'settings.view',
  SETTINGS_MANAGE: 'settings.manage',
  AUDIT_VIEW: 'audit.view',
  API_KEYS_VIEW: 'api_keys.view',
  API_KEYS_MANAGE: 'api_keys.manage',
  BILLING_VIEW: 'billing.view',
  BILLING_MANAGE: 'billing.manage',
  POINTS_VIEW: 'points.view',
  POINTS_MANAGE: 'points.manage',
} as const

// Admin permissions - used by hasAdminPermission to determine admin vs regular user
// Note: POINTS_VIEW excluded because it's also assigned to the `user` role
export const ADMIN_PERMISSIONS = [
  PERMISSION.REALM_VIEW,
  PERMISSION.DASHBOARD_VIEW,
  PERMISSION.REALM_MANAGE,
  PERMISSION.USERS_VIEW,
  PERMISSION.USERS_MANAGE,
  PERMISSION.CLIENTS_VIEW,
  PERMISSION.CLIENTS_MANAGE,
  PERMISSION.ROLES_VIEW,
  PERMISSION.ROLES_MANAGE,
  PERMISSION.PERMISSIONS_VIEW,
  PERMISSION.PERMISSIONS_MANAGE,
  PERMISSION.POLICIES_VIEW,
  PERMISSION.POLICIES_MANAGE,
  PERMISSION.SETTINGS_VIEW,
  PERMISSION.SETTINGS_MANAGE,
  PERMISSION.AUDIT_VIEW,
  PERMISSION.API_KEYS_VIEW,
  PERMISSION.API_KEYS_MANAGE,
  PERMISSION.BILLING_VIEW,
  PERMISSION.BILLING_MANAGE,
  PERMISSION.POINTS_MANAGE,
] as const

/**
 * Check if user has any admin permission
 *
 * @param permissions - List of permission strings
 * @returns true if user has any admin permission
 */
export function hasAdminPermission(permissions?: string[]): boolean {
  const normalizedPermissions = permissions?.map((permission) => permission.replace(':', '.'))
  return ADMIN_PERMISSIONS.some((perm) => normalizedPermissions?.includes(perm))
}

/**
 * Allowed redirect paths - whitelist to prevent open redirect attacks
 * These patterns expect relative paths (without realm prefix)
 */
const ALLOWED_REDIRECT_PATTERNS = [
  /^\/$/, // Root path
  /^\/user(\/.*)?$/, // User related paths (relative, without realm prefix)
  /^\/manage(\/.*)?$/, // Management related paths (relative, without realm prefix)
]

/**
 * Validate redirect path to prevent open redirect vulnerabilities
 *
 * @param redirectPath - The redirect path to validate
 * @returns true if the path is safe to redirect to
 */
export function validateRedirectPath(redirectPath: string | undefined): boolean {
  if (!redirectPath) return true

  if (redirectPath.startsWith('http://') || redirectPath.startsWith('https://')) {
    try {
      return new URL(redirectPath).hostname === window.location.hostname
    } catch {
      return false
    }
  }

  // Reject protocol-relative URLs
  if (redirectPath.startsWith('//')) {
    return false
  }

  // Reject paths starting with \ to prevent Windows-style absolute paths
  if (redirectPath.startsWith('\\')) {
    return false
  }

  // Check against whitelist
  return ALLOWED_REDIRECT_PATTERNS.some((pattern) => pattern.test(redirectPath))
}

/**
 * Get safe redirect path or fallback
 *
 * @param redirectPath - The requested redirect path
 * @param fallbackPath - The fallback path if redirectPath is invalid
 * @returns The safe redirect path
 */
export function getSafeRedirectPath(
  redirectPath: string | undefined,
  fallbackPath: string
): string {
  return validateRedirectPath(redirectPath) ? (redirectPath ?? fallbackPath) : fallbackPath
}

/**
 * Tab-local pending authentication flow key (sessionStorage).
 * The former localStorage entry under this key is discarded on auth restore.
 */
export const AUTH_STORAGE_KEY = 'auth-storage'

/**
 * Name for Zustand store (used in DevTools)
 */
export const AUTH_STORE_NAME = 'AuthStore'

/**
 * The platform-level realm id. Mirrors `herald_core::domain::realm::ADMIN_REALM_ID`.
 * Hosts platform-wide entries (e.g. self-service realm signup) and is the realm
 * the console + first-party client ids below are bound to.
 */
export const ADMIN_REALM_ID = 'admin'

/**
 * The built-in FirstParty OAuth Client App `client_id` for the Herald console.
 * Bound to the PKCE flow and to refresh-token families issued by the console.
 */
export const ADMIN_WEB_CONSOLE_CLIENT_ID = 'admin-web-console'
export const USER_ACCOUNT_CENTER_CLIENT_ID = 'user-account-center'

/**
 * The built-in (non-first-party) client app `client_id` backing API-key
 * authentication. Seeded at realm creation like the two above — see
 * `herald_core::domain::realm::services`.
 */
export const ADMIN_API_CLIENT_ID = 'admin-api-client'

export type FirstPartyClientId =
  | typeof ADMIN_WEB_CONSOLE_CLIENT_ID
  | typeof USER_ACCOUNT_CENTER_CLIENT_ID

export function firstPartyClientForPath(path: string | undefined): FirstPartyClientId {
  return path?.match(/(?:^|\/)manage(?:\/|$)/)
    ? ADMIN_WEB_CONSOLE_CLIENT_ID
    : USER_ACCOUNT_CENTER_CLIENT_ID
}

/** Default first-party identity for realm roots and self-service auth pages. */
export const FIRST_PARTY_CLIENT_ID = USER_ACCOUNT_CENTER_CLIENT_ID

/**
 * Default redirect path for regular users
 */
export const DEFAULT_USER_REDIRECT = '/user/profile'

/**
 * Default redirect path for admin users
 */
export const DEFAULT_ADMIN_REDIRECT = '/manage'
