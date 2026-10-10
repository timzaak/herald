import { z } from 'zod'

export const paginationSchema = z.object({
  page: z.number().int().min(0).optional(),
  page_size: z.number().int().min(1).max(100).optional(),
})

export const loginSearchSchema = z.object({
  redirect: z.string().optional(),
  clientId: z.string().optional(),
  oauthClientId: z.string().optional(),
  redirectUri: z.string().optional(),
  state: z.string().optional(),
})

export const usersSearchSchema = z.object({
  page: z.number().int().min(0).optional(),
  pageSize: z.number().int().min(1).max(100).optional(),
  email: z.string().optional(),
  status: z.string().optional(),
})

export const realmsSearchSchema = z.object({
  page: z.number().int().min(0).optional(),
  pageSize: z.number().int().min(1).max(100).optional(),
  search: z.string().optional(),
  sortBy: z.string().optional(),
  sortOrder: z.enum(['asc', 'desc']).optional(),
})

export const clientAppsSearchSchema = z.object({
  page: z.number().int().min(0).optional(),
  pageSize: z.number().int().min(1).max(100).optional(),
})

export const auditSearchSchema = z.object({
  page: z.number().int().min(0).optional(),
  pageSize: z.number().int().min(1).max(100).optional(),
  category: z.string().optional(),
  action: z.string().optional(),
  actorId: z.string().optional(),
  startTime: z.string().optional(),
  endTime: z.string().optional(),
})

export const apiKeysSearchSchema = z.object({
  page: z.number().int().min(0).optional(),
  pageSize: z.number().int().min(1).max(100).optional(),
})

export const resetPasswordSearchSchema = z.object({
  code: z.string().min(1),
})

// `code` stays optional: the confirm page renders an invalid-link state when
// it is missing (e.g. dropped by the login redirect) instead of letting
// validateSearch throw the route into its error boundary.
export const changeEmailSearchSchema = z.object({
  code: z.string().optional(),
})

/**
 * Search params for the purchase-points page. The payment provider redirects
 * back here with `attemptId` (so the page can resume polling) and `status`
 * (`success` for a completed checkout bounce, `cancel` for a Stripe cancel
 * bounce — Creem has no cancel_url). Payment status itself is confirmed via
 * webhook; these params only drive the UX bounce. `wechatOpenid` is the
 * caller-provided openid required for WeChat JSAPI payment inside WeChat
 * (e.g. appended by a WeChat official-account menu link); without it the
 * in-WeChat flow refuses to order instead of falling back to a QR.
 */
export const purchasePointsSearchSchema = z.object({
  attemptId: z.string().uuid().optional(),
  status: z.enum(['success', 'cancel']).optional(),
  wechatOpenid: z.string().min(1).optional(),
})

export type ResetPasswordSearchParams = z.infer<typeof resetPasswordSearchSchema>
export type ChangeEmailSearchParams = z.infer<typeof changeEmailSearchSchema>
export type PaginationParams = z.infer<typeof paginationSchema>
export type LoginSearchParams = z.infer<typeof loginSearchSchema>
export type UsersSearchParams = z.infer<typeof usersSearchSchema>
export type RealmsSearchParams = z.infer<typeof realmsSearchSchema>
export type ClientAppsSearchParams = z.infer<typeof clientAppsSearchSchema>
export type AuditSearchParams = z.infer<typeof auditSearchSchema>
export type ApiKeysSearchParams = z.infer<typeof apiKeysSearchSchema>
export type PurchasePointsSearchParams = z.infer<typeof purchasePointsSearchSchema>
