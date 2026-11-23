import { ConfirmDialog } from '@/components/shared'
import { m } from '@/paraglide/messages'

interface DeleteWebhookDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: () => void
  endpointUrl: string
}

export function DeleteWebhookDialog({
  open,
  onOpenChange,
  onConfirm,
  endpointUrl,
}: DeleteWebhookDialogProps) {
  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={m['webhooks.delete_title']()}
      description={m['webhooks.delete_description']({ url: endpointUrl })}
      onConfirm={onConfirm}
      confirmLabel={m['webhooks.delete_button']()}
      cancelLabel={m['common.cancel']()}
      contentTestId="delete-webhook-confirm-dialog"
      cancelTestId="cancel-delete-webhook-button"
      confirmTestId="confirm-delete-webhook-button"
    />
  )
}
