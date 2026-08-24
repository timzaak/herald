import { describe, it, expect } from 'vitest'
import { http, HttpResponse } from 'msw'
import { server } from './mocks/server'
import { makeClient, makeStatus, urls } from './helpers'

describe('status / logout (US-JS-006)', () => {
  it('get_status_populates_session', async () => {
    server.use(http.get(urls.status, () => HttpResponse.json(makeStatus())))

    const { client, events } = makeClient()
    const data = await client.getStatus()
    expect(data.authenticated).toBe(true)

    const session = client.session.getSession()
    expect(session.authenticated).toBe(true)
    expect(session.userId).toBe('u-1')
    expect(session.credentialClass).toBe('custom_user_ui')

    expect(events.some((e) => e.type === 'authenticated')).toBe(true)
  })

  it('logout_revokes_family_clears_session_emits_logged_out', async () => {
    server.use(
      http.get(urls.status, () => HttpResponse.json(makeStatus())),
      http.post(urls.logout, () => HttpResponse.json({ message: 'logged out' })),
    )

    const { client, events } = makeClient()
    await client.getStatus() // populate an authenticated session
    expect(client.session.getSession().authenticated).toBe(true)

    // Seed token material so we can assert it is cleared.
    client.storage.setRefreshToken('rt-before')

    const res = await client.logout()
    expect(res.message).toBe('logged out')

    expect(client.storage.getRefreshToken()).toBeNull()
    expect(client.session.getSession().authenticated).toBe(false)
    expect(events.some((e) => e.type === 'logged-out')).toBe(true)
  })
})
