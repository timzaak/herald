/** Shared constants + handler factories for the Herald Node SDK suite. */

import { HttpResponse, http } from 'msw'
import type { PermissionCheckResponse } from '../src'

export const BASE_URL = 'http://herald.test'
export const API_KEY = 'test-api-key'

export const permissionCheckUrl = `${BASE_URL}/api/ext/permission/check`

/** Counting handler for the permission endpoint; returns `allowed: true` with
 * the given userId and records every request for assertions. */
export function permissionHandler(userId: string, response?: Partial<PermissionCheckResponse>) {
  const requests: Request[] = []
  const handler = http.post(permissionCheckUrl, async ({ request }) => {
    requests.push(request.clone())
    return HttpResponse.json({ allowed: true, userId, ...response })
  })
  return { handler, requests }
}
