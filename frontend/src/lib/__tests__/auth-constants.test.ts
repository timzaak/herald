import { describe, it, expect } from 'vitest'
import {
  PERMISSION,
  ADMIN_PERMISSIONS,
  ADMIN_WEB_CONSOLE_CLIENT_ID,
  USER_ACCOUNT_CENTER_CLIENT_ID,
  firstPartyClientForPath,
} from '@/lib/constants/auth-constants'

describe('first-party product routing', () => {
  it('binds both scoped and realm-prefixed manage paths to the admin console', () => {
    expect(firstPartyClientForPath('/manage/users')).toBe(ADMIN_WEB_CONSOLE_CLIENT_ID)
    expect(firstPartyClientForPath('/tenant-a/manage/users')).toBe(ADMIN_WEB_CONSOLE_CLIENT_ID)
  })

  it('defaults roots, auth pages, and personal pages to the account center', () => {
    expect(firstPartyClientForPath('/')).toBe(USER_ACCOUNT_CENTER_CLIENT_ID)
    expect(firstPartyClientForPath('/auth/login')).toBe(USER_ACCOUNT_CENTER_CLIENT_ID)
    expect(firstPartyClientForPath('/tenant-a/user/profile')).toBe(USER_ACCOUNT_CENTER_CLIENT_ID)
  })
})

describe('PERMISSION constant object', () => {
  it('does NOT contain legacy REALM_ADMIN or REALM_CREATE keys', () => {
    expect(PERMISSION).not.toHaveProperty('REALM_ADMIN')
    expect(PERMISSION).not.toHaveProperty('REALM_CREATE')
  })
})

describe('ADMIN_PERMISSIONS array', () => {
  it('does NOT contain REALM_ADMIN, REALM_CREATE, or POINTS_VIEW', () => {
    // REALM_ADMIN and REALM_CREATE no longer exist in PERMISSION;
    // guard against accidental re-addition via string literals
    expect(ADMIN_PERMISSIONS).not.toContain('realm.admin')
    expect(ADMIN_PERMISSIONS).not.toContain('realm.create')
    // POINTS_VIEW belongs to the user role, not admin
    expect(ADMIN_PERMISSIONS).not.toContain(PERMISSION.POINTS_VIEW)
  })

  it('contains DASHBOARD_VIEW, AUDIT_VIEW, API_KEYS_VIEW, REALM_MANAGE', () => {
    expect(ADMIN_PERMISSIONS).toContain(PERMISSION.DASHBOARD_VIEW)
    expect(ADMIN_PERMISSIONS).toContain(PERMISSION.AUDIT_VIEW)
    expect(ADMIN_PERMISSIONS).toContain(PERMISSION.API_KEYS_VIEW)
    expect(ADMIN_PERMISSIONS).toContain(PERMISSION.REALM_MANAGE)
  })

  it('every value corresponds to a key in the PERMISSION object', () => {
    const permissionValues = new Set(Object.values(PERMISSION))
    for (const entry of ADMIN_PERMISSIONS) {
      expect(permissionValues).toContain(entry)
    }
  })
})
