import { queryOptions, keepPreviousData, type QueryClient } from '@tanstack/react-query'
import { redirect } from '@tanstack/react-router'
import {
  listUsers2,
  getUser,
  getUser2,
  listPermissions,
  getPermission,
  listRoles,
  getRole,
  getRolePermissions,
  getCurrentUserPermissions,
  getUserRoles,
  adminGetUserRoles,
  listRealmsPaginated,
  getRealm2,
  listClientApps,
  getClientApp,
  listOauthConfigs,
  getOauthConfig,
  getPublicConfig,
  getTurnstileStatus,
  status2 as emailOtpStatus,
  status3 as passkeyStatus,
  ldapStatus,
  listRealmConfigsByType,
  getSignupStatus,
  getProfile,
  handleGetTotpStatus,
  handleListPasskeyCredentials,
  handleGetRealmPasskeyConfig,
  handleGetRealmEmailOtpConfig,
  handleGetWhiteLabelConfig,
  handleGetCustomDomainConfig,
  getSubscriptionForClientApp,
  listWallets,
  getWallet,
  listTransactions,
  listUserTransactions,
  listUserWallets,
  getRegistrationRules,
  upsertRegistrationRules,
  getPaymentAttemptStatus,
  listPaymentProviders,
  listPurchaseOptions,
  getPurchaseHistory,
  listAuditEvents,
  getAuditEvent,
  getDashboardStats,
  emailStatus,
  listApiKeys,
  getApiKey,
  adminGetApiKeyRoles,
  adminUpdateApiKeyRoles,
  getEntitlementMapping,
  getSubscription,
  listCreditBucketsHandler,
  getCreditBucketHandler,
  getBucketOverviewHandler,
  listAgreements,
  getAgreement,
  getConsentStatus,
  recordConsent,
  deleteAccount,
  listUserSessions,
  adminListAgreements,
  adminPublishCustom,
  adminRevertToDefault,
  adminGetDraft,
  adminSaveDraft,
  adminPublishFromDraft,
  adminDiscardDraft,
  adminGetVersion,
  getFeatureAvailability,
  getUserFeatureAvailability,
  getSubscriptionHistory as getSubscriptionHistoryApi,
  listSubscriptionHistory,
} from '@/lib/api-generated'
import { handleApiResponse } from '@/lib/api-utils'
import { ApiResponseError, resolveApiError } from '@/lib/error-utils'
import { obtainReauthToken } from '@/lib/reauth-flow'
import { FIRST_PARTY_CLIENT_ID } from '@/lib/constants/auth-constants'
import type {
  OAuthConfigResponse,
  PaymentAttemptStatusResponse,
  PointsWalletResponse,
  EntitlementMappingListResponse,
  EntitlementMappingResponse,
  SubscriptionListResponse,
  SubscriptionDetailResponse,
  PurchaseOptionListResponse,
  PurchaseHistoryResponse,
  BucketResponse,
  BucketDetailResponse,
  BucketOverviewResponse,
  ListWalletsByBucketResponse,
  UpsertRegistrationRulesRequest,
  RegistrationRulesResponse,
  LegalAgreementSummary,
  ConsentStatusItem,
  RecordConsentRequest,
  PublishCustomRequest,
  PublishVersionResponse,
  LegalAgreementDraftResponse,
  SaveDraftRequest,
  WhiteLabelConfigStateResponse,
  CustomDomainConfigStateResponse,
  UserSessionResponse,
  FeatureAvailabilityResponse as GeneratedFeatureAvailabilityResponse,
  UserFeatureAvailabilityResponse,
} from '@/lib/api-generated'
import type {
  HistoryFilters,
  SingleSubscriptionHistoryResponse,
  GlobalSubscriptionHistoryResponse,
} from '@/types/billing'
import { TIME_CONSTANTS, QUERY_KEYS, QUERY_TIMING } from '@/lib/constants'
import { client } from '@/lib/api-generated/client.gen'

// ==================== Enhanced Error Handling ====================

function handleApiErrorWithStatus(error: unknown): never {
  throw error instanceof ApiResponseError ? error : new ApiResponseError(error)
}

const { GC_TIME_5_MIN, RETRY_COUNT, STALE_TIME_2_MIN, STALE_TIME_5_MIN } = QUERY_TIMING
const GC_TIME_10_MIN = 10 * 60 * 1000

const isClientError = (error: unknown): boolean => {
  const status = resolveApiError(error).status
  return status !== undefined && status >= 400 && status < 500
}

const clientErrorRetry = (failureCount: number, error: unknown): boolean => {
  if (isClientError(error)) return false
  return failureCount < RETRY_COUNT
}

