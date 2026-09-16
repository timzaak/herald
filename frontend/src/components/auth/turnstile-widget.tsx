import { Turnstile } from '@marsidev/react-turnstile'
import React from 'react'

interface TurnstileWidgetProps {
  siteKey: string
  onTokenChange: (token: string | null) => void
  onError: (error: string) => void
}

export function TurnstileWidget({ siteKey, onTokenChange, onError }: TurnstileWidgetProps) {
  const [error, setError] = React.useState<string | null>(null)

  const handleSuccess = (token: string) => {
    setError(null)
    onTokenChange(token)
  }

  const handleError = (errorCode: string) => {
    setError(errorCode)
    onError(errorCode)
  }

  const handleExpire = () => {
    onTokenChange(null)
  }

  return (
    <div className="turnstile-widget-container w-full overflow-x-auto">
      <Turnstile
        siteKey={siteKey}
        onSuccess={handleSuccess}
        onError={handleError}
        onExpire={handleExpire}
        options={{ theme: 'auto', size: 'normal' }}
      />
      {error && <div className="text-sm text-destructive mt-1">{error}</div>}
    </div>
  )
}
