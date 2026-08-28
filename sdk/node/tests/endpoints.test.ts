/**
 * Endpoint coverage for the non-permission methods, mirroring the Rust
 * crate's tests. WHY: these tests pin the exact wire contract (paths,
 * camelCase body fields, list-response unwrapping) that third-party Node
 * servers depend on — a drift here breaks integrations at runtime, not at
 * compile time.
 */

import { describe, expect, it } from 'vitest'
import { http, HttpResponse } from 'msw'
import { HeraldClient } from '../src'
import { server } from './mocks/server'
import { API_KEY, BASE_URL } from './helpers'

const client = () => new HeraldClient(BASE_URL, API_KEY)

describe('billing', () => {
  it('getSubscription hits the ext billing path and parses the detail', async () => {
    server.use(
      http.get(`${BASE_URL}/api/ext/bill/realm1/client/client1/subscription`, () =>
        HttpResponse.json({
          id: 'sub-123',
          clientAppId: 'client1',
          status: 'active',
          entitlementKey: 'basic-plan',
          paymentProvider: 'stripe',
          currentPeriodStart: null,
          currentPeriodEnd: null,
          cancelAt: null,
          cancelAtPeriodEnd: null,
          createdAt: '2025-01-01T00:00:00Z',
          updatedAt: '2025-01-01T00:00:00Z',
        }),
      ),
    )

    const sub = await client().getSubscription('realm1', 'client1')

    expect(sub.status).toBe('active')
    expect(sub.entitlementKey).toBe('basic-plan')
  })
})

describe('points', () => {
  it('getBalance passes userId as a query parameter', async () => {
    let capturedUrl = ''
    server.use(
      http.get(`${BASE_URL}/api/ext/points/realm1/balance`, ({ request }) => {
        capturedUrl = request.url
        return HttpResponse.json({
          userId: 'user-1',
          balance: 500,
          totalPaidGranted: 0,
          totalRecharged: 500,
          totalConsumed: 0,
          unit: 'credit',
          updatedAt: '2025-01-01T00:00:00Z',
        })
      }),
    )

    const balance = await client().getBalance('realm1', 'user-1')

    expect(new URL(capturedUrl).searchParams.get('userId')).toBe('user-1')
    expect(balance.balance).toBe(500)
  })

  it('consumePoints sends the camelCase body contract', async () => {
    let capturedBody: Record<string, unknown> | undefined
    server.use(
      http.post(`${BASE_URL}/api/ext/points/realm1/consume`, async ({ request }) => {
        capturedBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({
          userId: 'user-1',
          amount: 100,
          correlationId: 'corr-1',
          transactions: [
            {
              transactionId: 'tx-1',
              bucketId: 'bucket-1',
              walletId: 'wallet-1',
              userId: 'user-1',
              amount: 100,
              balanceAfter: 400,
            },
          ],
          allocations: [
            {
              bucketId: 'bucket-1',
              walletId: 'wallet-1',
              ledgerId: 'ledger-1',
              creditType: 'topup',
              allocatedAmount: 100,
            },
          ],
        })
      }),
    )

    const result = await client().consumePoints(
      'realm1',
      'user-1',
      'app-1',
      100,
      'Purchase item X',
      'idem-123',
    )

    expect(capturedBody).toEqual({
      userId: 'user-1',
      clientAppId: 'app-1',
      amount: 100,
      description: 'Purchase item X',
      idempotencyKey: 'idem-123',
    })
    expect(result.transactions[0]?.balanceAfter).toBe(400)
  })

  it('grantPoints sends bucketId/reason and parses expiresAt', async () => {
    let capturedBody: Record<string, unknown> | undefined
    server.use(
      http.post(`${BASE_URL}/api/ext/points/realm1/grant`, async ({ request }) => {
        capturedBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({
          transactionId: 'tx-9',
          userId: 'user-1',
          bucketId: 'bucket-1',
          amount: 200,
          grantedBalance: 200,
          balance: 200,
          expiresAt: '2026-07-01T00:00:00Z',
        })
      }),
    )

    const result = await client().grantPoints('realm1', 'user-1', 'bucket-1', 200, 'campaign', 30)

    expect(capturedBody).toEqual({
      userId: 'user-1',
      bucketId: 'bucket-1',
      amount: 200,
      reason: 'campaign',
      validityDays: 30,
    })
    expect(result.expiresAt).toBe('2026-07-01T00:00:00Z')
  })

  it('getTransaction hits the per-transaction path and parses null optionals', async () => {
    server.use(
      http.get(`${BASE_URL}/api/ext/points/realm1/transactions/tx-1`, () =>
        HttpResponse.json({
          transactionId: 'tx-1',
          walletId: 'wallet-1',
          userId: 'user-1',
          transactionType: 'consume',
          amount: 100,
          balanceAfter: 400,
          description: null,
          clientAppId: 'app-1',
          subscriptionId: null,
          externalRefId: null,
          createdAt: '2025-01-01T00:00:00Z',
        }),
      ),
    )

    const tx = await client().getTransaction('realm1', 'tx-1')

    expect(tx.transactionType).toBe('consume')
    expect(tx.clientAppId).toBe('app-1')
    expect(tx.description).toBeNull()
  })
})

