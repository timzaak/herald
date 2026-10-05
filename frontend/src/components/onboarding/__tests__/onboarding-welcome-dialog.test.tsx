import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { OnboardingWelcomeDialog } from '../onboarding-welcome-dialog'

// The one-shot signup signal selects only the welcome-copy variant; both
// variants render from the same shell and neither action may depend on
// anything else in the dialog.
describe('OnboardingWelcomeDialog', () => {
  it('GIVEN an existing admin entering the console WHEN the welcome renders THEN the generic copy is used and both actions stay usable', async () => {
    const user = userEvent.setup({ delay: null })
    const onStartTour = vi.fn()
    const onDismiss = vi.fn()

    render(
      <OnboardingWelcomeDialog
        freshSignup={false}
        onStartTour={onStartTour}
        onDismiss={onDismiss}
      />
    )

    expect(screen.getByText('Welcome to the Console')).toBeInTheDocument()

    await user.click(screen.getByTestId('onboarding-tour-start-button'))
    expect(onStartTour).toHaveBeenCalledTimes(1)

    await user.click(screen.getByTestId('onboarding-welcome-dismiss-button'))
    expect(onDismiss).toHaveBeenCalledTimes(1)
  })

  it('GIVEN a fresh self-service signup WHEN the welcome renders THEN the fresh-signup copy variant is used', () => {
    render(<OnboardingWelcomeDialog freshSignup={true} onStartTour={vi.fn()} onDismiss={vi.fn()} />)

    expect(screen.getByText('Welcome to Your New Realm')).toBeInTheDocument()
  })
})
