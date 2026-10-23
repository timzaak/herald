import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { PasswordInput } from '../password-input'

// The eye toggle exists so a user can proofread a masked password before
// submitting it: the field must reveal on demand and re-mask afterwards,
// and screen readers must announce which of the two states it is in.
describe('PasswordInput', () => {
  it('GIVEN a password input WHEN the eye button is clicked THEN the typed password is revealed, and clicking again re-masks it', async () => {
    const user = userEvent.setup()
    render(<PasswordInput data-testid="pw" defaultValue="s3cret!" />)

    const input = screen.getByTestId('pw') as HTMLInputElement
    expect(input.type).toBe('password')

    await user.click(screen.getByTestId('pw-visibility-toggle'))
    expect(input.type).toBe('text')

    await user.click(screen.getByTestId('pw-visibility-toggle'))
    expect(input.type).toBe('password')
  })

  it('GIVEN the eye button WHEN inspecting accessibility attributes THEN its label and pressed state track the reveal state', async () => {
    const user = userEvent.setup()
    render(<PasswordInput data-testid="pw" />)

    const toggle = screen.getByTestId('pw-visibility-toggle')
    expect(toggle).toHaveAttribute('aria-label', 'Show password')
    expect(toggle).toHaveAttribute('aria-pressed', 'false')

    await user.click(toggle)
    expect(toggle).toHaveAttribute('aria-label', 'Hide password')
    expect(toggle).toHaveAttribute('aria-pressed', 'true')
  })

  it('GIVEN the eye button WHEN typing into the input THEN the button never submits the surrounding form', async () => {
    const user = userEvent.setup()
    render(
      <form onSubmit={() => expect.unreachable('toggle must not submit the form')}>
        <PasswordInput data-testid="pw" />
        <button type="submit">submit</button>
      </form>
    )

    await user.type(screen.getByTestId('pw'), 'hunter2')
    await user.click(screen.getByTestId('pw-visibility-toggle'))
    expect((screen.getByTestId('pw') as HTMLInputElement).value).toBe('hunter2')
  })
})
