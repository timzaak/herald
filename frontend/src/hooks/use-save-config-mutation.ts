import { useMutation, useQueryClient, type QueryKey } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { toast } from 'sonner'
import { queryKeys } from '@/data/query-options'
import { resolveApiError } from '@/lib/error-utils'
import { m } from '@/paraglide/messages'

interface UseSaveConfigMutationProps<T> {
  realmId: string
  mutationFn: (data: T) => Promise<void>
  providerName: string
  invalidateKeys?: QueryKey[]
  isEditing: boolean
}

export function resolveConfigSaveErrorMessage(error: unknown): string {
  const message = resolveApiError(error).message

  // These are intentionally exact matches for stable, user-safe messages
  // emitted by the realm-config API. Unknown messages remain visible so a new
  // backend validation does not degrade into an unhelpful generic failure.
  switch (message) {
    case 'Payment provider base_url overrides are disabled in production':
      return m['billing.config_error_base_url_production']()
    case 'Secret value is required':
      return m['billing.config_error_secret_required']()
    case 'Failed to load existing provider secret':
      return m['billing.config_error_load_secret']()
    case 'Failed to upsert realm config':
    case 'Failed to batch upsert realm configs':
      return m['billing.config_error_save']()
    default:
      return message ?? m['billing.config_error_unknown']()
  }
}

/**
 * Shared mutation hook for billing config forms.
 * Handles error toast + success toast + query invalidation + navigation.
 */
export function useSaveConfigMutation<T>({
  realmId,
  mutationFn,
  providerName,
  invalidateKeys,
  isEditing,
}: UseSaveConfigMutationProps<T>) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const defaultInvalidateKeys = [
    ['payment-providers', realmId],
    ['realmConfig', realmId],
    queryKeys.featureAvailability(realmId),
  ]

  const keysToInvalidate = invalidateKeys ?? defaultInvalidateKeys

  return useMutation({
    mutationFn,
    onSuccess: async () => {
      const action = isEditing ? m['billing.updated']() : m['billing.created']()
      toast.success(m['billing.config_saved']({ provider: providerName, action }))
      await Promise.all(
        keysToInvalidate.map((key) => queryClient.invalidateQueries({ queryKey: key }))
      )
      navigate({ to: '/$realmId/manage/billing/payment-providers', params: { realmId } })
    },
    onError: (error: unknown) => {
      toast.error(
        m['billing.config_save_failed']({ message: resolveConfigSaveErrorMessage(error) })
      )
    },
  })
}
