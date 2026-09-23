import { useMutation, useQueryClient } from '@tanstack/react-query'
import { revokeUserSession, revokeAllUserSessions } from '@/lib/api-generated'
import type { RevokeAllSessionsResponse } from '@/lib/api-generated'
import { queryKeys } from '@/data/query-options'

export function useRevokeUserSession(realmId: string, userId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (familyId: string) => {
      const result = await revokeUserSession({
        path: { userId, familyId },
      })
      if (result.error) throw result.error
      return result.data
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.userSessions(realmId, userId) })
    },
  })
}

export function useRevokeAllUserSessions(realmId: string, userId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async () => {
      const result = await revokeAllUserSessions({
        path: { userId },
      })
      if (result.error) throw result.error
      return result.data as RevokeAllSessionsResponse
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.userSessions(realmId, userId) })
    },
  })
}
