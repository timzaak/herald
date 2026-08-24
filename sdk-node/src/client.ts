/**
 * Herald server SDK client — a 1:1 TypeScript port of the Rust `herald-sdk`
 * crate (`backend/sdk/src/lib.rs`), which is this SDK's source of truth.
 *
 * Auth model: every call carries the realm/service `X-API-Key` header against
 * the external API surface (`/api/ext/*`). `checkPermission` additionally
 * mirrors the Rust caching behaviour exactly:
 *
 *   - a TTL cache keyed by the full request (token + clientId + rules);
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
  CreateRealmSdkRequest,
  CreateUserSdkRequest,
  GrantPointsResponse,
  PermissionCheckRequest,
  PermissionCheckResponse,
  PointsBalanceResponse,
  RealmInfo,
  RealmItem,
  SubscriptionDetail,
  UserInfo,
} from './types'

/** Rust `is_token_expired` threshold: 5 minutes, independent of the cache TTL. */
const TOKEN_EXPIRY_THRESHOLD_MS = 300_000

interface CacheEntry {
  response: PermissionCheckResponse
  /** Lazy TTL: `Date.now()` after which the entry is treated as evicted. */
  expiresAtMs: number
}

/**
 * Cache key for a permission check. `rules: undefined` and `rules: []` are
 * DIFFERENT keys (Rust: `Option<Vec<Rule>>` participates in `Hash`/`Eq`), and
 * rule order is significant — `JSON.stringify` preserves both distinctions.
 */
function permissionCacheKey(req: PermissionCheckRequest): string {
  return JSON.stringify({ accessToken: req.accessToken, clientId: req.clientId, rules: req.rules })
}

/** Map a `fetch` response onto the Rust `handle_response` semantics. */
async function handleResponse<T>(response: Response): Promise<T> {
  const text = await response.text()
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

  private async requestJson<T>(method: 'GET' | 'POST', path: string, options: RequestOptions = {}): Promise<T> {
    let url = `${this.baseUrl}${path}`
    if (options.query) {
      const search = new URLSearchParams(options.query).toString()
      if (search) url += `?${search}`
    }
    const headers: Record<string, string> = { 'X-API-Key': this.apiKey }
    if (options.body !== undefined) headers['Content-Type'] = 'application/json'

    let response: Response
    try {
      response = await fetch(url, {
        method,
        headers,
        body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
      })
    } catch (cause) {
      throw new HeraldSdkError('network', undefined, String(cause))
    }
    return handleResponse<T>(response)
  }

  // --- Permission check (with cache) ---

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

  // --- Billing ---

  /** Subscription detail for a client app. */
  getSubscription(realmId: string, clientAppId: string): Promise<SubscriptionDetail> {
    return this.requestJson('GET', `/api/ext/bill/${encodeURIComponent(realmId)}/client/${encodeURIComponent(clientAppId)}/subscription`)
  }

  // --- Points ---

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

  // --- Realms ---

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

  // --- Users ---

  createUser(realmId: string, request: CreateUserSdkRequest): Promise<UserInfo> {
    return this.requestJson('POST', `/api/ext/realms/${encodeURIComponent(realmId)}/users`, { body: request })
  }

  async listUsers(realmId: string): Promise<UserInfo[]> {
    const body = await this.requestJson<{ items: UserInfo[] }>(
      'GET',
      `/api/ext/realms/${encodeURIComponent(realmId)}/users`,
    )
    return body.items
  }

  getUser(realmId: string, userId: string): Promise<UserInfo> {
    return this.requestJson(
      'GET',
      `/api/ext/realms/${encodeURIComponent(realmId)}/users/${encodeURIComponent(userId)}`,
    )
  }

  // --- Client apps ---

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
}
