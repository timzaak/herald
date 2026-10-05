import { Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { Coins, CreditCard, ListChecks, Plug, Shield, Wallet } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/hooks/use-auth'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { filterByPermission } from '@/lib/utils/filter-by-permission'
import { featureAvailabilityQueryOptions } from '@/data/query-options'
import { useOnboardingUiStore } from '@/stores/onboarding-store'
import { m } from '@/paraglide/messages'

interface TaskItem {
  testId: string
  icon: LucideIcon
  path: string
  title: string
  description: string
  permission: string
  visible?: boolean
}

/**
 * Capability checklist pinned to the dashboard: links into the platform's
 * selling-point surfaces plus the always-available "replay the console tour"
 * entry. Independent of the completion marker — it stays rendered after the
 * guidance is done. Tasks carry the same permission and feature-availability
 * gating as their sidebar counterparts, so an admin is never pointed at a
 * page they cannot enter or a capability their realm has disabled.
 */
export function OnboardingTasksCard({ realmId }: { realmId: string }) {
  const requestTourRestart = useOnboardingUiStore((state) => state.requestTourRestart)
  const { permissions } = useAuth()
  const { data: features } = useQuery(featureAvailabilityQueryOptions(realmId))
  const adminFeatures = features?.admin

  const tasks: TaskItem[] = [
    {
      testId: 'onboarding-task-permissions',
      icon: Shield,
      path: '/$realmId/manage/permissions',
      title: m['onboarding.tasks_item_permissions_title'](),
      description: m['onboarding.tasks_item_permissions_description'](),
      permission: PERMISSION.PERMISSIONS_VIEW,
    },
    {
      testId: 'onboarding-task-payment-providers',
      icon: CreditCard,
      path: '/$realmId/manage/billing/payment-providers',
      title: m['onboarding.tasks_item_payment_providers_title'](),
      description: m['onboarding.tasks_item_payment_providers_description'](),
      permission: PERMISSION.BILLING_VIEW,
      visible: adminFeatures?.billingConfigVisible ?? true,
    },
    {
      testId: 'onboarding-task-points',
      icon: Coins,
      path: '/$realmId/manage/billing/credit-buckets',
      title: m['onboarding.tasks_item_points_title'](),
      description: m['onboarding.tasks_item_points_description'](),
      permission: PERMISSION.POINTS_VIEW,
      visible: adminFeatures?.pointsVisible ?? true,
    },
    {
      testId: 'onboarding-task-wallets',
      icon: Wallet,
      path: '/$realmId/manage/points/wallets',
      title: m['onboarding.tasks_item_wallets_title'](),
      description: m['onboarding.tasks_item_wallets_description'](),
      permission: PERMISSION.POINTS_VIEW,
      visible: adminFeatures?.pointsVisible ?? true,
    },
    {
      testId: 'onboarding-task-integrations',
      icon: Plug,
      path: '/$realmId/manage/settings',
      title: m['onboarding.tasks_item_integrations_title'](),
      description: m['onboarding.tasks_item_integrations_description'](),
      permission: PERMISSION.SETTINGS_VIEW,
    },
  ]

  const visibleTasks = filterByPermission(tasks, permissions).filter(
    (task) => task.visible !== false
  )

  return (
    <Card data-testid="onboarding-tasks-card">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <ListChecks className="size-4 text-primary" aria-hidden="true" />
          {m['onboarding.tasks_title']()}
        </CardTitle>
        <CardDescription>{m['onboarding.tasks_description']()}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid gap-4 md:grid-cols-3">
          {visibleTasks.map((task) => (
            <Link
              key={task.testId}
              to={task.path}
              params={{ realmId }}
              data-testid={task.testId}
              className="flex flex-col gap-2 rounded-lg border p-4 transition-colors hover:border-primary/40 hover:bg-accent"
            >
              <task.icon className="size-4 text-primary" aria-hidden="true" />
              <span className="text-sm font-medium">{task.title}</span>
              <span className="text-xs leading-relaxed text-muted-foreground">
                {task.description}
              </span>
            </Link>
          ))}
        </div>
        <Button
          variant="outline"
          size="sm"
          data-testid="onboarding-tour-restart-button"
          onClick={requestTourRestart}
        >
          {m['onboarding.tasks_restart_tour']()}
        </Button>
      </CardContent>
    </Card>
  )
}
