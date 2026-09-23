import { http, HttpResponse, delay } from 'msw'
import { STRIPE_ERROR_CODES, STRIPE_KEY_PREFIXES, PAYMENT_PROVIDERS } from '@/lib/billing-constants'

/**
 * MSW handlers for Stripe payment integration tests
 * Covers checkout creation, config management, and error scenarios
 */

// ===== Checkout Response Scenarios =====

const CHECKOUT_SUCCESS_RESPONSE = {
  checkoutUrl: 'https://checkout.stripe.com/pay/cs_test_1234567890',
  sessionId: 'cs_test_1234567890',
  provider: PAYMENT_PROVIDERS.STRIPE,
}

const CHECKOUT_ERROR_SCENARIOS: Record<string, { status: number; body: object }> = {
  'invalid-key-test': {
    status: 400,
    body: {
      message: 'Invalid Stripe API key format. Expected: sk_test_... or sk_live_...',
      code: STRIPE_ERROR_CODES.INVALID_API_KEY,
    },
  },
  'webhook-missing-test': {
    status: 400,
    body: {
      message: 'Stripe webhook not configured. Please configure webhook secret in realm settings.',
      code: STRIPE_ERROR_CODES.WEBHOOK_NOT_CONFIGURED,
    },
  },
  'auth-error-test': {
    status: 401,
    body: {
      message: 'Unauthorized: Invalid or missing credentials',
      code: STRIPE_ERROR_CODES.UNAUTHORIZED,
    },
  },
  'forbidden-test': {
    status: 403,
    body: {
      message: 'Forbidden: You do not have permission to access this resource',
      code: STRIPE_ERROR_CODES.FORBIDDEN,
    },
  },
  'validation-error-test': {
    status: 422,
    body: {
      message: 'Validation failed',
      code: STRIPE_ERROR_CODES.VALIDATION_ERROR,
      errors: {
        entitlementKey: ['Entitlement key not found'],
        paymentProvider: ['Must be a valid payment provider'],
      },
    },
  },
  'service-unavailable-test': {
    status: 503,
    body: {
      message: 'Stripe service temporarily unavailable',
      code: STRIPE_ERROR_CODES.SERVICE_UNAVAILABLE,
    },
  },
}

// ===== Checkout Creation Handlers =====

/**
 * Unified checkout handler that handles all scenarios based on entitlementKey
 */
const handleCheckout = async ({ request }: { request: Request }) => {
  const body = (await request.json()) as { entitlementKey?: string; paymentProvider?: string }
  const entitlementKey = body.entitlementKey

  // Handle timeout scenario
  if (entitlementKey === 'timeout-test') {
    await delay(30000)
    return HttpResponse.json(CHECKOUT_SUCCESS_RESPONSE)
  }

  // Check for error scenarios
  const errorScenario = CHECKOUT_ERROR_SCENARIOS[entitlementKey || '']
  if (errorScenario) {
    return HttpResponse.json(errorScenario.body, { status: errorScenario.status })
  }

  // Default success response
  return HttpResponse.json(CHECKOUT_SUCCESS_RESPONSE)
}