export const queryKeys = {
  publicConfig: (realmId: string) => [QUERY_KEYS.PUBLIC_CONFIG, realmId] as const,
  realms: (filters: Record<string, unknown>) => [QUERY_KEYS.REALMS, filters] as const,
  realmsList: () => [QUERY_KEYS.REALMS] as const,
  realm: (realmId: string | null) => [QUERY_KEYS.REALM, realmId] as const,
  users: (realmId: string, filters: Record<string, unknown>) =>
    [QUERY_KEYS.USERS, realmId, filters] as const,
  usersList: (realmId: string) => [QUERY_KEYS.USERS, realmId] as const,
  user: (realmId: string, userId: string) => [QUERY_KEYS.USER, realmId, userId] as const,
  adminUser: (realmId: string, userId: string) =>
    [QUERY_KEYS.USER, realmId, 'admin', userId] as const,
  permissions: (realmId: string) => [QUERY_KEYS.PERMISSIONS, realmId] as const,
  permission: (realmId: string, permissionId: string) =>
    [QUERY_KEYS.PERMISSION, realmId, permissionId] as const,
  roles: (realmId: string) => [QUERY_KEYS.ROLES, realmId] as const,
  role: (realmId: string, roleId: string) => [QUERY_KEYS.ROLE, realmId, roleId] as const,
  rolePermissions: (realmId: string, roleId: string) =>
    [QUERY_KEYS.ROLE_PERMISSIONS, realmId, roleId] as const,
  adminUserRoles: (realmId: string, userId: string) =>
    [QUERY_KEYS.ADMIN_USER_ROLES, realmId, userId] as const,
  userSessions: (realmId: string, userId: string) =>
    [QUERY_KEYS.USER_SESSIONS, realmId, userId] as const,
  clientApps: (realmId: string, filters: Record<string, unknown>) =>
    [QUERY_KEYS.CLIENT_APPS, realmId, filters] as const,
  clientAppsList: (realmId: string) => [QUERY_KEYS.CLIENT_APPS, realmId] as const,
  clientApp: (realmId: string, id: string) => [QUERY_KEYS.CLIENT_APP, realmId, id] as const,
  oauthConfigs: (realmId: string) => [QUERY_KEYS.OAUTH_CONFIGS, realmId] as const,
  oauthConfig: (realmId: string, providerType: string) =>
    [QUERY_KEYS.OAUTH_CONFIGS, realmId, providerType] as const,
  profile: () => [QUERY_KEYS.PROFILE] as const,
  totpStatus: () => [QUERY_KEYS.TOTP_STATUS] as const,
  passkeyList: () => [QUERY_KEYS.PASSKEY_LIST] as const,
  passkeyRealmConfig: (realmId: string) => [QUERY_KEYS.PASSKEY_REALM_CONFIG, realmId] as const,
  emailOtpRealmConfig: (realmId: string) => [QUERY_KEYS.EMAIL_OTP_REALM_CONFIG, realmId] as const,
  whiteLabelRealmConfig: (realmId: string) =>
    [QUERY_KEYS.WHITE_LABEL_REALM_CONFIG, realmId] as const,
  customDomainRealmConfig: (realmId: string) =>
    [QUERY_KEYS.CUSTOM_DOMAIN_REALM_CONFIG, realmId] as const,
  turnstileStatus: (realmId: string, clientId: string) =>
    [QUERY_KEYS.TURNSTILE_STATUS, realmId, clientId] as const,
  emailOtpStatus: (realmId: string) => [QUERY_KEYS.EMAIL_OTP_STATUS, realmId] as const,
  passkeyStatus: (realmId: string) => [QUERY_KEYS.PASSKEY_STATUS, realmId] as const,
  ldapStatus: (realmId: string) => [QUERY_KEYS.LDAP_STATUS, realmId] as const,
  ldapRealmConfig: (realmId: string) => [QUERY_KEYS.LDAP_REALM_CONFIG, realmId] as const,
  signupStatus: (realmId: string) => [QUERY_KEYS.SIGNUP_STATUS, realmId] as const,
  subscription: (realmId: string, clientAppId: string) =>
    [QUERY_KEYS.SUBSCRIPTION, realmId, clientAppId] as const,
  subscriptionDetails: (realmId: string, subscriptionId: string) =>
    [QUERY_KEYS.SUBSCRIPTION_DETAILS, realmId, subscriptionId] as const,
  subscriptionHistory: (realmId: string, subscriptionId: string) =>
    [QUERY_KEYS.SUBSCRIPTION_HISTORY, realmId, subscriptionId] as const,
  globalSubscriptionHistory: (
    realmId: string,
    filters: HistoryFilters,
    page: number,
    pageSize: number
  ) => [QUERY_KEYS.GLOBAL_SUBSCRIPTION_HISTORY, realmId, filters, page, pageSize] as const,
  userSubscriptions: (realmId: string, clientAppIds: string) =>
    [QUERY_KEYS.USER_SUBSCRIPTIONS, realmId, clientAppIds] as const,
  pointsWallets: (realmId: string, filters: Record<string, unknown>) =>
    [QUERY_KEYS.POINTS_WALLETS, realmId, filters] as const,
  pointsWallet: (realmId: string, userId: string) =>
    [QUERY_KEYS.POINTS_WALLET, realmId, userId] as const,
  pointsTransactions: (realmId: string, filters: Record<string, unknown>) =>
    [QUERY_KEYS.POINTS_TRANSACTIONS, realmId, filters] as const,
  userPointsTransactions: (filters: Record<string, unknown>) =>
    [QUERY_KEYS.USER_POINTS_TRANSACTIONS, filters] as const,
  userPointsWallets: () => [QUERY_KEYS.USER_POINTS_WALLETS] as const,
  registrationRules: (realmId: string) => [QUERY_KEYS.REGISTRATION_RULES, realmId] as const,
  realmConfigs: (realmId: string) => [QUERY_KEYS.REALM_CONFIGS, realmId] as const,
  emailStatus: (realmId: string) => [QUERY_KEYS.EMAIL_STATUS, realmId] as const,
  userRoles: () => [QUERY_KEYS.USER_ROLES] as const,
  purchaseOptions: (realmId: string, clientAppId: string) =>
    [QUERY_KEYS.PURCHASE_OPTIONS, realmId, clientAppId] as const,
  purchaseHistory: (realmId: string, filters: Record<string, unknown>) =>
    [QUERY_KEYS.PURCHASE_HISTORY, realmId, filters] as const,
  paymentAttemptStatus: (realmId: string, attemptId: string) =>
    [QUERY_KEYS.PAYMENT_ATTEMPT_STATUS, realmId, attemptId] as const,
  paymentProviders: (realmId: string) => [QUERY_KEYS.PAYMENT_PROVIDERS, realmId] as const,
  audit: (realmId: string, filters?: Record<string, unknown>) =>
    [QUERY_KEYS.AUDIT_EVENTS, realmId, filters ?? {}] as const,
  auditDetail: (realmId: string, eventId: string) =>
    [QUERY_KEYS.AUDIT_EVENT, realmId, eventId] as const,
  dashboardStats: (realmId: string) => [QUERY_KEYS.DASHBOARD_STATS, realmId] as const,
  featureAvailability: (realmId: string) => [QUERY_KEYS.FEATURE_AVAILABILITY, realmId] as const,
  userFeatureAvailability: () => [QUERY_KEYS.USER_FEATURE_AVAILABILITY] as const,
  apiKeys: (realmId: string, filters: { page?: number; pageSize?: number }) =>
    [QUERY_KEYS.API_KEYS, realmId, filters] as const,
  apiKeysList: (realmId: string) => [QUERY_KEYS.API_KEYS, realmId] as const,
  apiKey: (realmId: string, id: string) => [QUERY_KEYS.API_KEY, realmId, id] as const,
  apiKeyRoles: (realmId: string, apiKeyId: string) =>
    [QUERY_KEYS.API_KEY_ROLES, realmId, apiKeyId] as const,
  entitlementMappings: (realmId: string, filters: Record<string, unknown>) =>
    [QUERY_KEYS.ENTITLEMENT_MAPPINGS, realmId, filters] as const,
  entitlementMapping: (realmId: string, mappingId: string) =>
    [QUERY_KEYS.ENTITLEMENT_MAPPING, realmId, mappingId] as const,
  subscriptions: (realmId: string, filters: Record<string, unknown>) =>
    [QUERY_KEYS.ADMIN_SUBSCRIPTIONS, realmId, filters] as const,
  adminSubscription: (realmId: string, subscriptionId: string) =>
    [QUERY_KEYS.ADMIN_SUBSCRIPTION, realmId, subscriptionId] as const,
  creditBucketsList: (realmId: string) => [QUERY_KEYS.CREDIT_BUCKETS, realmId] as const,
  creditBucket: (realmId: string, bucketId: string) =>
    [QUERY_KEYS.CREDIT_BUCKETS, realmId, bucketId] as const,
  creditBucketOverview: (realmId: string) => [QUERY_KEYS.CREDIT_BUCKET_OVERVIEW, realmId] as const,
  walletsByBucket: (realmId: string) => [QUERY_KEYS.WALLETS_BY_BUCKET, realmId] as const,
  legalAgreements: (realmId: string) => [QUERY_KEYS.LEGAL_AGREEMENTS, realmId] as const,
  legalAgreement: (realmId: string, agreementType: string, locale?: string) =>
    [QUERY_KEYS.LEGAL_AGREEMENT, realmId, agreementType, locale] as const,
  consentStatus: (realmId: string) => [QUERY_KEYS.CONSENT_STATUS, realmId] as const,
  legalAdminAgreements: (realmId: string) => [QUERY_KEYS.LEGAL_ADMIN_AGREEMENTS, realmId] as const,
  legalDraft: (realmId: string, agreementType: string) =>
    [QUERY_KEYS.LEGAL_DRAFT, realmId, agreementType] as const,
  legalVersion: (realmId: string, versionId: string) =>
    [QUERY_KEYS.LEGAL_AGREEMENT, realmId, versionId] as const,
}

