import { http, HttpResponse } from 'msw'

const API_BASE_URL = 'http://localhost:3000'

// ===== Grant Points Handlers =====

export const grantPointsHandler = http.post(
  `${API_BASE_URL}/api/points/grant`,
  async ({ request, params }) => {
    const body = (await request.json()) as any

    return HttpResponse.json({
      transactionId: 'txn-grant-001',
      userId: body.userId,
      amount: body.amount,
      grantedBalance: body.amount,
      totalBalance: body.amount + 500,
      expiresAt: body.validityDays
        ? new Date(Date.now() + body.validityDays * 86400000).toISOString()
        : null,
    })
  }
)

// ===== User Search Handlers =====

const DEFAULT_USERS = [
  {
    id: 'user-1',
    email: 'alice@example.com',
    nickname: 'Alice',
    realmId: 'test-realm',
    status: 1,
    createdAt: '2026-01-01T00:00:00Z',
  },
  {
    id: 'user-2',
    email: 'bob@example.com',
    nickname: null,
    realmId: 'test-realm',
    status: 1,
    createdAt: '2026-01-02T00:00:00Z',
  },
]

export const userSearchHandler = http.get(`${API_BASE_URL}/api/users`, ({ request, params }) => {
  const url = new URL(request.url)
  const email = url.searchParams.get('email') ?? ''
  const filtered = email ? DEFAULT_USERS.filter((u) => u.email.includes(email)) : DEFAULT_USERS

  return HttpResponse.json({
    items: filtered,
    page: 0,
    pageSize: 20,
    total: filtered.length,
  })
})

// ===== Export Handlers Array =====

export const pointsHandlers = [grantPointsHandler]

// ===== Error Scenario Helpers =====

export function createGrantPointsErrorHandler(status: number, message: string) {
  return http.post(`${API_BASE_URL}/api/points/grant`, () => {
    return HttpResponse.json({ message }, { status })
  })
}

export function createUserSearchEmptyHandler() {
  return http.get(`${API_BASE_URL}/api/users`, () => {
    return HttpResponse.json({
      items: [],
      page: 0,
      pageSize: 20,
      total: 0,
    })
  })
}
