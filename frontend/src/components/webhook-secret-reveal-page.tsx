import { useEffect, useCallback } from 'react'
import { useRouterState, useNavigate } from '@tanstack/react-router'
import type { WebhookSubscriptionItemWithSecret } from '@/lib/api-generated'
import { Button } from '@/components/ui/button'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Card, CardContent } from '@/components/ui/card'
import { Copy, Check, TriangleAlert, ArrowLeft } from 'lucide-react'
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

interface WebhookSecretRevealPageProps {
  realmId: string
}

export function WebhookSecretRevealPage({ realmId }: WebhookSecretRevealPageProps) {
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const webhooksPath = realmPath({ ...realmContext, realmId }, '/manage/webhooks')
  const { copied, copyToClipboard } = useCopyToClipboard()

  const secretData = useRouterState({
    select: (s) =>
      (s.location.state as unknown as Record<string, unknown> | undefined)?.secretData as
        | WebhookSubscriptionItemWithSecret
        | undefined,
  })

  useEffect(() => {
    // Direct visits and refreshes have no state (the secret never enters the
    // URL) — bouncing back to the list is the api-keys precedent.
    if (!secretData) {
      void navigate({ to: webhooksPath })
    }
  }, [webhooksPath, secretData, navigate])

  const handleCopy = useCallback(async () => {
    if (!secretData?.secret) return
    await copyToClipboard(secretData.secret)
  }, [secretData, copyToClipboard])

  const handleDone = () => {
    void navigate({ to: webhooksPath })
  }

  if (!secretData) {
    return null
  }

  return (
    <div className="space-y-6" data-testid="webhook-secret-reveal-page">
      <div className="flex items-center gap-4">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={handleDone}
          data-testid="webhook-secret-reveal-back-button"
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div>
          <h1 className="text-2xl font-bold" data-testid="page-title">
            {m['webhooks.reveal_title']()}
          </h1>
          <p className="text-muted-foreground text-sm">{m['webhooks.reveal_subtitle']()}</p>
        </div>
      </div>

      <Alert className="border-warning/50 bg-warning/10 text-warning">
        <TriangleAlert className="h-4 w-4 !text-warning" />
        <AlertDescription>{m['webhooks.reveal_warning']()}</AlertDescription>
      </Alert>

      <Card>
        <CardContent className="space-y-4 pt-6">
          <div className="space-y-1">
            <p className="text-[11px] font-medium uppercase tracking-[0.1em] text-muted-foreground">
              {m['webhooks.endpoint_url_label']()}
            </p>
            <p className="break-all font-mono text-sm">{secretData.endpointUrl}</p>
          </div>

          <div className="space-y-2">
            <p className="text-[11px] font-medium uppercase tracking-[0.1em] text-muted-foreground">
              {m['webhooks.reveal_title']()}
            </p>
            <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
              <code
                className="flex-1 rounded-lg bg-instrument px-3 py-2.5 font-mono text-sm text-gold shadow-led break-all select-all"
                data-testid="webhook-secret-reveal-value"
              >
                {secretData.secret}
              </code>
              <Button
                variant="outline"
                size="sm"
                onClick={handleCopy}
                className="shrink-0"
                data-testid="copy-webhook-secret-button"
              >
                {copied ? (
                  <>
                    <Check className="mr-2 h-4 w-4" />
                    {m['webhooks.copied']()}
                  </>
                ) : (
                  <>
                    <Copy className="mr-2 h-4 w-4" />
                    {m['webhooks.copy']()}
                  </>
                )}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      <div className="flex justify-end">
        <Button onClick={handleDone} data-testid="webhook-secret-reveal-done-button">
          {m['webhooks.done']()}
        </Button>
      </div>
    </div>
  )
}
