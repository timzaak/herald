import { describe, it, expect } from 'vitest'
import { http, HttpResponse } from 'msw'
import { server } from './mocks/server'
import { CLIENT, makeClient, urls } from './helpers'

describe('register / reset (US-JS-002 / US-JS-003)', () => {
  it('register_returns_verification_required', async () => {
    let receivedBody: Record<string, unknown> | undefined
    server.use(
      http.post(urls.register, async ({ request }) => {
        receivedBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ message: 'registered', verificationRequired: true })
      }),
    )

    const { client } = makeClient()
    const res = await client.register({ email: 'a@b.c', password: 'pw', username: 'a' })
    expect(res).toEqual({ message: 'registered', verificationRequired: true })
    // clientId is injected from config, not required on the payload.
    expect(receivedBody?.['clientId']).toBe(CLIENT)
    expect(receivedBody?.['email']).toBe('a@b.c')
  })

  it('trigger_verify_email', async () => {
    server.use(
      http.post(urls.verifyEmailTrigger, () => HttpResponse.json({ message: 'sent' })),
    )
    const { client } = makeClient()
    const res = await client.triggerVerifyEmail({ email: 'a@b.c' })
    expect(res).toEqual({ message: 'sent' })
  })

  it('request_password_reset_always_ok', async () => {
    server.use(
      http.post(urls.resetPasswordRequest, () => HttpResponse.json({ message: 'ok' })),
    )
    const { client } = makeClient()
    const res = await client.requestPasswordReset({ email: 'a@b.c' })
    expect(res).toEqual({ message: 'ok' })
  })

  it('confirm_is_not_wrapped_documented', () => {
    // Email-verification / password-reset CONFIRM endpoints are 302 browser
    // redirects (no JSON), so the SDK intentionally does NOT wrap them.
    const { client } = makeClient()
    expect((client as Record<string, unknown>)['verifyEmailConfirm']).toBeUndefined()
    expect((client as Record<string, unknown>)['resetPasswordConfirm']).toBeUndefined()
    expect((client as Record<string, unknown>)['confirmEmail']).toBeUndefined()
    expect((client as Record<string, unknown>)['confirmPasswordReset']).toBeUndefined()
  })
})
