import { m } from '@/paraglide/messages'

export const USER_STATUS = {
  WAIT_VERIFIED: 0,
  NORMAL: 1,
  FORBIDDEN: 2,
} as const

export type UserStatus = (typeof USER_STATUS)[keyof typeof USER_STATUS]

export function getUserStatusLabel(status: number): string {
  switch (status) {
    case USER_STATUS.WAIT_VERIFIED:
      return m['user_status.wait_verified']()
    case USER_STATUS.NORMAL:
      return m['user_status.normal']()
    case USER_STATUS.FORBIDDEN:
      return m['user_status.forbidden']()
    default:
      return String(status)
  }
}

export function getUserStatusOptions() {
  return [
    { label: m['user_status.all'](), value: 'all' },
    { label: m['user_status.wait_verified'](), value: String(USER_STATUS.WAIT_VERIFIED) },
    { label: m['user_status.normal'](), value: String(USER_STATUS.NORMAL) },
    { label: m['user_status.forbidden'](), value: String(USER_STATUS.FORBIDDEN) },
  ]
}

// Admin create/edit writes toggle enable/disable only (Normal/Forbidden):
// admin-created users skip email verification and the backend rejects
// WaitVerified writes; use the list filter for read-only status views.
export function getWritableUserStatusOptions() {
  return getUserStatusOptions().filter(
    (opt) => opt.value !== 'all' && opt.value !== String(USER_STATUS.WAIT_VERIFIED)
  )
}

export const USER_STATUS_COLORS: Record<number, string> = {
  [USER_STATUS.WAIT_VERIFIED]: 'bg-warning/10 text-warning',
  [USER_STATUS.NORMAL]: 'bg-success/10 text-success',
  [USER_STATUS.FORBIDDEN]: 'bg-destructive/10 text-destructive',
}
