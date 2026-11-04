import type { UserStatus } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'

// Keyed by the generated UserStatus union so a new backend variant is a compile error, not UI drift.
export type { UserStatus }

const USER_STATUS_LABELS: Record<UserStatus, () => string> = {
  wait_verified: () => m['user_status.wait_verified'](),
  normal: () => m['user_status.normal'](),
  forbidden: () => m['user_status.forbidden'](),
  deleted: () => m['user_status.deleted'](),
}

export function getUserStatusLabel(status: UserStatus): string {
  return USER_STATUS_LABELS[status]()
}

export function getUserStatusOptions() {
  const entries = Object.keys(USER_STATUS_LABELS) as UserStatus[]
  return [
    { label: m['user_status.all'](), value: 'all' },
    ...entries.map((status) => ({
      label: USER_STATUS_LABELS[status](),
      value: status,
    })),
  ]
}

// Admin-created users skip email verification, so the backend rejects
// WaitVerified/Deleted writes; use the list filter for read-only status views.
export function getWritableUserStatusOptions() {
  return getUserStatusOptions().filter(
    (opt) => opt.value !== 'all' && opt.value !== 'wait_verified'
  )
}

export const USER_STATUS_COLORS: Record<UserStatus, string> = {
  wait_verified: 'bg-warning/10 text-warning',
  normal: 'bg-success/10 text-success',
  forbidden: 'bg-destructive/10 text-destructive',
  deleted: 'bg-muted text-muted-foreground',
}
