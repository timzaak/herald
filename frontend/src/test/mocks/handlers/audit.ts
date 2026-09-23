import { http, HttpResponse } from 'msw'

const API_BASE_URL = 'http://localhost:3000'

const MOCK_AUDIT_EVENT = {
  id: 'evt-001',
  action: 'user.create',
  actorId: 'actor-001',
  actorName: 'Admin User',
  actorType: 'admin',
  category: 'user_management',
  createdAt: '2026-05-14T10:30:00Z',
  ipAddress: '192.168.1.100',
  result: 'success',
  targetId: 'target-001',
  targetName: 'John Doe',
  targetType: 'user',
}

const MOCK_AUDIT_LIST = {
  items: [
    MOCK_AUDIT_EVENT,
    {
      id: 'evt-002',
      action: 'role.assign',
      actorId: 'actor-001',
      actorName: 'Admin User',
      actorType: 'admin',
      category: 'rbac',
      createdAt: '2026-05-14T09:15:00Z',
      ipAddress: '192.168.1.100',
      result: 'success',
      targetId: 'role-001',
      targetName: 'Editor Role',
      targetType: 'role',
    },
  ],
  page: 0,
  pageSize: 20,
  total: 2,
}

const MOCK_AUDIT_DETAIL = {
  ...MOCK_AUDIT_EVENT,
  details: { email: 'john@example.com' },
  traceId: 'trace-abc-123',
  userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
}

export const getAuditListHandler = http.get(`${API_BASE_URL}/api/audit`, ({ request, params }) => {
  const url = new URL(request.url)
  const page = parseInt(url.searchParams.get('page') || '0')
  const pageSize = parseInt(url.searchParams.get('pageSize') || '20')
  const category = url.searchParams.get('category')
  const action = url.searchParams.get('action')

  let filtered = MOCK_AUDIT_LIST.items
  if (category) {
    filtered = filtered.filter((item) => item.category === category)
  }
  if (action) {
    filtered = filtered.filter((item) => item.action === action)
  }

  return HttpResponse.json({
    items: filtered,
    page,
    pageSize,
    total: filtered.length,
  })
})

export const getAuditDetailHandler = http.get(
  `${API_BASE_URL}/api/audit/:eventId`,
  ({ params }) => {
    const { eventId } = params
    if (eventId === 'not-found') {
      return HttpResponse.json({ message: 'Event not found' }, { status: 404 })
    }
    return HttpResponse.json({ ...MOCK_AUDIT_DETAIL, id: eventId as string })
  }
)

export const auditHandlers = [getAuditListHandler, getAuditDetailHandler]
