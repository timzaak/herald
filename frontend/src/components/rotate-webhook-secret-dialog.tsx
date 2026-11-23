import { ConfirmDialog } from '@/components/shared'
import { m } from '@/paraglide/messages'

interface RotateWebhookSecretDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: () => void
  endpointUrl: string
}

export function RotateWebhookSecretDialog({
  open,
  onOpenChange,
  onConfirm,
  endpointUrl,
}: RotateWebhookSecretDialogProps) {
  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={m['webhooks.rotate_title']()}
      description={m['webhooks.rotate_description']({ url: endpointUrl })}
      onConfirm={onConfirm}
      confirmLabel={m['webhooks.rotate_button']()}
      cancelLabel={m['common.cancel']()}
      confirmClassName="bg-warning text-white hover:bg-warning/90"
      contentTestId="rotate-webhook-secret-dialog"
      cancelTestId="cancel-rotate-webhook-secret-button"
      confirmTestId="confirm-rotate-webhook-secret-button"
    />
  )
}
