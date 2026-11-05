/** @vitest-environment jsdom */
import { describe, expect, it, vi } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { renderWithProviders } from '@/test/utils/render'
import { PhoneUnbindDialog } from '../phone-unbind-dialog'

const BASE_URL = 'http://localhost:3000/api/user'

function renderDialog() {
  renderWithProviders(<PhoneUnbindDialog maskedPhone="138****8000" onClose={vi.fn()} />)
  return userEvent.setup()
}

describe('PhoneUnbindDialog reauthentication boundary (US-PL-001)', () => {
  it('uses the freshly issued ticket after each reauth, including recovery from a rejected ticket', async () => {
    const submitted: unknown[] = []
    let issued = 0
    server.use(
      http.post(`${BASE_URL}/reauth`, () => HttpResponse.json({ availableFactors: ['password'] })),
      http.post(`${BASE_URL}/reauth/verify`, () =>
        HttpResponse.json({
          reauthToken: `ticket-${++issued}`,
          expiresIn: 120,
        })
      ),
      http.post(`${BASE_URL}/phone/unbind`, async ({ request }) => {
        submitted.push(await request.json())
        return submitted.length === 1
          ? HttpResponse.json({ status: 401, message: 'Ticket expired' }, { status: 401 })
          : HttpResponse.json({ message: 'ok' })
      })
    )
    const user = renderDialog()
    await user.click(screen.getByTestId('confirm-unbind-phone-button'))
    await user.type(await screen.findByTestId('phone-unbind-password-input'), 'correct-password')
    await user.click(screen.getByTestId('phone-unbind-verify-button'))
    await waitFor(() => expect(submitted).toEqual([{ reauthToken: 'ticket-1' }]))
    const retry = await screen.findByTestId('phone-unbind-password-input')
    await waitFor(() => expect(retry).toHaveValue(''))
    await user.type(retry, 'correct-password')
    await user.click(screen.getByTestId('phone-unbind-verify-button'))
    await screen.findByTestId('phone-unbind-done-panel')
    expect(submitted).toEqual([{ reauthToken: 'ticket-1' }, { reauthToken: 'ticket-2' }])
  })

  it('never attempts unbind when reauth returns no ownership ticket', async () => {
    const unbind = vi.fn()
    server.use(
      http.post(`${BASE_URL}/reauth`, () => HttpResponse.json({ availableFactors: ['password'] })),
      http.post(`${BASE_URL}/reauth/verify`, () => HttpResponse.json({ expiresIn: 120 })),
      http.post(`${BASE_URL}/phone/unbind`, () => {
        unbind()
        return HttpResponse.json({ message: 'ok' })
      })
    )
    const user = renderDialog()
    await user.click(screen.getByTestId('confirm-unbind-phone-button'))
    await user.type(await screen.findByTestId('phone-unbind-password-input'), 'correct-password')
    await user.click(screen.getByTestId('phone-unbind-verify-button'))
    await expect(screen.findByTestId('phone-unbind-error-message')).resolves.toBeVisible()
    expect(unbind).not.toHaveBeenCalled()
    expect(screen.queryByTestId('phone-unbind-done-panel')).not.toBeInTheDocument()
  })
})