// ==================== Public Config ====================

export const publicConfigQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.publicConfig(realmId),
    queryFn: async () => {
      const response = await getPublicConfig({
        path: { realmId },
      })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: 0,
    refetchOnMount: 'always',
    refetchOnWindowFocus: true,
    gcTime: GC_TIME_10_MIN,
  })

// ==================== Realms ====================

export const realmQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.realm(realmId),
    queryFn: async () => handleApiResponse(await getRealm2({ path: { realmId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export type FeatureAvailabilityResponse = GeneratedFeatureAvailabilityResponse

export const featureAvailabilityQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.featureAvailability(realmId),
    queryFn: async () => handleApiResponse(await getFeatureAvailability({ path: { realmId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// Current user's feature availability (`GET /api/user/feature-availability`).
// Unlike the admin `featureAvailabilityQueryOptions`, this needs no
// billing/points view permission — only the `FeatureRead` token scope — so it
// is the right source for user-facing pages (e.g. the security page gating the
// passkey tab on `passkeyEnabled`).
export const userFeatureAvailabilityQueryOptions = queryOptions({
  queryKey: queryKeys.userFeatureAvailability(),
  queryFn: async () => handleApiResponse(await getUserFeatureAvailability()),
  retry: RETRY_COUNT,
  staleTime: STALE_TIME_2_MIN,
  gcTime: GC_TIME_5_MIN,
})

export async function requireFeature(
  queryClient: QueryClient,
  realmId: string,
  check: (features: FeatureAvailabilityResponse) => boolean,
  redirectOptions: { to: string; params?: Record<string, string>; search?: Record<string, unknown> }
) {
  const features = await queryClient.ensureQueryData(featureAvailabilityQueryOptions(realmId))
  if (!check(features)) {
    throw redirect(redirectOptions)
  }
}

export async function requireUserFeature(
  queryClient: QueryClient,
  check: (features: UserFeatureAvailabilityResponse) => boolean,
  redirectOptions: { to: string; params?: Record<string, string>; search?: Record<string, unknown> }
) {
  const features = await queryClient.ensureQueryData(userFeatureAvailabilityQueryOptions)
  if (!check(features)) {
    throw redirect(redirectOptions)
  }
}

export const realmsQueryOptions = (filters: {
  page?: number
  pageSize?: number
  search?: string
  sortBy?: string
  sortOrder?: string
}) =>
  queryOptions({
    queryKey: queryKeys.realms(filters),
    queryFn: async () =>
      handleApiResponse(
        await listRealmsPaginated({
          query: {
            page: filters.page ?? 0,
            pageSize: filters.pageSize ?? 20,
            search: filters.search,
            sortBy: filters.sortBy ?? 'created_at',
            sortOrder: filters.sortOrder ?? 'desc',
          },
        })
      ),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

// ==================== Users ====================

export const usersQueryOptions = (
  realmId: string,
  filters: {
    page?: number
    pageSize?: number
    email?: string
    status?: string
  }
) =>
  queryOptions({
    queryKey: queryKeys.users(realmId, filters),
    queryFn: async () =>
      handleApiResponse(
        await listUsers2({
          path: { realmId },
          query: {
            page: filters.page ?? 0,
            pageSize: filters.pageSize ?? 20,
            email: filters.email,
            // status is supported by backend but not yet in generated types
            ...(filters.status ? { status: Number(filters.status) } : {}),
          } as { page?: number; pageSize?: number; email?: string; status?: number },
        })
      ),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const adminUsersQueryOptions = usersQueryOptions

export const userQueryOptions = (realmId: string, userId: string) =>
  queryOptions({
    queryKey: queryKeys.user(realmId, userId),
    queryFn: async () => handleApiResponse(await getUser({ path: { realmId, userId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const adminUserQueryOptions = (realmId: string, userId: string) =>
  queryOptions({
    queryKey: queryKeys.adminUser(realmId, userId),
    queryFn: async () => handleApiResponse(await getUser2({ path: { realmId, userId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

// ==================== Permissions ====================

export const permissionsQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.permissions(realmId),
    queryFn: async () => handleApiResponse(await listPermissions({ path: { realmId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const adminPermissionsQueryOptions = permissionsQueryOptions

export const permissionQueryOptions = (realmId: string, permissionId: string) =>
  queryOptions({
    queryKey: queryKeys.permission(realmId, permissionId),
    queryFn: async () =>
      handleApiResponse(
        await getPermission({ path: { realmId, permissionDefinitionId: permissionId } })
      ),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

// ==================== Roles ====================

export const rolesQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.roles(realmId),
    queryFn: async () => handleApiResponse(await listRoles({ path: { realmId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const adminRolesQueryOptions = rolesQueryOptions

export const roleQueryOptions = (realmId: string, roleId: string) =>
  queryOptions({
    queryKey: queryKeys.role(realmId, roleId),
    queryFn: async () => handleApiResponse(await getRole({ path: { realmId, roleId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const rolePermissionsQueryOptions = (realmId: string, roleId: string) =>
  queryOptions({
    queryKey: queryKeys.rolePermissions(realmId, roleId),
    queryFn: async () => handleApiResponse(await getRolePermissions({ path: { realmId, roleId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

// ==================== User Roles ====================

export const userRolesQueryOptions = () =>
  queryOptions({
    queryKey: queryKeys.userRoles(),
    queryFn: async () => handleApiResponse(await getUserRoles()),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const currentUserRolesQueryOptions = userRolesQueryOptions

export const currentUserPermissionsQueryOptions = () =>
  queryOptions({
    queryKey: [QUERY_KEYS.PERMISSIONS, 'current-user'] as const,
    queryFn: async () => handleApiResponse(await getCurrentUserPermissions()),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const adminUserRolesQueryOptions = (realmId: string, userId: string) =>
  queryOptions({
    queryKey: queryKeys.adminUserRoles(realmId, userId),
    queryFn: async () =>
      adminGetUserRoles({
        path: { realmId, userId },
      }),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

export const userSessionsQueryOptions = (realmId: string, userId: string) =>
  queryOptions({
    queryKey: queryKeys.userSessions(realmId, userId),
    queryFn: async () =>
      handleApiResponse(
        await listUserSessions({ path: { realmId, userId } })
      ) as UserSessionResponse[],
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

// ==================== Client Apps ====================

export const clientAppsQueryOptions = (
  realmId: string,
  filters: {
    page?: number
    pageSize?: number
  }
) =>
  queryOptions({
    queryKey: queryKeys.clientApps(realmId, filters),
    queryFn: async () =>
      handleApiResponse(
        await listClientApps({
          path: { realmId },
          query: {
            page: filters.page ?? 0,
            pageSize: filters.pageSize ?? 20,
          },
        })
      ),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const clientAppQueryOptions = (realmId: string, id: string) =>
  queryOptions({
    queryKey: queryKeys.clientApp(realmId, id),
    queryFn: async () =>
      handleApiResponse(await getClientApp({ path: { realmId, clientAppId: id } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

// ==================== OAuth Configurations ====================

export const providerConfigsQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.oauthConfigs(realmId),
    queryFn: async () => {
      const response = await listOauthConfigs({ path: { realmId } })
      if (response.error) throw response.error
      return response.data as OAuthConfigResponse[]
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

export const providerConfigQueryOptions = (realmId: string, providerType: string) =>
  queryOptions({
    queryKey: queryKeys.oauthConfig(realmId, providerType),
    queryFn: async () => {
      const response = await getOauthConfig({ path: { realmId, providerType } })
      if (response.error) throw response.error
      return response.data as OAuthConfigResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== User Profile ====================

export const profileQueryOptions = queryOptions({
  queryKey: queryKeys.profile(),
  queryFn: async () => {
    const response = await getProfile()
    if (response.error) throw response.error
    return response.data
  },
  retry: RETRY_COUNT,
  staleTime: STALE_TIME_5_MIN,
  gcTime: GC_TIME_10_MIN,
})

export const currentUserProfileQueryOptions = profileQueryOptions

// `clientId` defaults to the first-party Client App: every non-login auth page
// (register/forgot-password/reset-password/verify-email) targets it, so they
// call this with a single `realmId` arg. Only `login.tsx` passes a resolved
// per-request clientId.
export const turnstileStatusQueryOptions = (
  realmId: string,
  clientId: string = FIRST_PARTY_CLIENT_ID
) =>
  queryOptions({
    queryKey: queryKeys.turnstileStatus(realmId, clientId),
    queryFn: async () => {
      const response = await getTurnstileStatus({
        path: { realmId },
        query: { clientId },
      })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

// ==================== Email OTP Status (public) ====================
//
// Reads the Realm's OTP-login enablement flag
// (`GET /api/auth/{realmId}/email-otp/status`). Public; consumed by the login
// route to gate the "Email code" entry visibility.
export const emailOtpStatusQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.emailOtpStatus(realmId),
    queryFn: async () => {
      const response = await emailOtpStatus({ path: { realmId } })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== Passkey Status (public) ====================
//
// Reads the Realm's Passkey enablement flag
// (`GET /api/auth/{realmId}/passkey/status`). Public; consumed by the login
// route to gate the passkey entry visibility BEFORE the PasskeyLoginForm is
// mounted (so a realm with passkey disabled never fires the begin-options
// probe request at all). Mirrors `emailOtpStatusQueryOptions`.
export const passkeyStatusQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.passkeyStatus(realmId),
    queryFn: async () => {
      const response = await passkeyStatus({ path: { realmId } })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== LDAP Status (public) ====================
//
// Reads the Realm's corporate-directory login enablement flag
// (`GET /api/auth/{realmId}/ldap/status`). Public; consumed by the login route
// to gate the "corporate account" entry visibility. Fail-closed: the entry is
// rendered only on an explicit `enabled === true` (missing/failed → hidden).
export const ldapStatusQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.ldapStatus(realmId),
    queryFn: async () => {
      const response = await ldapStatus({ path: { realmId } })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== Signup Status (public) ====================
//
// Reads the platform self-service realm-signup enablement flag
// (`GET /api/auth/{realmId}/signup/status`, admin realm only). Public; consumed
// by the public signup route to gate the entry visibility (DEC-009). Missing
// config → `enabled:false` (fail-closed, DEC-013). Mirrors
// `emailOtpStatusQueryOptions`, except `staleTime: 0` so the entry is re-read
// on every mount — the enablement flag is a fail-closed security gate and must
// not be served from a stale cache when a visitor navigates to the page.
export const signupStatusQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.signupStatus(realmId),
    queryFn: async () => {
      const response = await getSignupStatus({ path: { realmId } })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: 0,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== TOTP Status ====================

export const totpStatusQueryOptions = queryOptions({
  queryKey: queryKeys.totpStatus(),
  queryFn: async () => {
    const response = await handleGetTotpStatus()
    if (response.error) throw response.error
    return response.data
  },
  retry: RETRY_COUNT,
  staleTime: STALE_TIME_2_MIN,
  gcTime: GC_TIME_5_MIN,
})

// ==================== Passkey Credentials (current user) ====================
//
// Lists the current user's registered Passkeys (`GET /api/user/passkey/credentials`).
// The generated `ListPasskeysResponse` carries `credentials: PasskeyCredentialViewResponse[]`.
export const passkeyListQueryOptions = queryOptions({
  queryKey: queryKeys.passkeyList(),
  queryFn: async () => {
    const response = await handleListPasskeyCredentials()
    if (response.error) throw response.error
    return response.data
  },
  retry: RETRY_COUNT,
  staleTime: STALE_TIME_2_MIN,
  gcTime: GC_TIME_5_MIN,
})

// ==================== Passkey Realm Config (admin) ====================
//
// Reads a realm's Passkey configuration (`GET /api/realms/{realmId}/config/passkey`).
// Requires `settings.view`; used by the admin realm-security config page.
export const passkeyRealmConfigQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.passkeyRealmConfig(realmId),
    queryFn: async () => {
      const response = await handleGetRealmPasskeyConfig({ path: { realmId } })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== Email-OTP Realm Config (admin) ====================
//
// Reads a realm's Email-OTP configuration (`GET /api/realms/{realmId}/config/email-otp`):
// the login enablement flag and the auto-registration toggle. Requires
// `settings.view`; used by the admin Settings → Security "Email code" tab.
export const emailOtpRealmConfigQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.emailOtpRealmConfig(realmId),
    queryFn: async () => {
      const response = await handleGetRealmEmailOtpConfig({ path: { realmId } })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== LDAP Realm Config (admin) ====================
//
// Reads a realm's LDAP directory config rows (`GET /api/configs/{realmId}/ldap`,
// the generic configs by-type list). Requires `settings.view`; consumed by the
// Settings → LDAP tab. `bind_password` values are masked to null server-side;
// only the row's existence matters (parsed into `hasBindPassword`).
export const ldapRealmConfigQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.ldapRealmConfig(realmId),
    queryFn: async () => {
      const response = await listRealmConfigsByType({ path: { realmId, configType: 'ldap' } })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== White-label Realm Config (admin) ====================
//
// Reads a realm's white-label management state
// (`GET /api/realms/{realmId}/config/white-label`): the published config, an
// optional draft, whether a previous version can be restored, and update
// timestamps. Requires `settings.view`; consumed by the Settings white-label tab.
export const whiteLabelRealmConfigQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.whiteLabelRealmConfig(realmId),
    queryFn: async () => {
      const response = await handleGetWhiteLabelConfig({ path: { realmId } })
      if (response.error) throw response.error
      return response.data as WhiteLabelConfigStateResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== Custom-domain Realm Config (admin) ====================
//
// Reads a realm's custom-domain management state
// (`GET /api/realms/{realmId}/config/custom-domain`): the published config, an
// optional draft, whether a previous version can be restored, the CNAME target,
// and the live CNAME/TLS status. Requires `settings.view`; consumed by the
// Settings custom-domain tab.
export const customDomainRealmConfigQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.customDomainRealmConfig(realmId),
    queryFn: async () => {
      const response = await handleGetCustomDomainConfig({ path: { realmId } })
      if (response.error) throw response.error
      return response.data as CustomDomainConfigStateResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== Subscriptions ====================

export const subscriptionQueryOptions = (realmId: string, clientAppId: string) =>
  queryOptions({
    queryKey: queryKeys.subscription(realmId, clientAppId),
    queryFn: async () => {
      const response = await getSubscriptionForClientApp({ path: { realmId, clientAppId } })
      if (response.error) {
        if (response.error.status === 404) return null
        throw response.error
      }
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const subscriptionDetailsQueryOptions = <TData>(
  realmId: string,
  subscriptionId: string,
  getCurrentState: () => TData | undefined
) =>
  queryOptions({
    queryKey: queryKeys.subscriptionDetails(realmId, subscriptionId),
    queryFn: async () => getCurrentState(),
    staleTime: STALE_TIME_2_MIN,
  })

export const userSubscriptionsQueryOptions = <TData>(
  realmId: string,
  clientAppIds: string,
  queryFn: () => Promise<TData>
) =>
  queryOptions({
    queryKey: queryKeys.userSubscriptions(realmId, clientAppIds),
    queryFn,
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

// ==================== Subscription History ====================

export async function getSubscriptionHistory(
  realmId: string,
  subscriptionId: string
): Promise<SingleSubscriptionHistoryResponse> {
  const response = await getSubscriptionHistoryApi({
    path: { realmId, subscriptionId },
  })
  return handleApiResponse(response) as SingleSubscriptionHistoryResponse
}

export async function getGlobalSubscriptionHistory(
  realmId: string,
  filters: HistoryFilters,
  page: number = 1,
  pageSize: number = 20
): Promise<GlobalSubscriptionHistoryResponse> {
  const response = await listSubscriptionHistory({
    path: { realmId },
    query: {
      ...filters,
      page,
      pageSize,
    },
  })
  return handleApiResponse(response) as GlobalSubscriptionHistoryResponse
}

export const subscriptionHistoryQueryOptions = (realmId: string, subscriptionId: string) =>
  queryOptions({
    queryKey: queryKeys.subscriptionHistory(realmId, subscriptionId),
    queryFn: () => getSubscriptionHistory(realmId, subscriptionId),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
    gcTime: GC_TIME_5_MIN,
  })

export const globalSubscriptionHistoryQueryOptions = (
  realmId: string,
  filters: HistoryFilters,
  page: number = 1,
  pageSize: number = 20
) =>
  queryOptions({
    queryKey: queryKeys.globalSubscriptionHistory(realmId, filters, page, pageSize),
    queryFn: () => getGlobalSubscriptionHistory(realmId, filters, page, pageSize),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
    // Keep the previous page visible while a filter/pagination change refetches,
    // so the list doesn't blank out between Apply Filters and the new data.
    placeholderData: keepPreviousData,
  })

// ==================== Points ====================

export const pointsWalletQueryOptions = (realmId: string, userId: string) =>
  queryOptions({
    queryKey: queryKeys.pointsWallet(realmId, userId),
    queryFn: async () =>
      handleApiResponse(await getWallet({ path: { realmId, userId } })) as PointsWalletResponse,
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

export const pointsTransactionsQueryOptions = (
  realmId: string,
  filters: {
    userId?: string
    clientAppId?: string
    subscriptionId?: string
    transactionType?: string
    bucketId?: string
    startTime?: string
    endTime?: string
    page?: number
    pageSize?: number
  }
) =>
  queryOptions({
    queryKey: queryKeys.pointsTransactions(realmId, filters),
    queryFn: async () => {
      const data = handleApiResponse(
        await listTransactions({
          path: { realmId },
          query: {
            userId: filters.userId,
            clientAppId: filters.clientAppId,
            subscriptionId: filters.subscriptionId,
            transactionType: filters.transactionType,
            bucketId: filters.bucketId,
            startTime: filters.startTime,
            endTime: filters.endTime,
            page: filters.page,
            pageSize: filters.pageSize,
          },
        })
      )
      return {
        total: data.total,
        page: data.page,
        pageSize: data.pageSize,
        transactions: data.items,
      }
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

export const userPointsTransactionsQueryOptions = (filters: {
  clientAppId?: string
  subscriptionId?: string
  transactionType?: string
  bucketId?: string
  startTime?: string
  endTime?: string
  page?: number
  pageSize?: number
}) =>
  queryOptions({
    queryKey: queryKeys.userPointsTransactions(filters),
    queryFn: async () => {
      const data = handleApiResponse(await listUserTransactions({ query: filters }))
      return {
        total: data.total,
        page: data.page,
        pageSize: data.pageSize,
        transactions: data.items,
      }
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

export const userPointsWalletsQueryOptions = queryOptions({
  queryKey: queryKeys.userPointsWallets(),
  queryFn: async () => handleApiResponse(await listUserWallets()) as ListWalletsByBucketResponse,
  retry: RETRY_COUNT,
  staleTime: STALE_TIME_2_MIN,
})

// ==================== Registration Rules ====================

export const registrationRulesQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.registrationRules(realmId),
    queryFn: async () => {
      try {
        const response = await getRegistrationRules({ path: { realmId } })
        if (response.error) handleApiErrorWithStatus(response.error)
        return response.data as RegistrationRulesResponse
      } catch (error) {
        handleApiErrorWithStatus(error)
      }
    },
    retry: clientErrorRetry,
    staleTime: STALE_TIME_5_MIN,
  })

export const updateRegistrationRulesMutation = async (
  realmId: string,
  data: UpsertRegistrationRulesRequest
) => {
  try {
    const response = await upsertRegistrationRules({
      path: { realmId },
      body: data,
    })
    if (response.error) handleApiErrorWithStatus(response.error)
    return response.data
  } catch (error) {
    handleApiErrorWithStatus(error)
  }
}

// ==================== Purchase Options (price-granularity) ====================

export const purchaseOptionsQueryOptions = (realmId: string, clientAppId: string) =>
  queryOptions({
    queryKey: queryKeys.purchaseOptions(realmId, clientAppId),
    queryFn: async () => {
      const response = await listPurchaseOptions({
        path: { realmId, clientAppId },
      })
      if (response.error) throw response.error
      return (
        (response.data as PurchaseOptionListResponse | undefined) ?? {
          items: [],
        }
      )
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

// ==================== Purchase History ====================

export interface PurchaseHistoryFilters {
  page?: number
  pageSize?: number
  paymentProvider?: string
  startDate?: string
  endDate?: string
}

export const purchaseHistoryQueryOptions = (
  realmId: string,
  filters: PurchaseHistoryFilters = {}
) =>
  queryOptions({
    queryKey: queryKeys.purchaseHistory(realmId, filters as Record<string, unknown>),
    queryFn: async () => {
      const query: Record<string, unknown> = {}
      if (filters.page !== undefined) query.page = filters.page
      if (filters.pageSize !== undefined) query.page_size = filters.pageSize
      if (filters.paymentProvider !== undefined) query.payment_provider = filters.paymentProvider
      if (filters.startDate !== undefined) query.start_date = filters.startDate
      if (filters.endDate !== undefined) query.end_date = filters.endDate

      const response = await getPurchaseHistory({
        query: query as {
          page?: number | null
          page_size?: number | null
          payment_provider?: string | null
          start_date?: string | null
          end_date?: string | null
        },
      })
      if (response.error) throw response.error
      return response.data as PurchaseHistoryResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

export const paymentAttemptStatusQueryOptions = (realmId: string, attemptId: string) =>
  queryOptions({
    queryKey: queryKeys.paymentAttemptStatus(realmId, attemptId),
    queryFn: async () => {
      if (!attemptId) {
        throw new Error('attemptId is required')
      }
      const response = await getPaymentAttemptStatus({
        path: { realmId, attemptId },
      })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: TIME_CONSTANTS.ONE_MINUTE, // More frequent updates for payment status
    refetchInterval: (query) => {
      // Handle test environment where query might be undefined or a mock
      if (!query || !query.state) {
        return false
      }
      // Poll more frequently for pending payments
      const status = query.state.data as PaymentAttemptStatusResponse | undefined
      if (status && (status.status === 'Pending' || status.status === 'RequiresAction')) {
        return TIME_CONSTANTS.ONE_MINUTE
      }
      return false
    },
  })

export const paymentProvidersQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.paymentProviders(realmId),
    queryFn: async () => {
      const response = await listPaymentProviders({
        path: { realmId },
      })
      if (response.error) throw response.error
      return response.data?.providers ?? []
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

// ==================== Audit ====================

export const auditListQueryOptions = (
  realmId: string,
  filters: {
    page?: number
    pageSize?: number
    category?: string
    action?: string
    actorId?: string
    startTime?: string
    endTime?: string
  }
) =>
  queryOptions({
    queryKey: queryKeys.audit(realmId, filters),
    queryFn: async () =>
      handleApiResponse(
        await listAuditEvents({
          path: { realmId },
          query: {
            page: filters.page ?? 0,
            pageSize: filters.pageSize ?? 20,
            category: filters.category,
            action: filters.action,
            actorId: filters.actorId,
            startTime: filters.startTime,
            endTime: filters.endTime,
          },
        })
      ),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const auditDetailQueryOptions = (realmId: string, eventId: string) =>
  queryOptions({
    queryKey: queryKeys.auditDetail(realmId, eventId),
    queryFn: async () => handleApiResponse(await getAuditEvent({ path: { realmId, eventId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

// ==================== Dashboard ====================

export const dashboardStatsQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.dashboardStats(realmId),
    queryFn: async () => {
      const response = await getDashboardStats({ path: { realmId } })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

// ==================== Email Status ====================

export const emailStatusQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.emailStatus(realmId),
    queryFn: async () => {
      const response = await emailStatus({ path: { realmId } })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

// ==================== API Keys ====================

export const apiKeysQueryOptions = (
  realmId: string,
  filters: {
    page?: number
    pageSize?: number
  }
) =>
  queryOptions({
    queryKey: queryKeys.apiKeys(realmId, filters),
    queryFn: async () =>
      handleApiResponse(
        await listApiKeys({
          path: { realmId },
          query: {
            page: filters.page ?? 0,
            pageSize: filters.pageSize ?? 20,
          },
        })
      ),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const apiKeyQueryOptions = (realmId: string, id: string) =>
  queryOptions({
    queryKey: queryKeys.apiKey(realmId, id),
    queryFn: async () => handleApiResponse(await getApiKey({ path: { realmId, apiKeyId: id } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

// ==================== API Key Roles ====================

export const adminApiKeyRolesQueryOptions = (realmId: string, apiKeyId: string) =>
  queryOptions({
    queryKey: queryKeys.apiKeyRoles(realmId, apiKeyId),
    queryFn: async () =>
      handleApiResponse(
        await adminGetApiKeyRoles({
          path: { realmId, apiKeyId },
        })
      ),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const updateApiKeyRolesMutation = async (
  realmId: string,
  apiKeyId: string,
  roleIds: string[]
) => {
  try {
    const response = await adminUpdateApiKeyRoles({
      path: { realmId, apiKeyId },
      body: { roleIds },
    })
    if (response.error) handleApiErrorWithStatus(response.error)
    return response.data
  } catch (error) {
    handleApiErrorWithStatus(error)
  }
}

// ==================== Entitlement Mappings ====================

export interface EntitlementMappingFilters {
  paymentProvider?: string
  enabled?: boolean
  page?: number
  pageSize?: number
}

export const entitlementMappingsQueryOptions = (
  realmId: string,
  filters: EntitlementMappingFilters = {}
) =>
  queryOptions({
    queryKey: queryKeys.entitlementMappings(realmId, filters as Record<string, unknown>),
    queryFn: async () => {
      const query: Record<string, unknown> = {}
      if (filters.paymentProvider !== undefined) query.paymentProvider = filters.paymentProvider
      if (filters.enabled !== undefined) query.enabled = filters.enabled
      if (filters.page !== undefined) query.page = filters.page
      if (filters.pageSize !== undefined) query.pageSize = filters.pageSize

      const response = await client.get<EntitlementMappingListResponse>({
        url: `/api/bill/${realmId}/entitlement-mappings`,
        query,
      })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

export const entitlementMappingQueryOptions = (realmId: string, mappingId: string) =>
  queryOptions({
    queryKey: queryKeys.entitlementMapping(realmId, mappingId),
    queryFn: async () => {
      const response = await getEntitlementMapping({
        path: { realmId, mappingId },
      })
      if (response.error) throw response.error
      return response.data as EntitlementMappingResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

// ==================== Admin Subscriptions ====================

export interface SubscriptionFilters {
  entitlementKey?: string
  status?: string
  paymentProvider?: string
  page?: number
  pageSize?: number
}

export const subscriptionsQueryOptions = (realmId: string, filters: SubscriptionFilters = {}) =>
  queryOptions({
    queryKey: queryKeys.subscriptions(realmId, filters as Record<string, unknown>),
    queryFn: async () => {
      const query: Record<string, unknown> = {}
      if (filters.entitlementKey !== undefined) query.entitlementKey = filters.entitlementKey
      if (filters.status !== undefined) query.status = filters.status
      if (filters.paymentProvider !== undefined) query.paymentProvider = filters.paymentProvider
      if (filters.page !== undefined) query.page = filters.page
      if (filters.pageSize !== undefined) query.pageSize = filters.pageSize

      const response = await client.get<SubscriptionListResponse>({
        url: `/api/bill/${realmId}/subscriptions`,
        query,
      })
      if (response.error) throw response.error
      return response.data
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    // Keep the previous page visible while a filter/pagination change refetches,
    // so the table doesn't blank out into the loading skeleton each time.
    placeholderData: keepPreviousData,
  })

export const subscriptionDetailQueryOptions = (realmId: string, subscriptionId: string) =>
  queryOptions({
    queryKey: queryKeys.adminSubscription(realmId, subscriptionId),
    queryFn: async () => {
      const response = await getSubscription({
        path: { realmId, subscriptionId },
      })
      if (response.error) throw response.error
      return response.data as SubscriptionDetailResponse
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

// ==================== Credit Buckets ====================

export const creditBucketsListQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.creditBucketsList(realmId),
    queryFn: async () =>
      handleApiResponse(await listCreditBucketsHandler({ path: { realmId } })) as BucketResponse[],
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

export const creditBucketDetailQueryOptions = (realmId: string, bucketId: string) =>
  queryOptions({
    queryKey: queryKeys.creditBucket(realmId, bucketId),
    queryFn: async () =>
      handleApiResponse(
        await getCreditBucketHandler({ path: { realmId, bucketId } })
      ) as BucketDetailResponse,
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

export const creditBucketOverviewQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.creditBucketOverview(realmId),
    queryFn: async () =>
      handleApiResponse(
        await getBucketOverviewHandler({ path: { realmId } })
      ) as BucketOverviewResponse,
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

/**
 * Wallets grouped by (bucket_id, user_id) for a realm — `GET /api/points/{realmId}/wallets`
 * via the generated `listWallets` SDK (returns `ListWalletsByBucketResponse`).
 *
 * Backend scoping (Gap #2 fix): the endpoint is `points.view`-gated, and the service
 * hard-scopes the result to the caller's identity.
 *   - `points.view`-only callers receive ONLY their own wallet rows (server-injected
 *     `user_id`; the client cannot target another user — `search` is stripped
 *     server-side for non-managers).
 *   - `points.manage` holders receive the full realm-wide (cross-user) set.
 *   - the user points page still client-filters `items` by the current `userId`
 *     via `deriveUserPointsView` — now a defensive no-op for view-only callers, kept
 *     because it is harmless and still correct.
 *   - admin wallets consumes the full `items` + `crossBucketTotal`.
 *
 * For a `points.view`-only caller `crossBucketTotal` is that user's own cross-bucket
 * total; for a `points.manage` caller it is the realm-wide cross-user total.
 */
export const walletsByBucketQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.walletsByBucket(realmId),
    queryFn: async () =>
      handleApiResponse(await listWallets({ path: { realmId } })) as ListWalletsByBucketResponse,
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

// ==================== Legal / Consent / Account Deletion ====================

/**
 * Convert agreement summaries (snake_case wire shape) into the camelCase
 * `{ agreementType, versionId }` pairs expected by login/TOTP retry requests.
 */
export function toAuthConsentAgreements(
  agreements: LegalAgreementSummary[]
): Array<{ agreementType: string; versionId: string }> {
  return agreements.map((agreement) => ({
    agreementType: agreement.agreement_type,
    versionId: agreement.version_id,
  }))
}

/**
 * Convert agreement summaries into the snake_case `RecordConsentRequest` body
 * used by `POST /api/legal/{realmId}/consent`.
 */
export function toRecordConsentRequest(agreements: LegalAgreementSummary[]): RecordConsentRequest {
  return {
    agreements: agreements.map((agreement) => ({
      agreement_type: agreement.agreement_type,
      version_id: agreement.version_id,
    })),
  }
}

/**
 * Build a `RecordConsentRequest` from the current `ConsentStatusItem` rows.
 * Posts the realm's current effective version ids for every pending item.
 */
export function toRecordConsentRequestFromStatus(items: ConsentStatusItem[]): RecordConsentRequest {
  return {
    agreements: items.map((item) => ({
      agreement_type: item.agreement_type,
      version_id: item.current_version_id,
    })),
  }
}

export const legalAgreementsQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.legalAgreements(realmId),
    queryFn: async () => handleApiResponse(await listAgreements({ path: { realmId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const legalAgreementQueryOptions = (
  realmId: string,
  agreementType: string,
  locale?: string
) =>
  queryOptions({
    queryKey: queryKeys.legalAgreement(realmId, agreementType, locale),
    queryFn: async () => {
      const request = locale
        ? { path: { realmId, agreementType }, query: { locale } }
        : { path: { realmId, agreementType } }
      return handleApiResponse(await getAgreement(request))
    },
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const consentStatusQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.consentStatus(realmId),
    queryFn: async () => handleApiResponse(await getConsentStatus({ path: { realmId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
    gcTime: GC_TIME_5_MIN,
  })

export const legalAdminAgreementsQueryOptions = (realmId: string) =>
  queryOptions({
    queryKey: queryKeys.legalAdminAgreements(realmId),
    queryFn: async () => handleApiResponse(await adminListAgreements({ path: { realmId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_2_MIN,
  })

/// Past-version detail for the admin "view version" dialog. Lazily fetched on
/// demand (the history list only carries summaries; the full body is pulled
/// when an admin opens a specific version). Callers pass `enabled` to gate the
/// fetch until a version is selected.
export const legalVersionQueryOptions = (realmId: string, versionId: string) =>
  queryOptions({
    queryKey: queryKeys.legalVersion(realmId, versionId),
    queryFn: async () => handleApiResponse(await adminGetVersion({ path: { realmId, versionId } })),
    retry: RETRY_COUNT,
    staleTime: STALE_TIME_5_MIN,
  })

export const recordConsentMutation = async (
  realmId: string,
  data: RecordConsentRequest
): Promise<void> => {
  const response = await recordConsent({ path: { realmId }, body: data })
  if (response.error) throw response.error
}

export const deleteAccountMutation = async (password: string): Promise<void> => {
  // High-risk op (delete_account): obtain a single-use reauth ticket using the
  // user's password, then submit account deletion with it.
  const reauthToken = await obtainReauthToken('delete_account', password)
  const response = await deleteAccount({ body: { reauthToken } })
  if (response.error) throw response.error
}

export const publishCustomAgreementMutation = async (
  realmId: string,
  agreementType: string,
  data: PublishCustomRequest
): Promise<PublishVersionResponse> => {
  const response = await adminPublishCustom({
    path: { realmId, agreementType },
    body: data,
  })
  if (response.error) throw response.error
  return response.data as PublishVersionResponse
}

export const revertToDefaultAgreementMutation = async (
  realmId: string,
  agreementType: string
): Promise<PublishVersionResponse> => {
  const response = await adminRevertToDefault({ path: { realmId, agreementType } })
  if (response.error) throw response.error
  return response.data as PublishVersionResponse
}

/// Draft query: returns the staged draft, or `null` when none exists (a 404
/// from the backend — "no draft saved for this type" — is treated as the
/// normal "no draft yet" state, not an error).
///
/// The 404 is folded to `null` inside `queryFn`, so it never reaches `retry`.
/// Transient errors (500, network) should still get the standard one-shot retry
/// — a flake must not present as "no draft" and silently blank the admin form.
export const legalDraftQueryOptions = (realmId: string, agreementType: string) =>
  queryOptions({
    queryKey: queryKeys.legalDraft(realmId, agreementType),
    queryFn: async () => {
      const response = await adminGetDraft({ path: { realmId, agreementType } })
      // A 404 ("no draft saved for this type") is the normal "no draft yet"
      // state, not an error. Inspect the raw response error's status before
      // `handleApiResponse` would wrap it as a plain Error (which loses the
      // status code). Any other error is surfaced normally.
      const err = response.error as { status?: number } | undefined
      if (err?.status === 404) return null
      return handleApiResponse(response) as LegalAgreementDraftResponse
    },
    retry: clientErrorRetry,
    staleTime: STALE_TIME_2_MIN,
  })

export const saveDraftMutation = async (
  realmId: string,
  agreementType: string,
  data: SaveDraftRequest
): Promise<LegalAgreementDraftResponse> => {
  const response = await adminSaveDraft({ path: { realmId, agreementType }, body: data })
  if (response.error) throw response.error
  return response.data as LegalAgreementDraftResponse
}

export const discardDraftMutation = async (
  realmId: string,
  agreementType: string
): Promise<void> => {
  const response = await adminDiscardDraft({ path: { realmId, agreementType } })
  if (response.error) throw response.error
}

/// Publish the staged draft. `versionLabelOverride` optionally replaces the
/// draft's label for this publish only; when omitted the draft's stored label
/// is used. Returns the newly published version identifiers.
export const publishFromDraftMutation = async (
  realmId: string,
  agreementType: string,
  versionLabelOverride?: string | null
): Promise<PublishVersionResponse> => {
  const body = versionLabelOverride !== undefined ? { version_label: versionLabelOverride } : {}
  const response = await adminPublishFromDraft({ path: { realmId, agreementType }, body })
  if (response.error) throw response.error
  return response.data as PublishVersionResponse
}
