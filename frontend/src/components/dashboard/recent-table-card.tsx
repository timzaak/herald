import type { ReactNode } from 'react'
import { Link } from '@tanstack/react-router'
import { ArrowRight } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { m } from '@/paraglide/messages'

interface RecentTableCardProps {
  title: string
  linkTo: string
  linkParams: Record<string, string>
  testId?: string
  isLoading?: boolean
  children: ReactNode
}

/**
 * Shared shell for the dashboard "recent activity" tables: card frame,
 * heading and a "view all" link into the full management page.
 */
export function RecentTableCard({
  title,
  linkTo,
  linkParams,
  testId,
  isLoading,
  children,
}: RecentTableCardProps) {
  if (isLoading) {
    return <Skeleton className="h-[280px] rounded-xl" data-testid={`${testId}-skeleton`} />
  }

  return (
    <Card data-testid={testId}>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
        <Link
          to={linkTo}
          params={linkParams}
          className="flex items-center gap-1 text-xs text-primary hover:text-primary/80"
        >
          {m['dashboard.view_all']()}
          <ArrowRight className="size-3" />
        </Link>
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  )
}
