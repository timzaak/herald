/**
 * herald-sdk — Herald official Node.js SDK (server-side).
 *
 * TypeScript counterpart of the Rust `herald-sdk` crate: an API-key client for
 * Herald's external API (permission checks with caching, subscriptions,
 * points, realms/users/client-apps). Zero runtime dependencies, Node 18+
 * native fetch. For browser end-user authentication use `herald-auth-web`.
 */

export { HeraldClient } from './client'

export { HeraldSdkError } from './errors'
export type { HeraldSdkErrorCode } from './errors'

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
  CreateOrgSdkRequest,
  CreateRealmSdkRequest,
  CreateUserSdkRequest,
  GrantPointsResponse,
  OrgCheckRule,
  OrgInfo,
  OrgMemberInfo,
  OrgMemberListItem,
  OrgMemberLookup,
  OrgMemberPage,
  OrgPermissionCheckResult,
  OrgPermissionCheckSdkRequest,
  OrgRoleRef,
  PermissionCheckRequest,
  PermissionCheckResponse,
  PointsBalanceResponse,
  QuotaWindowView,
  RealmInfo,
  RealmItem,
  Rule,
  SubscriptionDetail,
  SubscriptionStatus,
  TransactionDetail,
  UpsertOrgMemberSdkRequest,
  UserInfo,
  UserOrgListItem,
  UserOrgPage,
  UserPage,
  UserStatus,
  WalletByBucket,
} from './types'

export {
  HeraldWebhookError,
  WEBHOOK_HEADER_DELIVERY_ID,
  WEBHOOK_HEADER_EVENT_ID,
  WEBHOOK_HEADER_EVENT_TYPE,
  WEBHOOK_HEADER_SIGNATURE,
  WEBHOOK_HEADER_TIMESTAMP,
  verifyAndParseWebhookEvent,
  verifyWebhookSignature,
} from './webhooks'
export type {
  HeraldWebhookEnvelope,
  HeraldWebhookErrorCode,
  HeraldWebhookEvent,
  RoleWebhookEventData,
  SubscriptionWebhookEventData,
  UserWebhookEventData,
  VerifyWebhookSignatureOptions,
  WebhookEventCategory,
  WebhookHeadersInput,
  WebhookRoleEventSource,
} from './webhooks'