describe('realms', () => {
  const realmDetail = {
    id: 'realm-001',
    name: 'test-realm',
    description: 'A test realm',
    adminUser: { id: 'user-001', email: 'admin@test.com', role: 'admin' },
    createdAt: '2025-01-01T00:00:00Z',
    updatedAt: '2025-01-01T00:00:00Z',
  }

  it('createRealm posts the request body', async () => {
    let capturedBody: Record<string, unknown> | undefined
    server.use(
      http.post(`${BASE_URL}/api/ext/realms`, async ({ request }) => {
        capturedBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json(realmDetail, { status: 201 })
      }),
    )

    const realm = await client().createRealm({
      name: 'test-realm',
      description: 'A test realm',
      adminUser: { email: 'admin@test.com', password: 'password123' },
    })

    expect(capturedBody).toMatchObject({ name: 'test-realm', adminUser: { email: 'admin@test.com' } })
    expect(realm.adminUser?.id).toBe('user-001')
  })

  it('listRealms unwraps the realms array', async () => {
    server.use(
      http.get(`${BASE_URL}/api/ext/realms`, () =>
        HttpResponse.json({
          realms: [
            { id: 'realm-001', name: 'realm-a', description: null, createdAt: '2025-01-01T00:00:00Z', updatedAt: '2025-01-01T00:00:00Z' },
            { id: 'realm-002', name: 'realm-b', description: 'Second realm', createdAt: '2025-02-01T00:00:00Z', updatedAt: '2025-02-01T00:00:00Z' },
          ],
        }),
      ),
    )

    const realms = await client().listRealms()

    expect(realms).toHaveLength(2)
    expect(realms[1]?.name).toBe('realm-b')
  })

  it('getRealm returns the detail', async () => {
    server.use(http.get(`${BASE_URL}/api/ext/realms/realm-001`, () => HttpResponse.json(realmDetail)))

    const realm = await client().getRealm('realm-001')

    expect(realm.id).toBe('realm-001')
  })
})

describe('users', () => {
  it('createUser posts under the realm and returns UserInfo', async () => {
    server.use(
      http.post(`${BASE_URL}/api/ext/realms/realm-001/users`, () =>
        HttpResponse.json(
          { id: 'user-001', email: 'test@example.com', nickname: 'testuser', status: 1, createdAt: '2025-01-01T00:00:00Z' },
          { status: 201 },
        ),
      ),
    )

    const user = await client().createUser('realm-001', {
      email: 'test@example.com',
      password: 'password123',
      nickname: 'testuser',
    })

    expect(user.id).toBe('user-001')
  })

  it('listUsers unwraps items from the paginated envelope', async () => {
    server.use(
      http.get(`${BASE_URL}/api/ext/realms/realm-001/users`, () =>
        HttpResponse.json({
          items: [
            { id: 'user-001', email: 'a@example.com', nickname: null, status: 1, createdAt: '2025-01-01T00:00:00Z' },
            { id: 'user-002', email: 'b@example.com', nickname: 'bob', status: 1, createdAt: '2025-02-01T00:00:00Z' },
          ],
          page: 1,
          pageSize: 20,
          total: 2,
        }),
      ),
    )

    const users = await client().listUsers('realm-001')

    expect(users).toHaveLength(2)
    expect(users[0]?.email).toBe('a@example.com')
  })

  it('getUser returns the detail', async () => {
    server.use(
      http.get(`${BASE_URL}/api/ext/realms/realm-001/users/user-001`, () =>
        HttpResponse.json({ id: 'user-001', email: 'test@example.com', nickname: null, status: 1, createdAt: '2025-01-01T00:00:00Z' }),
      ),
    )

    const user = await client().getUser('realm-001', 'user-001')

    expect(user.email).toBe('test@example.com')
  })
})

describe('client apps', () => {
  const appDetail = {
    id: 'app-001',
    clientId: 'client-abc',
    clientSecret: null,
    name: 'My App',
    description: 'A test app',
    redirectUris: ['https://example.com/callback'],
    enabled: true,
    createdAt: '2025-01-01T00:00:00Z',
  }

  it('createClientApp posts redirectUris and returns the detail', async () => {
    server.use(
      http.post(`${BASE_URL}/api/ext/realms/realm-001/client-apps`, () =>
        HttpResponse.json(appDetail, { status: 201 }),
      ),
    )

    const app = await client().createClientApp('realm-001', {
      name: 'My App',
      description: 'A test app',
      redirectUris: ['https://example.com/callback'],
    })

    expect(app.clientId).toBe('client-abc')
  })

  it('listClientApps unwraps the clientApps array', async () => {
    server.use(
      http.get(`${BASE_URL}/api/ext/realms/realm-001/client-apps`, () =>
        HttpResponse.json({
          clientApps: [
            { id: 'app-001', clientId: 'client-abc', name: 'App A', enabled: true, createdAt: '2025-01-01T00:00:00Z' },
            { id: 'app-002', clientId: 'client-def', name: 'App B', enabled: false, createdAt: '2025-02-01T00:00:00Z' },
          ],
        }),
      ),
    )

    const apps = await client().listClientApps('realm-001')

    expect(apps).toHaveLength(2)
    expect(apps[1]?.enabled).toBe(false)
  })

  it('getClientApp returns the detail', async () => {
    server.use(http.get(`${BASE_URL}/api/ext/realms/realm-001/client-apps/app-001`, () => HttpResponse.json(appDetail)))

    const app = await client().getClientApp('realm-001', 'app-001')

    expect(app.redirectUris).toEqual(['https://example.com/callback'])
  })
})
