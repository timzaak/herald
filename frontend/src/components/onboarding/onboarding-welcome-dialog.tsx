import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { m } from '@/paraglide/messages'

interface OnboardingWelcomeDialogProps {
  /** Fresh self-service signup vs an existing admin entering the console. */
  freshSignup: boolean
  onStartTour: () => void
  onDismiss: () => void
}

/**
 * Marketing-style first-login welcome. Every dismiss path (skip button, X,
 * Esc, overlay click) funnels to `onDismiss`.
 */
export function OnboardingWelcomeDialog({
  freshSignup,
  onStartTour,
  onDismiss,
}: OnboardingWelcomeDialogProps) {
  return (
    <Dialog
      open
      onOpenChange={(next) => {
        if (!next) onDismiss()
      }}
    >
      <DialogContent data-testid="onboarding-welcome-dialog" className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>
            {freshSignup
              ? m['onboarding.welcome_title_fresh_signup']()
              : m['onboarding.welcome_title']()}
          </DialogTitle>
          <DialogDescription>{m['onboarding.welcome_description']()}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="outline"
            data-testid="onboarding-welcome-dismiss-button"
            onClick={onDismiss}
          >
            {m['onboarding.welcome_cta_dismiss']()}
          </Button>
          <Button data-testid="onboarding-tour-start-button" onClick={onStartTour}>
            {m['onboarding.welcome_cta_start_tour']()}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
