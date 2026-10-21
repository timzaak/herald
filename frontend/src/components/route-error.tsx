import { useState } from 'react'
import { useRouter, type ErrorComponentProps } from '@tanstack/react-router'
import { TriangleAlert, RefreshCw, Home } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { RealmRequiredError } from '@/lib/realm-routing'

/**
 * Root-level error boundary UI for TanStack Router.
 *
 * Rendered when any route loader (or component tree) throws and no nearer
 * `errorComponent` handles it. Replaces the library's default inline-styled
 * "Something went wrong!" page with the app's own design system so a failed
 * load still looks like part of the product.
 */
export function RootRouteError({ error, reset }: ErrorComponentProps) {
  const router = useRouter()
  const [showDetail, setShowDetail] = useState(false)
  const needsRealm = error instanceof RealmRequiredError

  const retry = () => {
    // Re-run the failed loader (invalidate marks it stale) then clear the
    // catch boundary so the route re-renders instead of staying on the error.
    router.invalidate()
    reset()
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <div className="w-full max-w-md text-center">
        <div className="mx-auto mb-5 flex size-14 items-center justify-center rounded-full bg-destructive/10">
          <TriangleAlert className="size-7 text-destructive" />
        </div>
        <h1 className="mb-2 text-xl font-semibold text-foreground">
          {needsRealm ? '此链接缺少租户信息' : '页面加载失败'}
        </h1>
        <p className="mb-6 text-sm text-muted-foreground">
          {needsRealm
            ? '请使用所属租户的完整登录链接，或通过该租户已绑定的自定义域名访问。'
            : '抱歉，加载此页面时出现问题。请稍后重试，或返回首页继续操作。'}
        </p>
        <div className="flex items-center justify-center gap-3">
          {/* Retry re-runs the same loader; a missing realm prefix is decided
              by the URL alone, so retry can never succeed — offer only the
              way out. */}
          {!needsRealm && (
            <Button onClick={retry}>
              <RefreshCw />
              重试
            </Button>
          )}
          <Button variant="outline" onClick={() => window.location.assign('/')}>
            <Home />
            回首页
          </Button>
        </div>
        {import.meta.env.DEV && error?.message ? (
          <div className="mt-6 text-left">
            <button
              type="button"
              onClick={() => setShowDetail((s) => !s)}
              className="text-xs text-muted-foreground underline-offset-2 hover:underline"
            >
              {showDetail ? '隐藏错误详情' : '查看错误详情'}
            </button>
            {showDetail ? (
              <pre className="mt-2 max-h-48 overflow-auto rounded-md bg-muted p-3 text-xs text-muted-foreground">
                <code>{error.message}</code>
              </pre>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  )
}
