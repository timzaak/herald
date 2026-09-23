import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { createOauthConfig, updateOauthConfig, deleteOauthConfig } from '@/lib/api-generated'
import type {
  OAuthConfigResponse,
  CreateOAuthConfigRequest,
  UpdateOAuthConfigRequest,
} from '@/lib/api-generated'
import type { OAuthConfigFormData } from '@/lib/schemas/oauth-config'
import { getErrorMessage } from '@/lib/error-utils'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'

/**
 * Hook for OAuth provider toggle (enable/disable)
 */
export function useOauthToggleMutation(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (config: OAuthConfigResponse) => {
      const response = await updateOauthConfig({
        path: { providerType: config.providerType },
        body: {
          enabled: !config.enabled,
        },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success(m['oauth.toggle_success']())
      queryClient.invalidateQueries({ queryKey: queryKeys.oauthConfigs(realmId) })
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      console.error('Provider status update failed:', error)
      toast.error(m['oauth.toggle_error']({ message: errorMessage }))
    },
  })
}

/**
 * Hook for OAuth provider deletion
 */
export function useOauthDeleteMutation(realmId: string, onDeleteSuccess?: () => void) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (providerType: string) => {
      const response = await deleteOauthConfig({
        path: { providerType },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success(m['oauth.delete_success']())
      queryClient.invalidateQueries({ queryKey: queryKeys.oauthConfigs(realmId) })
      onDeleteSuccess?.()
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      console.error('Provider delete failed:', error)
      toast.error(m['oauth.delete_error']({ message: errorMessage }))
    },
  })
}

/**
 * Hook for OAuth provider save (create or update)
 */
interface UseOauthSaveMutationOptions {
  realmId: string
  editingConfig?: OAuthConfigResponse
  onSuccess?: () => void
}

export function useOauthSaveMutation({
  realmId,
  editingConfig,
  onSuccess,
}: UseOauthSaveMutationOptions) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (values: OAuthConfigFormData) => {
      console.log('[OAuth Save Mutation] Starting save operation', {
        isEdit: !!editingConfig,
        providerType: editingConfig?.providerType,
        realmId,
        values,
      })

      if (editingConfig) {
        const updateData: UpdateOAuthConfigRequest = {
          clientId: values.clientId,
          enabled: values.enabled,
        }
        if (values.clientSecret && values.clientSecret.length > 0) {
          updateData.clientSecret = values.clientSecret
        }
        console.log('[OAuth Save Mutation] Sending PUT request to update provider', {
          path: { realmId, providerType: editingConfig.providerType },
          body: updateData,
        })
        const response = await updateOauthConfig({
          path: { providerType: editingConfig.providerType },
          body: updateData,
        })
        console.log('[OAuth Save Mutation] PUT response received', response)
        if (response.error) {
          console.error('[OAuth Save Mutation] PUT request failed', response.error)
          throw response.error
        }
        return response.data
      } else {
        const createData: CreateOAuthConfigRequest = {
          providerType: values.providerType,
          clientId: values.clientId,
          clientSecret: values.clientSecret || '',
          scopes: values.scopes,
          enabled: values.enabled,
        }
        console.log('[OAuth Save Mutation] Sending POST request to create provider', {
          path: { realmId },
          body: createData,
        })
        const response = await createOauthConfig({
          body: createData,
        })
        console.log('[OAuth Save Mutation] POST response received', response)
        if (response.error) {
          console.error('[OAuth Save Mutation] POST request failed', response.error)
          throw response.error
        }
        return response.data
      }
    },
    onSuccess: (data) => {
      const action = editingConfig
        ? m['oauth.save_action_updated']()
        : m['oauth.save_action_created']()
      console.log('[OAuth Save Mutation] Success', {
        action,
        providerType: editingConfig?.providerType,
        data,
      })
      toast.success(
        m['oauth.save_success']({
          name: editingConfig?.providerType || 'new',
          action,
        })
      )
      queryClient.invalidateQueries({ queryKey: queryKeys.oauthConfigs(realmId) })
      onSuccess?.()
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      console.error('[OAuth Save Mutation] Provider save failed:', error)
      toast.error(m['oauth.save_error']({ message: errorMessage }))
    },
  })
}
