import { useMutation, useQueryClient } from '@tanstack/react-query'
import { grantPoints } from '@/lib/api-generated'
import type { GrantPointsResponse } from '@/lib/api-generated'
import type { GrantPointsFormData } from '@/lib/schemas/points-forms'
import { QUERY_KEYS } from '@/lib/constants'

export type { GrantPointsResponse }

export function useGrantPoints(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (values: GrantPointsFormData) => {
      const response = await grantPoints({
        body: {
          userId: values.userId,
          amount: values.amount,
          reason: values.reason,
          validityDays: values.validityDays ?? null,
          bucketId: values.bucketId,
        },
      })
      if (response.error) throw response.error
      return response.data as GrantPointsResponse
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [QUERY_KEYS.WALLETS_BY_BUCKET, realmId] })
      queryClient.invalidateQueries({ queryKey: [QUERY_KEYS.POINTS_TRANSACTIONS, realmId] })
    },
  })
}
