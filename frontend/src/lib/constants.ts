/**
 * Shared constants used across the application
 */

/**
 * Default product/brand name shown as a last-resort fallback when a realm has
 * no white-label logo, title, or realm name configured. Kept in one place so a
 * rename only touches this constant.
 */
export const BRAND_NAME = 'Herald'

/**
 * Default page size for paginated lists
 */
export const DEFAULT_PAGE_SIZE = 20

/**
 * Low balance threshold for points wallets
 * Wallets with balance below this value may trigger warnings
 */
export const LOW_POINTS_THRESHOLD = 100

/**
 * Time constants in milliseconds
 */
export const TIME_CONSTANTS = {
  ONE_MINUTE: 60 * 1000,
  FIVE_MINUTES: 5 * 60 * 1000,
  TWO_MINUTES: 2 * 60 * 1000,
} as const

/**
 * Shared React Query timing defaults so data modules don't each
 * re-derive them from TIME_CONSTANTS.
 */
export const QUERY_TIMING = {
  GC_TIME_5_MIN: TIME_CONSTANTS.FIVE_MINUTES,
  STALE_TIME_2_MIN: TIME_CONSTANTS.TWO_MINUTES,
  STALE_TIME_5_MIN: TIME_CONSTANTS.FIVE_MINUTES,
  RETRY_COUNT: 1,
} as const

/**
 * Query cache keys for React Query
 * These should be used consistently across the application
 */
export const QUERY_KEYS = {
  PUBLIC_CONFIG: 'public-config',
  REALMS: 'realms',
  REALM: 'realm',
  USERS: 'users',
  USER: 'user',
  PERMISSIONS: 'permissions',
  PERMISSION: 'permission',
  ROLES: 'roles',
  ROLE: 'role',
  ROLE_PERMISSIONS: 'role-permissions',
  ADMIN_USER_ROLES: 'admin-user-roles',
  USER_ROLES: 'user-roles',
  USER_SESSIONS: 'user-sessions',
  CLIENT_APPS: 'client-apps',
  CLIENT_APP: 'client-app',
  OAUTH_CONFIGS: 'oauth-configs',
  PROFILE: 'profile',
  TOTP_STATUS: 'totp-status',
  PASSKEY_LIST: 'passkey-list',
  PASSKEY_REALM_CONFIG: 'passkey-realm-config',
  WHITE_LABEL_REALM_CONFIG: 'white-label-realm-config',
  CUSTOM_DOMAIN_REALM_CONFIG: 'custom-domain-realm-config',
  TURNSTILE_STATUS: 'turnstile-status',
  EMAIL_OTP_STATUS: 'email-otp-status',
  PASSKEY_STATUS: 'passkey-status',
  SIGNUP_STATUS: 'signup-status',
  EMAIL_OTP_REALM_CONFIG: 'email-otp-realm-config',
  LDAP_STATUS: 'ldap-status',
  LDAP_REALM_CONFIG: 'ldap-realm-config',
  USER_SUBSCRIPTIONS: 'user-subscriptions',
  SUBSCRIPTION_DETAILS: 'subscription-details',
  SUBSCRIPTION: 'subscription',
  SUBSCRIPTION_HISTORY: 'subscription-history',
  GLOBAL_SUBSCRIPTION_HISTORY: 'global-subscription-history',
  POINTS_WALLETS: 'points-wallets',
  POINTS_WALLET: 'points-wallet',
  POINTS_TRANSACTIONS: 'points-transactions',
  PURCHASE_OPTIONS: 'purchase-options',
  PURCHASE_HISTORY: 'purchase-history',
  PAYMENT_ATTEMPT_STATUS: 'payment-attempt-status',
  PAYMENT_PROVIDERS: 'payment-providers',
  REGISTRATION_RULES: 'registration-rules',
  REALM_CONFIGS: 'realm-configs',
  EMAIL_STATUS: 'email-status',
  AUDIT_EVENTS: 'audit-events',
  AUDIT_EVENT: 'audit-event',
  DASHBOARD_STATS: 'dashboard-stats',
  FEATURE_AVAILABILITY: 'feature-availability',
  USER_FEATURE_AVAILABILITY: 'user-feature-availability',
  USER_POINTS_TRANSACTIONS: 'user-points-transactions',
  USER_POINTS_WALLETS: 'user-points-wallets',
  API_KEYS: 'api-keys',
  API_KEY: 'api-key',
  API_KEY_ROLES: 'api-key-roles',
  ENTITLEMENT_MAPPINGS: 'entitlement-mappings',
  ENTITLEMENT_MAPPING: 'entitlement-mapping',
  ADMIN_SUBSCRIPTIONS: 'admin-subscriptions',
  ADMIN_SUBSCRIPTION: 'admin-subscription',
  CREDIT_BUCKETS: 'credit-buckets',
  CREDIT_BUCKET: 'credit-bucket',
  CREDIT_BUCKET_OVERVIEW: 'credit-bucket-overview',
  WALLETS_BY_BUCKET: 'wallets-by-bucket',
  LEGAL_AGREEMENTS: 'legal-agreements',
  LEGAL_AGREEMENT: 'legal-agreement',
  LEGAL_ADMIN_AGREEMENTS: 'legal-admin-agreements',
  LEGAL_DRAFT: 'legal-draft',
  CONSENT_STATUS: 'consent-status',
} as const

/**
 * Filter constant for "all" option in dropdowns
 */
export const FILTER_ALL_VALUE = '__all__'

/**
 * UTC time boundary suffixes
 */
export const UTC_TIME_BOUNDARIES = {
  START: 'T00:00:00.000Z',
  END: 'T23:59:59.999Z',
} as const