export const stripeHandlers = [
  // ===== Checkout Creation Handler =====
  http.post('/api/bill/client/:clientAppId/checkout', handleCheckout),

  // ===== Realm Config Handlers =====

  /**
   * Get Stripe configuration
   */
  http.get('/api/:realmId/config', async ({ request }) => {
    const url = new URL(request.url)
    const configType = url.searchParams.get('configType')

    if (configType === 'stripe') {
      return HttpResponse.json({
        configs: [
          {
            configType: 'stripe',
            configKey: 'settings',
            configValue: JSON.stringify({
              enabled: true,
              publishableKey: 'pk_test_123456789',
              secretKey: 'sk_test_••••••••',
              webhookSecret: 'whsec_••••••••',
            }),
            enabled: true,
          },
        ],
      })
    }

    return HttpResponse.json({ configs: [] })
  }),

  /**
   * Unified config save handler with validation and error scenarios
   */
  http.post('/api/:realmId/config', async ({ request }) => {
    const body = await request.json()

    if (!Array.isArray(body)) {
      return HttpResponse.json({ success: true })
    }

    const settingsConfig = body.find((c: any) => c.configKey === 'settings')
    if (!settingsConfig) {
      return HttpResponse.json({ success: true })
    }

    try {
      const parsed = JSON.parse(settingsConfig.configValue)

      // Test scenarios
      if (parsed.secretKey === 'auth-error-test') {
        return HttpResponse.json(
          {
            message: 'Unauthorized: Invalid or missing credentials',
            code: STRIPE_ERROR_CODES.UNAUTHORIZED,
          },
          { status: 401 }
        )
      }

      if (parsed.secretKey === 'forbidden-test') {
        return HttpResponse.json(
          {
            message: 'Forbidden: You do not have permission to update realm config',
            code: STRIPE_ERROR_CODES.FORBIDDEN,
          },
          { status: 403 }
        )
      }

      // Validate publishable key format
      if (
        parsed.publishableKey &&
        !parsed.publishableKey.startsWith(STRIPE_KEY_PREFIXES.PUBLISHABLE)
      ) {
        return HttpResponse.json(
          {
            message: `Invalid Stripe public key format. Expected: ${STRIPE_KEY_PREFIXES.PUBLISHABLE}test_... or ${STRIPE_KEY_PREFIXES.PUBLISHABLE}live_...`,
            code: STRIPE_ERROR_CODES.INVALID_PUBLIC_KEY_FORMAT,
          },
          { status: 400 }
        )
      }

      // Validate secret key format
      if (parsed.secretKey && !parsed.secretKey.startsWith(STRIPE_KEY_PREFIXES.SECRET)) {
        return HttpResponse.json(
          {
            message: `Invalid Stripe secret key format. Expected: ${STRIPE_KEY_PREFIXES.SECRET}test_... or ${STRIPE_KEY_PREFIXES.SECRET}live_...`,
            code: STRIPE_ERROR_CODES.INVALID_SECRET_KEY_FORMAT,
          },
          { status: 400 }
        )
      }

      // Validate webhook secret format
      if (
        parsed.webhookSecret &&
        parsed.webhookSecret !== '' &&
        !parsed.webhookSecret.startsWith(STRIPE_KEY_PREFIXES.WEBHOOK)
      ) {
        return HttpResponse.json(
          {
            message: `Invalid webhook secret format. Expected: ${STRIPE_KEY_PREFIXES.WEBHOOK}...`,
            code: STRIPE_ERROR_CODES.INVALID_WEBHOOK_SECRET_FORMAT,
          },
          { status: 400 }
        )
      }

      // Validate required fields
      if (!parsed.secretKey || !parsed.publishableKey) {
        return HttpResponse.json(
          {
            message: 'Both publishable key and secret key are required',
          },
          { status: 400 }
        )
      }
    } catch (e) {
      return HttpResponse.json(
        {
          message: 'Invalid configuration JSON format',
        },
        { status: 400 }
      )
    }

    return HttpResponse.json({ success: true })
  }),
]

/**
 * Helper function to simulate network failure
 * @param errorMessage - Error message to return
 * @param statusCode - HTTP status code (default: 503)
 * @param delayMs - Delay before response in ms (default: 100)
 */
export function simulateNetworkFailure(
  errorMessage: string,
  statusCode: number = 503,
  delayMs: number = 100
) {
  return http.post('/api/bill/client/:clientAppId/checkout', async () => {
    await delay(delayMs)
    return HttpResponse.json(
      {
        message: errorMessage,
        code: STRIPE_ERROR_CODES.NETWORK_ERROR,
      },
      { status: statusCode }
    )
  })
}

/**
 * Helper function to simulate retry scenario
 * @param failCount - Number of times to fail before success
 * @param delayMs - Delay before each response in ms
 */
export function simulateRetryScenario(failCount: number, delayMs: number = 100) {
  let attempts = 0
  return http.post('/api/bill/client/:clientAppId/checkout', async () => {
    await delay(delayMs)
    attempts++

    if (attempts <= failCount) {
      return HttpResponse.json(
        {
          message: 'Service temporarily unavailable',
          code: STRIPE_ERROR_CODES.SERVICE_UNAVAILABLE,
        },
        { status: 503 }
      )
    }

    return HttpResponse.json(CHECKOUT_SUCCESS_RESPONSE)
  })
}
