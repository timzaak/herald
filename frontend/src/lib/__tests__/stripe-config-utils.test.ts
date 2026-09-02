import { describe, test, expect } from 'vitest'
import { parseStripeConfig, buildStripeConfigRequest } from '../stripe-config-utils'
import type { RealmConfigResponse } from '@/lib/api-generated'
import type { StripeConfigForm } from '@/lib/schemas/stripe-config'
import { PAYMENT_PROVIDERS, STRIPE_CONFIG_KEYS } from '@/lib/billing-constants'

describe('stripe-config-utils', () => {
  describe('parseStripeConfig', () => {
    test('parses valid Stripe config from realm config array', () => {
      const configs: RealmConfigResponse[] = [
        {
          configType: PAYMENT_PROVIDERS.STRIPE,
          configKey: STRIPE_CONFIG_KEYS.PUBLISHABLE_KEY,
          configValue: 'pk_test_123456789',
          enabled: true,
        },
        {
          configType: PAYMENT_PROVIDERS.STRIPE,
          configKey: STRIPE_CONFIG_KEYS.API_KEY,
          configValue: 'sk_test_987654321',
          enabled: true,
        },
        {
          configType: PAYMENT_PROVIDERS.STRIPE,
          configKey: STRIPE_CONFIG_KEYS.WEBHOOK_SECRET,
          configValue: 'whsec_abcdef',
          enabled: true,
        },
      ]

      const result = parseStripeConfig(configs)

      expect(result).toEqual({
        publishableKey: 'pk_test_123456789',
        secretKey: '',
        webhookSecret: '',
        asyncPointsStrategy: 'conservative',
      })
    })

    test('returns default config when Stripe config is missing', () => {
      const configs: RealmConfigResponse[] = []

      const result = parseStripeConfig(configs)

      expect(result).toEqual({
        publishableKey: '',
        secretKey: '',
        webhookSecret: '',
        asyncPointsStrategy: 'conservative',
      })
    })

    test('handles missing optional fields gracefully', () => {
      const configs: RealmConfigResponse[] = [
        {
          configType: PAYMENT_PROVIDERS.STRIPE,
          configKey: STRIPE_CONFIG_KEYS.PUBLISHABLE_KEY,
          configValue: 'pk_test_123',
          enabled: true,
        },
        // API_KEY and WEBHOOK_SECRET are missing
      ]

      const result = parseStripeConfig(configs)

      expect(result).toEqual({
        publishableKey: 'pk_test_123',
        secretKey: '',
        webhookSecret: '',
        asyncPointsStrategy: 'conservative',
      })
    })

    test('ignores non-stripe configs', () => {
      const configs: RealmConfigResponse[] = [
        {
          configType: 'creem',
          configKey: 'settings',
          configValue: JSON.stringify({ enabled: true }),
          enabled: true,
        },
        {
          configType: PAYMENT_PROVIDERS.STRIPE,
          configKey: STRIPE_CONFIG_KEYS.PUBLISHABLE_KEY,
          configValue: 'pk_test_123',
          enabled: true,
        },
      ]

      const result = parseStripeConfig(configs)

      expect(result).toEqual({
        publishableKey: 'pk_test_123',
        secretKey: '',
        webhookSecret: '',
        asyncPointsStrategy: 'conservative',
      })
    })

    test('parses eager async points strategy', () => {
      const configs: RealmConfigResponse[] = [
        {
          configType: PAYMENT_PROVIDERS.STRIPE,
          configKey: STRIPE_CONFIG_KEYS.ASYNC_POINTS_STRATEGY,
          configValue: 'eager',
          enabled: true,
        },
      ]

      const result = parseStripeConfig(configs)

      expect(result.asyncPointsStrategy).toBe('eager')
    })
  })

  describe('buildStripeConfigRequest', () => {
    test('builds correct upsert request for full Stripe config', () => {
      const formData: StripeConfigForm = {
        publishableKey: 'pk_test_123',
        secretKey: 'sk_test_456',
        webhookSecret: 'whsec_789',
        asyncPointsStrategy: 'conservative',
      }

      const result = buildStripeConfigRequest(formData)

      expect(result).toEqual([
        {
          configType: PAYMENT_PROVIDERS.STRIPE,
          configKey: STRIPE_CONFIG_KEYS.PUBLISHABLE_KEY,
          configValue: 'pk_test_123',
          isSecret: false,
          enabled: true,
        },
        {
          configType: PAYMENT_PROVIDERS.STRIPE,
          configKey: STRIPE_CONFIG_KEYS.API_KEY,
          configValue: 'sk_test_456',
          isSecret: true,
          enabled: true,
        },
        {
          configType: PAYMENT_PROVIDERS.STRIPE,
          configKey: STRIPE_CONFIG_KEYS.WEBHOOK_SECRET,
          configValue: 'whsec_789',
          isSecret: true,
          enabled: true,
        },
        {
          configType: PAYMENT_PROVIDERS.STRIPE,
          configKey: STRIPE_CONFIG_KEYS.ASYNC_POINTS_STRATEGY,
          configValue: 'conservative',
          isSecret: false,
          enabled: true,
        },
      ])
    })

    test('builds request with optional webhook secret omitted', () => {
      const formData: StripeConfigForm = {
        publishableKey: 'pk_test_123',
        secretKey: 'sk_test_456',
        webhookSecret: '',
        asyncPointsStrategy: 'conservative',
      }

      const result = buildStripeConfigRequest(formData)

      // Empty secrets are filtered out
      expect(result.find((r) => r.configKey === STRIPE_CONFIG_KEYS.WEBHOOK_SECRET)).toBeUndefined()
      expect(result.find((r) => r.configKey === STRIPE_CONFIG_KEYS.API_KEY)).toBeDefined()
    })
    // Note: enabled/isSecret/configType flags are pinned by the full deep-equal
    // test above; no separate per-flag tests needed.
  })
})
