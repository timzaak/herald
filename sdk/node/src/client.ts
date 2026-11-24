/**
 * Herald server SDK client — a 1:1 TypeScript port of the Rust `herald-sdk`
 * crate (`sdk/rust/src/lib.rs`), which is this SDK's source of truth.
 *
 * Auth model: every call carries the realm/service `X-API-Key` header against
 * the external API surface (`/api/ext/*`). `checkPermission` additionally
 * mirrors the Rust caching behaviour exactly:
 *
 *   - a TTL cache keyed by the full request (token + rules);
 *   - a token→keys index so `invalidateCache(token)` drops every cached check
 *     for that token;
 *   - a 300s "token snapshot is stale" heuristic: once a token has been seen
 *     at least once more than 5 minutes ago, its cached entries are
 *     invalidated before the next check (Rust `is_token_expired`).
 *
 * HTTP layer: native `fetch` (Node 18+), hand-rolled — same deliberate choice
 * as the Rust crate (no OpenAPI-generated client), keeping the package at zero
 * runtime dependencies and its types in lockstep with the crate's.
 */

import { HeraldSdkError } from './errors'
import type {
  ClientAppInfo,
  ClientAppItem,
  ConsumePointsResponse,
  CreateClientAppSdkRequest,
  CreateOrgSdkRequest,
  CreateRealmSdkRequest,
  CreateUserSdkRequest,
  GrantPointsResponse,
  OrgInfo,
  OrgMemberInfo,
  OrgMemberLookup,
  OrgMemberPage,
  OrgPermissionCheckResult,
  OrgPermissionCheckSdkRequest,
  PermissionCheckRequest,
  PermissionCheckResponse,
  PointsBalanceResponse,
  RealmInfo,
  RealmItem,
  SubscriptionDetail,
  TransactionDetail,
  UpsertOrgMemberSdkRequest,
  UserInfo,
  UserOrgPage,
  UserPage,
} from './types'

/** Rust `is_token_expired` threshold: 5 minutes, independent of the cache TTL. */
const TOKEN_EXPIRY_THRESHOLD_MS = 300_000

interface CacheEntry {
  response: PermissionCheckResponse
  /** Lazy TTL: `Date.now()` after which the entry is treated as evicted. */
  expiresAtMs: number
}

/**
 * Cache key for a permission check. An empty `rules: []` is a valid key and
 * rule order is significant — `JSON.stringify` preserves both distinctions.
 * (The Rust cache key is the whole `PermissionCheckRequest`; `clientId` was
 * removed from the request because the backend body has no such field — the
 * client identity travels in the `X-API-Key` header.)
 */
function permissionCacheKey(req: PermissionCheckRequest): string {
  return JSON.stringify({ accessToken: req.accessToken, rules: req.rules })
}

/** Map a `fetch` response onto the Rust `handle_response` semantics. */
async function handleResponse<T>(response: Response): Promise<T> {
  let text: string
  try {
    text = await response.text()
  } catch (cause) {
    // Body-phase transport failure (e.g. AbortSignal.timeout firing after the
    // headers arrived). sendRequest's catch only covers fetch itself, but the
    // Rust timeout spans the whole request including the body.
    throw new HeraldSdkError('network', undefined, String(cause))
  }
  const status = response.status
  if (status === 401) throw new HeraldSdkError('unauthorized', status, text)
  if (status === 403) throw new HeraldSdkError('forbidden', status, text)
  if (status === 404) throw new HeraldSdkError('not-found', status, text)
  if (status === 500) throw new HeraldSdkError('internal-server-error', status, text)
  if (status >= 200 && status < 300) {
    try {
      return JSON.parse(text) as T
    } catch (cause) {
      throw new HeraldSdkError('parse', status, `invalid JSON body: ${String(cause)}`)
    }
  }
  throw new HeraldSdkError('api-error', status, text)
}

interface RequestOptions {
  query?: Record<string, string>
  body?: unknown
}

export class HeraldClient {
  private readonly baseUrl: string
  private readonly apiKey: string
  private readonly cacheTtlMs: number
  private timeoutMs?: number
  private readonly permissionCache = new Map<string, CacheEntry>()
  /** token → cache keys for its checks (Rust `token_index: DashMap`). */
  private readonly tokenIndex = new Map<string, Set<string>>()
  /** token → last successful check timestamp (Rust `token_cache`; the Rust
   * tuple also stored the response, but only the timestamp is ever read). */
  private readonly tokenLastSeen = new Map<string, number>()

  constructor(baseUrl: string, apiKey: string, cacheTtlSeconds?: number) {
    this.baseUrl = baseUrl.replace(/\/+$/, '')
    this.apiKey = apiKey
    this.cacheTtlMs = (cacheTtlSeconds ?? 300) * 1000
  }

