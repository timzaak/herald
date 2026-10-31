import { createFileRoute, useRouter } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { GraduationCap, LayoutDashboard, Users, UserPlus, Activity } from 'lucide-react'
import { dashboardStatsQueryOptions } from '@/data/query-options'
import { StatsCard } from '@/components/dashboard/stats-card'
import { AuthTrendChart } from '@/components/dashboard/auth-trend-chart'
import { RecentUsersTable } from '@/components/dashboard/recent-users-table'
import { RecentAuditEventsTable } from '@/components/dashboard/recent-audit-events-table'
import { RecentPointsTransactionsTable } from '@/components/dashboard/recent-points-transactions-table'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { useOnboardingUiStore } from '@/stores/onboarding-store'
import { m } from '@/paraglide/messages'
import { resolvedRealmFromPath } from '@/lib/realm-routing'
import { getErrorMessage } from '@/lib/error-utils'

export const Route = createFileRoute('/$realmId/manage/')({
  component: ManageDashboard,
})

export function ManageDashboard() {
  const router = useRouter()
  const { realmId } = resolvedRealmFromPath(router.state.location.pathname)
  const { data, isLoading, isError, error, refetch } = useQuery(dashboardStatsQueryOptions(realmId))
  const requestTourRestart = useOnboardingUiStore((state) => state.requestTourRestart)

  const userStats = data?.userStats
  const authTrend = data?.authTrend ?? []

  return (
    <div className="container mx-auto py-6 space-y-6">
      <div className="flex items-center gap-3">
        <div className="flex size-10 items-center justify-center rounded-xl bg-primary/10">
          <LayoutDashboard className="h-5 w-5 text-primary" />
        </div>
        <div>
          <h1 className="text-xl font-semibold tracking-tight">{m['dashboard.title']()}</h1>
          <p className="text-xs text-muted-foreground">{m['dashboard.subtitle']()}</p>
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="ml-auto"
          onClick={requestTourRestart}
          aria-label={m['onboarding.tasks_restart_tour']()}
          title={m['onboarding.tasks_restart_tour']()}
          data-testid="dashboard-replay-tour-button"
        >
          <GraduationCap className="h-5 w-5" />
        </Button>
      </div>

      {isError ? (
        <div
          className="rounded-lg border border-destructive/50 bg-destructive/10 p-6 text-center"
          data-testid="dashboard-error"
        >
          <p className="text-destructive mb-3">
            {getErrorMessage(error) || m['dashboard.failed_to_load']()}
          </p>
          <button
            onClick={() => refetch()}
            className="rounded-md bg-primary px-4 py-2 text-sm text-primary-foreground hover:bg-primary/90"
            data-testid="dashboard-retry-button"
          >
            {m['common.retry']()}
          </button>
        </div>
      ) : (
        <>
          <div className="grid gap-4 md:grid-cols-3" data-testid="dashboard-stats-row">
            {isLoading ? (
              <>
                <Skeleton className="h-[120px] rounded-xl" />
                <Skeleton className="h-[120px] rounded-xl" />
                <Skeleton className="h-[120px] rounded-xl" />
              </>
            ) : (
              <>
                <StatsCard
                  title={m['dashboard.total_users']()}
                  value={userStats?.totalUsers ?? 0}
                  description={m['dashboard.users']()}
                  icon={Users}
                  testId="dashboard-total-users-card"
                  linkTo="/$realmId/manage/users"
                  linkParams={{ realmId }}
                />
                <StatsCard
                  title={m['dashboard.new_users']()}
                  value={userStats?.newUsers ?? 0}
                  description={m['dashboard.past_7_days']()}
                  icon={UserPlus}
                  testId="dashboard-new-users-card"
                />
                <StatsCard
                  title={m['dashboard.active_users']()}
                  value={userStats?.activeUsers ?? 0}
                  description={m['dashboard.past_7_days']()}
                  icon={Activity}
                  testId="dashboard-active-users-card"
                />
              </>
            )}
          </div>

          {isLoading ? (
            <Skeleton className="h-[350px] rounded-xl" data-testid="dashboard-chart-skeleton" />
          ) : (
            <AuthTrendChart data={authTrend} testId="dashboard-auth-trend-chart" />
          )}

          <div className="grid gap-4 lg:grid-cols-2" data-testid="dashboard-recent-tables">
            <RecentUsersTable realmId={realmId} />
            <RecentAuditEventsTable realmId={realmId} />
            <div className="lg:col-span-2">
              <RecentPointsTransactionsTable realmId={realmId} />
            </div>
          </div>
        </>
      )}
    </div>
  )
}
