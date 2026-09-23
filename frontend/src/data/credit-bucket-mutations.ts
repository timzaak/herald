import { useMutation, useQueryClient } from '@tanstack/react-query'
import {
  createCreditBucketHandler,
  updateCreditBucketHandler,
  deleteCreditBucketHandler,
} from '@/lib/api-generated'
import type {
  BucketDetailResponse,
  CreateCreditBucketRequest,
  UpdateCreditBucketRequest,
} from '@/lib/api-generated'
import { QUERY_KEYS } from '@/lib/constants'
import { queryKeys } from '@/data/query-options'

export type { BucketDetailResponse, CreateCreditBucketRequest, UpdateCreditBucketRequest }

export function useCreateCreditBucket(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (body: CreateCreditBucketRequest) => {
      const response = await createCreditBucketHandler({
        body,
      })
      if (response.error) throw response.error
      return response.data as BucketDetailResponse
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.creditBucketsList(realmId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.creditBucketOverview(realmId) })
    },
  })
}

export function useUpdateCreditBucket(realmId: string, bucketId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (body: UpdateCreditBucketRequest) => {
      const response = await updateCreditBucketHandler({
        path: { bucketId },
        body,
      })
      if (response.error) throw response.error
      return response.data as BucketDetailResponse
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.creditBucketsList(realmId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.creditBucket(realmId, bucketId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.creditBucketOverview(realmId) })
      // Entitlement mappings may change attribution when a bucket's attached
      // mapping set changes; invalidate the whole collection to stay correct.
      queryClient.invalidateQueries({ queryKey: [QUERY_KEYS.ENTITLEMENT_MAPPINGS, realmId] })
    },
  })
}

export function useDeleteCreditBucket(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (bucketId: string) => {
      const response = await deleteCreditBucketHandler({
        path: { bucketId },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.creditBucketsList(realmId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.creditBucketOverview(realmId) })
    },
  })
}