  /**
   * Set the HTTP timeout in milliseconds for every request sent through this
   * client (mirrors Rust `Client::with_http_timeout`). The setting persists
   * on this instance and every reference to it — unlike the Rust builder,
   * which consumes `self`, this mutates the shared client, so build a
   * separate `HeraldClient` if only some calls need a different timeout.
   * Without this, `fetch` relies on the runtime's default timeouts; a
   * timed-out request rejects with `HeraldSdkError('network')`.
   */
  withHttpTimeout(timeoutMs: number): this {
    // AbortSignal.timeout throws ERR_OUT_OF_RANGE above 2^31-1 ms — reject
    // here so a config bug fails at configuration time, not as a per-request
    // 'network' error.
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || timeoutMs > 2_147_483_647) {
      throw new RangeError('timeoutMs must be a positive finite number (max 2^31-1 ms)')
    }
    this.timeoutMs = timeoutMs
    return this
  }

  private async sendRequest(
    method: 'GET' | 'POST' | 'DELETE',
    path: string,
    options: RequestOptions = {},
  ): Promise<Response> {
    let url = `${this.baseUrl}${path}`
    if (options.query) {
      const search = new URLSearchParams(options.query).toString()
      if (search) url += `?${search}`
    }
    const headers: Record<string, string> = { 'X-API-Key': this.apiKey }
    if (options.body !== undefined) headers['Content-Type'] = 'application/json'

    try {
      return await fetch(url, {
        method,
        headers,
        body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
        signal: this.timeoutMs !== undefined ? AbortSignal.timeout(this.timeoutMs) : undefined,
      })
    } catch (cause) {
      throw new HeraldSdkError('network', undefined, String(cause))
    }
  }

  private async requestJson<T>(
    method: 'GET' | 'POST',
    path: string,
    options: RequestOptions = {},
  ): Promise<T> {
    const response = await this.sendRequest(method, path, options)
    return handleResponse<T>(response)
  }

  private async requestVoid(
    method: 'DELETE',
    path: string,
    options: RequestOptions = {},
  ): Promise<void> {
    const response = await this.sendRequest(method, path, options)
    // 204 carries an empty body — nothing to deserialize.
    if (response.status === 204) return
    await handleResponse<void>(response)
  }

  private pageQuery(page?: number, pageSize?: number): Record<string, string> {
    const query: Record<string, string> = {}
    if (page !== undefined) query.page = String(page)
    if (pageSize !== undefined) query.pageSize = String(pageSize)
    return query
  }

  /**
   * Check whether a user (identified by their browser access token) is allowed
   * an action. Results are cached per exact request for the client's TTL;
   * a token not checked for over 5 minutes has its cache invalidated first.
   */
  async checkPermission(req: PermissionCheckRequest): Promise<PermissionCheckResponse> {
    if (this.isTokenExpired(req.accessToken)) {
      this.invalidateCache(req.accessToken)
    }

    const cached = this.getCached(req)
    if (cached) return cached

    const response = await this.requestJson<PermissionCheckResponse>('POST', '/api/ext/permission/check', {
      body: req,
    })

    const now = Date.now()
    this.tokenLastSeen.set(req.accessToken, now)
    const key = permissionCacheKey(req)
    let keys = this.tokenIndex.get(req.accessToken)
    if (!keys) {
      keys = new Set()
      this.tokenIndex.set(req.accessToken, keys)
    }
    keys.add(key)
    this.permissionCache.set(key, { response, expiresAtMs: now + this.cacheTtlMs })
    return response
  }

  /** Rust `is_token_expired`: seen before, and more than 5 minutes ago. */
  private isTokenExpired(token: string): boolean {
    const at = this.tokenLastSeen.get(token)
    return at !== undefined && Date.now() - at > TOKEN_EXPIRY_THRESHOLD_MS
  }

  /** Cached response for an exact request, lazily evicting expired entries
   * (the Map counterpart of moka's TTL + eviction listener). */
  private getCached(req: PermissionCheckRequest): PermissionCheckResponse | undefined {
    const key = permissionCacheKey(req)
    const entry = this.permissionCache.get(key)
    if (!entry) return undefined
    if (Date.now() > entry.expiresAtMs) {
      this.permissionCache.delete(key)
      this.dropIndexEntry(req.accessToken, key)
      return undefined
    }
    return entry.response
  }

  private dropIndexEntry(token: string, key: string): void {
    const keys = this.tokenIndex.get(token)
    if (!keys) return
    keys.delete(key)
    if (keys.size === 0) this.tokenIndex.delete(token)
  }

  /** Drop every cached permission check for a token (e.g. after you know the
   * user's permissions changed). Unlike the Rust crate this is synchronous —
   * `await` it if you prefer; the return value is `void` either way. */
  invalidateCache(token: string): void {
    const keys = this.tokenIndex.get(token)
    if (!keys) return
    for (const key of keys) this.permissionCache.delete(key)
    this.tokenIndex.delete(token)
  }

  /** Subscription detail for a client app. */
  getSubscription(realmId: string, clientAppId: string): Promise<SubscriptionDetail> {
    return this.requestJson('GET', `/api/ext/bill/${encodeURIComponent(realmId)}/client/${encodeURIComponent(clientAppId)}/subscription`)
  }

  /** User points balance. */
  getBalance(realmId: string, userId: string): Promise<PointsBalanceResponse> {
    return this.requestJson('GET', `/api/ext/points/${encodeURIComponent(realmId)}/balance`, {
      query: { userId },
    })
  }

  /** Consume points from a user's account. `idempotencyKey` prevents double
   * charges when the same logical consume is retried. */
  consumePoints(
    realmId: string,
    userId: string,
    clientAppId: string,
    amount: number,
    description?: string,
    idempotencyKey?: string,
  ): Promise<ConsumePointsResponse> {
    return this.requestJson('POST', `/api/ext/points/${encodeURIComponent(realmId)}/consume`, {
      body: {
        userId,
        clientAppId,
        amount,
        description,
        idempotencyKey,
      },
    })
  }

  /** Grant points to a user. `bucketId` is REQUIRED: every grant must target
   * an explicit Credit Bucket. `reason` must be non-empty; `validityDays`
   * omitted means permanent. */
  grantPoints(
    realmId: string,
    userId: string,
    bucketId: string,
    amount: number,
    reason: string,
    validityDays?: number,
  ): Promise<GrantPointsResponse> {
    return this.requestJson('POST', `/api/ext/points/${encodeURIComponent(realmId)}/grant`, {
      body: {
        userId,
        bucketId,
        amount,
        reason,
        validityDays,
      },
    })
  }

  /** Single points transaction detail by transaction ID. */
  getTransaction(realmId: string, transactionId: string): Promise<TransactionDetail> {
    return this.requestJson(
      'GET',
      `/api/ext/points/${encodeURIComponent(realmId)}/transactions/${encodeURIComponent(transactionId)}`,
    )
  }

  createRealm(request: CreateRealmSdkRequest): Promise<RealmInfo> {
    return this.requestJson('POST', '/api/ext/realms', { body: request })
  }

  async listRealms(): Promise<RealmItem[]> {
    const body = await this.requestJson<{ realms: RealmItem[] }>('GET', '/api/ext/realms')
    return body.realms
  }

  getRealm(realmId: string): Promise<RealmInfo> {
    return this.requestJson('GET', `/api/ext/realms/${encodeURIComponent(realmId)}`)
  }

  createUser(realmId: string, request: CreateUserSdkRequest): Promise<UserInfo> {
    return this.requestJson('POST', `/api/ext/realms/${encodeURIComponent(realmId)}/users`, { body: request })
  }

  /** List users in a realm, first page with the server defaults
   *  (page = 1, pageSize = 20). Use {@link listUsersPage} to page further. */
  async listUsers(realmId: string): Promise<UserInfo[]> {
    return (await this.listUsersPage(realmId)).items
  }

  /**
   * List users in a realm with pagination. `page` is 1-based and defaults to
   * 1 when undefined; `pageSize` defaults to 20 and is clamped to 100 by the
   * server. The response echoes the applied paging and the cross-page total.
   */
  async listUsersPage(realmId: string, page?: number, pageSize?: number): Promise<UserPage> {
    return this.requestJson<UserPage>(
      'GET',
      `/api/ext/realms/${encodeURIComponent(realmId)}/users`,
      { query: this.pageQuery(page, pageSize) },
    )
  }

  getUser(realmId: string, userId: string): Promise<UserInfo> {
    return this.requestJson(
      'GET',
      `/api/ext/realms/${encodeURIComponent(realmId)}/users/${encodeURIComponent(userId)}`,
    )
  }

  createClientApp(realmId: string, request: CreateClientAppSdkRequest): Promise<ClientAppInfo> {
    return this.requestJson('POST', `/api/ext/realms/${encodeURIComponent(realmId)}/client-apps`, {
      body: request,
    })
  }

  async listClientApps(realmId: string): Promise<ClientAppItem[]> {
    const body = await this.requestJson<{ clientApps: ClientAppItem[] }>(
      'GET',
      `/api/ext/realms/${encodeURIComponent(realmId)}/client-apps`,
    )
    return body.clientApps
  }

  getClientApp(realmId: string, clientAppId: string): Promise<ClientAppInfo> {
    return this.requestJson(
      'GET',
      `/api/ext/realms/${encodeURIComponent(realmId)}/client-apps/${encodeURIComponent(clientAppId)}`,
    )
  }

  /**
   * Create an organization (idempotent on `(clientApp, externalId)`).
   *
   * The first creation for a `(realm, clientApp)` also seeds the three
   * built-in tiers (`org-owner` / `org-admin` / `org-member`). A replay
   * answers 200 with `created: false`.
   */
  createOrg(realmId: string, request: CreateOrgSdkRequest): Promise<OrgInfo> {
    return this.requestJson('POST', `/api/ext/realms/${encodeURIComponent(realmId)}/orgs`, {
      body: request,
    })
  }

  /** Delete an organization (memberships cascade). Repeated delete throws `not-found`. */
  deleteOrg(realmId: string, orgId: string): Promise<void> {
    return this.requestVoid(
      'DELETE',
      `/api/ext/realms/${encodeURIComponent(realmId)}/orgs/${encodeURIComponent(orgId)}`,
    )
  }

  /**
   * Add or change an organization member (idempotent upsert).
   *
   * Granting the `org-owner` tier performs an atomic ownership transfer.
   */
  upsertOrgMember(
    realmId: string,
    orgId: string,
    request: UpsertOrgMemberSdkRequest,
  ): Promise<OrgMemberInfo> {
    return this.requestJson(
      'POST',
      `/api/ext/realms/${encodeURIComponent(realmId)}/orgs/${encodeURIComponent(orgId)}/members`,
      { body: request },
    )
  }

  /** Remove an organization member (owner and last admin are protected). */
  removeOrgMember(
    realmId: string,
    orgId: string,
    userId: string,
    actorUserId?: string,
  ): Promise<void> {
    const query: Record<string, string> = {}
    if (actorUserId !== undefined) query.actorUserId = actorUserId
    return this.requestVoid(
      'DELETE',
      `/api/ext/realms/${encodeURIComponent(realmId)}/orgs/${encodeURIComponent(orgId)}/members/${encodeURIComponent(userId)}`,
      { query },
    )
  }

  /** List an organization's members, first page with server defaults
   * (page = 1, pageSize = 20). */
  listOrgMembers(realmId: string, orgId: string): Promise<OrgMemberPage> {
    return this.listOrgMembersPage(realmId, orgId)
  }

  /** List an organization's members with pagination. */
  listOrgMembersPage(
    realmId: string,
    orgId: string,
    page?: number,
    pageSize?: number,
  ): Promise<OrgMemberPage> {
    return this.requestJson<OrgMemberPage>(
      'GET',
      `/api/ext/realms/${encodeURIComponent(realmId)}/orgs/${encodeURIComponent(orgId)}/members`,
      { query: this.pageQuery(page, pageSize) },
    )
  }

  /** Query a user's tier in an organization (non-members get `member: false`). */
  getOrgMember(realmId: string, orgId: string, userId: string): Promise<OrgMemberLookup> {
    return this.requestJson(
      'GET',
      `/api/ext/realms/${encodeURIComponent(realmId)}/orgs/${encodeURIComponent(orgId)}/members/${encodeURIComponent(userId)}`,
    )
  }

  /** Check a user's capabilities in an organization (all rules must hit;
   * non-members and denied members are structured 200 results). */
  checkOrgPermission(
    realmId: string,
    orgId: string,
    request: OrgPermissionCheckSdkRequest,
  ): Promise<OrgPermissionCheckResult> {
    return this.requestJson(
      'POST',
      `/api/ext/realms/${encodeURIComponent(realmId)}/orgs/${encodeURIComponent(orgId)}/permission-check`,
      { body: request },
    )
  }

  /** List a user's organizations in the caller's Client App, first page with
   * server defaults (page = 1, pageSize = 20). */
  listUserOrgs(realmId: string, userId: string): Promise<UserOrgPage> {
    return this.listUserOrgsPage(realmId, userId)
  }

  /** List a user's organizations in the caller's Client App with pagination. */
  listUserOrgsPage(
    realmId: string,
    userId: string,
    page?: number,
    pageSize?: number,
  ): Promise<UserOrgPage> {
    return this.requestJson<UserOrgPage>(
      'GET',
      `/api/ext/realms/${encodeURIComponent(realmId)}/orgs`,
      { query: { userId, ...this.pageQuery(page, pageSize) } },
    )
  }
}
