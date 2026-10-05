import { Link } from '@tanstack/react-router'
import { ListChecks, MonitorSmartphone, Settings, UserPlus } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/hooks/use-auth'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { filterByPermission } from '@/lib/utils/filter-by-permission'
import { useOnboardingUiStore } from '@/stores/onboarding-store'
import { m } from '@/paraglide/messages'

/**
 * Starter checklist pinned to the dashboard: three linked first steps plus the
 * always-available "replay the console tour" entry. Independent of the
 * completion marker — it stays rendered after the guidance is done. Tasks
 * require the same view permissions as their QuickNav counterparts, so a
 * restricted admin is never pointed at pages they cannot enter.
 */
export function OnboardingTasksCard() {
  const requestTourRestart = useOnboardingUiStore((state) => state.requestTourRestart)
  const { permissions } = useAuth()

  const tasks = [
    {
      testId: 'onboarding-task-users',
      icon: UserPlus,
      path: '/manage/users',
      title: m['onboarding.tasks_item_users_title'](),
      description: m['onboarding.tasks_item_users_description'](),
      permission: PERMISSION.USERS_VIEW,
    },
    {
      testId: 'onboarding-task-clients',
      icon: MonitorSmartphone,
      path: '/manage/client-apps',
      title: m['onboarding.tasks_item_clients_title'](),
      description: m['onboarding.tasks_item_clients_description'](),
      permission: PERMISSION.CLIENTS_VIEW,
    },
    {
      testId: 'onboarding-task-settings',
      icon: Settings,
      path: '/manage/settings',
      title: m['onboarding.tasks_item_settings_title'](),
      description: m['onboarding.tasks_item_settings_description'](),
      permission: PERMISSION.SETTINGS_VIEW,
    },
  ]

  const visibleTasks = filterByPermission(tasks, permissions)

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
