/**
 * herald-sdk — Herald official Node.js SDK (server-side).
 *
 * TypeScript counterpart of the Rust `herald-sdk` crate: an API-key client for
 * Herald's external API (permission checks with caching, subscriptions,
 * points, realms/users/client-apps). Zero runtime dependencies, Node 18+
 * native fetch. For browser end-user authentication use `herald-auth-web`.
 */

// Client.
export { HeraldClient } from './client'

// Errors.
export { HeraldSdkError } from './errors'
export type { HeraldSdkErrorCode } from './errors'

// Wire types (1:1 with the Rust crate's public structs).
export type {
  AdminUserSdkInput,
  AdminUserSdkOutput,
  AllocationDetail,
  BalancesByType,
  BucketTransaction,
  ClientAppInfo,
  ClientAppItem,
  ConsumePointsResponse,
  CreateClientAppSdkRequest,
  CreateRealmSdkRequest,
  CreateUserSdkRequest,
  GrantPointsResponse,
  PermissionCheckRequest,
  PermissionCheckResponse,
  PointsBalanceResponse,
  QuotaWindowView,
  RealmInfo,
  RealmItem,
  Rule,
  SubscriptionDetail,
  UserInfo,
  WalletByBucket,
} from './types'
