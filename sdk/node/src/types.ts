/**
 * Wire types for the Herald external API (`/api/ext/*`).
 *
 * Ported 1:1 from the Rust `herald-sdk` crate (`sdk/rust/src/lib.rs`), which
 * is the source of truth for this SDK: field names follow the backend's
 * camelCase JSON contract, optionality mirrors the Rust `Option` fields (fields
 * the backend always emits as `null` are typed `| null`; fields the backend
 * omits when absent are optional `?`).
 */

// --- Permission check ---

export interface Rule {
  resource: string
  action: string
}

/** `POST /api/ext/permission/check` request. */
export interface PermissionCheckRequest {
  /** Browser access token issued by `/api/auth/{realmId}/login`. */
  accessToken: string
  rules?: Rule[]
  clientId: string
}

export interface PermissionCheckResponse {
  allowed: boolean
  userId?: string
}

// --- Billing / subscription ---

export interface SubscriptionDetail {
  id: string
  clientAppId: string | null
  status: string
  entitlementKey: string
  paymentProvider: string
  /** Provider price id bound to this subscription; omitted for price-less
   * providers (Creem) or when the subscription has no bound price yet. */
  externalPriceId?: string
  currentPeriodStart: string | null
  currentPeriodEnd: string | null
  cancelAt: string | null
  cancelAtPeriodEnd: boolean | null
  createdAt: string
  updatedAt: string
}

// --- Points ---

export interface PointsBalanceResponse {
  userId: string
  balance: number
  totalPaidGranted?: number
  totalRecharged: number
  totalConsumed: number
  unit: string
  updatedAt: string
}

/** Per-bucket transaction inside a multi-bucket consume response.
 *
 * Single-pool consume → `transactions` has length 1 (structure unified with
 * the multi-bucket case). `amount` is the deduction magnitude (positive). */
export interface BucketTransaction {
  transactionId: string
  bucketId: string
  walletId: string
  userId: string
  amount: number
  balanceAfter: number
}

/** Ledger-level allocation detail for a consume. */
export interface AllocationDetail {
  bucketId: string
  walletId: string
  ledgerId: string
  creditType: string
  allocatedAmount: number
}

/** Points consume response (per-bucket multi-transaction shape). */
export interface ConsumePointsResponse {
  userId: string
  amount: number
  correlationId: string
  transactions: BucketTransaction[]
  allocations: AllocationDetail[]
}

/** Points grant response. */
export interface GrantPointsResponse {
  transactionId: string
  userId: string
  bucketId: string
  amount: number
  grantedBalance: number
  balance: number
  expiresAt?: string
}

/** Per-credit-type balances (`balancesByType`). */
export interface BalancesByType {
  topup?: number
  subscription?: number
  registration?: number
  freePeriodic?: number
  granted?: number
}

/** Quota window read view (`QuotaWindowView`).
 *
 * One row per distinct window `key` for a (user, bucket). `key` is the stable
 * display identity derived from the window length (e.g. `5h`/`week`/`month`),
 * NOT a row ordinal. `isTightest` flags the minimum-remaining window (the
 * spendable-from-quota constraint); `exhausted` flags `remaining == 0`.
 * `resetsAt` is an ISO8601 string (matches the SDK's string-date convention). */
export interface QuotaWindowView {
  /** Stable display key (config-derived, not row ordinal). */
  key: string
  limit: number
  used: number
  remaining: number
  /** Sliding window length in seconds (month ≈ 30d). */
  windowSeconds: number
  /** Approximate next reset point of the window (ISO8601); omitted when no
   * consume has occurred in the window yet. */
  resetsAt?: string
  /** True if this window is the minimum-remaining (tightest) constraint. */
  isTightest: boolean
  /** True if `remaining == 0`. */
  exhausted: boolean
}

/** Wallet balances grouped by Credit Bucket (`WalletByBucket`).
 *
 * For the admin (`billing/points/wallets`) view, `userId` is populated and
 * rows group per `(user, bucket)`; for the `users/me/points/wallets` view,
 * `userId` is the calling user. */
export interface WalletByBucket {
  bucketId?: string | null
  name?: string | null
  enabled?: boolean | null
  userId: string
  balancesByType: BalancesByType
  /** Currently spendable total for this bucket = window-available
   * (`spendableFromQuota`) + pool balance (`spendableFromPool`). */
  bucketTotal: number
  /** Per-window quota view for this (user, bucket); omitted for a pool-only
   * bucket (no active subscription / free-periodic quota entitlement). */
  quotaWindows?: QuotaWindowView[]
  /** Window-quota available amount = minimum `remaining` across
   * `quotaWindows` (the tightest constraint); omitted for pool-only buckets. */
  spendableFromQuota?: number
  /** Pool-side balance sum (topup + registration + granted credit types) for
   * this bucket; omitted for window-only buckets with no pool balance. */
  spendableFromPool?: number
}

// --- Realms ---

export interface AdminUserSdkInput {
  email: string
  password: string
}

/** Request body for creating a realm. */
export interface CreateRealmSdkRequest {
  name: string
  description?: string | null
  adminUser: AdminUserSdkInput
}

export interface AdminUserSdkOutput {
  id: string
  email: string
  role: string
}

/** Realm detail (create/get response). */
export interface RealmInfo {
  id: string
  name: string
  description: string | null
  adminUser: AdminUserSdkOutput | null
  createdAt: string
  updatedAt: string
}

/** Realm list item. */
export interface RealmItem {
  id: string
  name: string
  description: string | null
  createdAt: string
  updatedAt: string
}

// --- Users ---

/** Request body for creating a user. */
export interface CreateUserSdkRequest {
  email: string
  password: string
  nickname?: string | null
}

/** User info (create/get/list response). */
export interface UserInfo {
  id: string
  email: string
  nickname: string | null
  status: number
  createdAt: string
}

// --- Client apps ---

/** Request body for creating a client app. */
export interface CreateClientAppSdkRequest {
  name: string
  description?: string | null
  redirectUris: string[]
}

/** Client app detail (create/get response). */
export interface ClientAppInfo {
  id: string
  clientId: string
  clientSecret: string | null
  name: string
  description: string | null
  redirectUris: string[]
  enabled: boolean
  createdAt: string
}

/** Client app list item. */
export interface ClientAppItem {
  id: string
  clientId: string
  name: string
  enabled: boolean
  createdAt: string
}
