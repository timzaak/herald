import { afterEach, describe, expect, it } from 'vitest'
import { setLocale } from '@/paraglide/runtime'
import { resolveConfigSaveErrorMessage } from '../use-save-config-mutation'

afterEach(() => {
  setLocale('en', { reload: false })
})

describe('resolveConfigSaveErrorMessage', () => {
  it('localizes stable realm-config errors in English and Chinese', () => {
    // WHY: realm configuration errors are safe for users but backend English
    // must not leak through when the console is running in Chinese.
    const cases = [
      {
        backend: 'Payment provider base_url overrides are disabled in production',
        en: 'Payment provider endpoint overrides are not allowed in production.',
        zh: '生产环境不允许覆盖支付服务商接口地址。',
      },
      {
        backend: 'Secret value is required',
        en: 'A required provider credential is missing.',
        zh: '缺少必填的支付服务商凭据。',
      },
      {
        backend: 'Failed to load existing provider secret',
        en: 'Unable to load the existing provider credentials. Please try again.',
        zh: '无法读取现有支付服务商凭据，请稍后重试。',
      },
      {
        backend: 'Failed to batch upsert realm configs',
        en: 'The server could not save the provider configuration. Please try again.',
        zh: '服务端无法保存支付服务商配置，请稍后重试。',
      },
    ]

    for (const item of cases) {
      expect(resolveConfigSaveErrorMessage({ error: { message: item.backend } })).toBe(item.en)
    }

    setLocale('zh-CN', { reload: false })
    for (const item of cases) {
      expect(resolveConfigSaveErrorMessage({ error: { message: item.backend } })).toBe(item.zh)
    }
  })

  it('preserves unknown backend validation messages and localizes a missing message', () => {
    expect(resolveConfigSaveErrorMessage({ message: 'Provider validation failed' })).toBe(
      'Provider validation failed'
    )
    expect(resolveConfigSaveErrorMessage({})).toBe('Unknown error')

    setLocale('zh-CN', { reload: false })
    expect(resolveConfigSaveErrorMessage({})).toBe('未知错误')
  })
})
