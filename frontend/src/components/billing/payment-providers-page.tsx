import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { PageHeader } from '@/components/shared/page-header'
import { DeleteConfirmDialog } from './DeleteConfirmDialog'
import { Edit, Trash2, Plug2, Plus } from 'lucide-react'
import { listPaymentProviders } from '@/lib/api-generated'
import { deleteRealmConfig } from '@/lib/api-generated/sdk.gen'
import {
  STRIPE_CONFIG_KEYS,
  APPLE_CONFIG_KEYS,
  GOOGLE_CONFIG_KEYS,
  type PaymentProvider,
} from '@/lib/billing-constants'
import { CREEM_CONFIG_KEYS } from '@/lib/creem-config-utils'
import { WECHAT_CONFIG_KEYS } from '@/lib/wechat-config-utils'
import { formatProviderName } from '@/components/billing/format-provider-name'
import { queryKeys } from '@/data/query-options'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

/** Display order for the providers table and "add provider" buttons. */
const PROVIDER_TYPES: readonly PaymentProvider[] = ['stripe', 'creem', 'apple', 'google', 'wechat']

const CONFIG_KEYS_BY_PROVIDER: Record<PaymentProvider, readonly string[]> = {
  stripe: Object.values(STRIPE_CONFIG_KEYS),
  creem: Object.values(CREEM_CONFIG_KEYS),
  apple: Object.values(APPLE_CONFIG_KEYS),
  google: Object.values(GOOGLE_CONFIG_KEYS),
  wechat: Object.values(WECHAT_CONFIG_KEYS),
}

function ProviderRow({
  type,
  onEdit,
  onDelete,
}: {
  type: PaymentProvider
  onEdit: () => void
  onDelete: () => void
}) {
  return (
    <TableRow data-testid={`${type}-provider-row`}>
      <TableCell className="font-medium">{formatProviderName(type)}</TableCell>
      <TableCell className="text-right">
        <div className="flex items-center justify-end gap-2">
          <Button variant="outline" size="sm" onClick={onEdit} data-testid={`edit-${type}-button`}>
            <Edit className="mr-1 h-3 w-3" />
            {m['common.edit']()}
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={onDelete}
            data-testid={`delete-${type}-button`}
          >
            <Trash2 className="mr-1 h-3 w-3" />
            {m['common.delete']()}
          </Button>
        </div>
      </TableCell>
    </TableRow>
  )
}

interface PaymentProvidersPageProps {
  realmId: string
}

export function PaymentProvidersPage({ realmId }: PaymentProvidersPageProps) {
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false)
  const [deleteProviderType, setDeleteProviderType] = useState<PaymentProvider>('stripe')

  const { data: providers, isLoading } = useQuery({
    queryKey: ['payment-providers', realmId],
    queryFn: async () => {
      const result = await listPaymentProviders()
      return result.data?.providers ?? []
    },
  })

  const deleteMutation = useMutation({
    mutationFn: async () => {
      const configKeys = CONFIG_KEYS_BY_PROVIDER[deleteProviderType].map((configKey) => ({
        configType: deleteProviderType,
        configKey,
      }))
      // Delete all keys, ignoring 404s for keys that don't exist
      await Promise.all(
        configKeys.map((k) =>
          deleteRealmConfig({ path: { ...k } }).catch((e) => {
            if (e?.status !== 404) throw e
          })
        )
      )
      return undefined
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['payment-providers', realmId] })
      queryClient.invalidateQueries({ queryKey: ['realmConfig', realmId] })
      queryClient.invalidateQueries({ queryKey: queryKeys.featureAvailability(realmId) })
      toast.success(m['billing.provider_deleted']())
      setIsDeleteDialogOpen(false)
    },
    onError: (error: { status?: number; message?: string }) => {
      if (error?.status === 409) {
        toast.error(m['billing.provider_delete_conflict']())
      } else {
        toast.error(m['billing.provider_delete_failed']())
      }
    },
  })

  const handleNavigate = (type: PaymentProvider) => {
    void navigate({
      to: realmPath({ ...realmContext, realmId }, `/manage/billing/payment-providers/${type}`),
    })
  }

  const handleDelete = () => {
    deleteMutation.mutate()
  }

  const openDeleteDialog = (type: PaymentProvider) => {
    setDeleteProviderType(type)
    setIsDeleteDialogOpen(true)
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">{m['billing.loading_providers']()}</div>
      </div>
    )
  }

  const configuredPlatforms = new Set((providers ?? []).map((p) => p.platform))
  const configuredTypes = PROVIDER_TYPES.filter((type) => configuredPlatforms.has(type))
  const unconfiguredTypes = PROVIDER_TYPES.filter((type) => !configuredPlatforms.has(type))
  const hasAnyProvider = configuredTypes.length > 0

  return (
    <div className="space-y-6" data-testid="payment-providers-page">
      <PageHeader title={m['billing.payment_providers_title']()} />

      {unconfiguredTypes.length > 0 && (
        <div className="flex gap-2 flex-wrap">
          {unconfiguredTypes.map((type) => (
            <Button
              key={type}
              onClick={() => handleNavigate(type)}
              data-testid={`add-${type}-button`}
              variant="outline"
            >
              <Plus className="mr-2 h-4 w-4" />
              {m['billing.add_provider']({ name: formatProviderName(type) })}
            </Button>
          ))}
        </div>
      )}

      {hasAnyProvider ? (
        <Table data-testid="provider-list">
          <TableHeader>
            <TableRow>
              <TableHead>{m['billing.col_provider']()}</TableHead>
              <TableHead className="text-right">{m['common.actions']()}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {configuredTypes.map((type) => (
              <ProviderRow
                key={type}
                type={type}
                onEdit={() => handleNavigate(type)}
                onDelete={() => openDeleteDialog(type)}
              />
            ))}
          </TableBody>
        </Table>
      ) : (
        <Card className="border-dashed">
          <CardContent className="flex flex-col items-center justify-center py-12">
            <Plug2 className="h-12 w-12 text-muted-foreground mb-4" />
            <p className="text-sm text-muted-foreground text-center">
              {m['billing.no_providers_configured']()}
            </p>
          </CardContent>
        </Card>
      )}

      <DeleteConfirmDialog
        open={isDeleteDialogOpen}
        onOpenChange={setIsDeleteDialogOpen}
        onConfirm={handleDelete}
        configType={formatProviderName(deleteProviderType)}
        activeSubscriptions={0}
        isDeleting={deleteMutation.isPending}
      />
    </div>
  )
}
