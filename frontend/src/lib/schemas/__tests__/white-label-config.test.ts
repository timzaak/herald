import { describe, it, expect } from 'vitest'
import { whiteLabelConfigSchema, whiteLabelBackgroundSchema } from '../realm-config'

/**
 * Factory: an all-null white-label config, i.e. the shape a brand-new unconfigured
 * realm's form holds. Every field is nullable but *required*, so a valid config is
 * the full object with every value set to `null`. Overrides merge on top so
 * individual tests can focus on one field without re-declaring the whole shape.
 */
function makeWhiteLabelConfig(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    brandName: null,
    logoUrl: null,
    faviconUrl: null,
    accentColor: null,
    background: null,
    footerText: null,
    loginTitle: null,
    loginSubtitle: null,
    registerTitle: null,
    registerSubtitle: null,
    ...overrides,
  }
}

/** All top-level string-typed fields (background is an object, tested separately). */
const STRING_FIELDS = [
  'brandName',
  'logoUrl',
  'faviconUrl',
  'accentColor',
  'footerText',
  'loginTitle',
  'loginSubtitle',
  'registerTitle',
  'registerSubtitle',
] as const

/** Every top-level field — used to prove each is required even though nullable. */
const ALL_FIELDS = [
  'brandName',
  'logoUrl',
  'faviconUrl',
  'accentColor',
  'background',
  'footerText',
  'loginTitle',
  'loginSubtitle',
  'registerTitle',
  'registerSubtitle',
] as const

describe('whiteLabelConfigSchema', () => {
  describe('unconfigured realm accepts an all-null config', () => {
    it('accepts a config where every field is null', () => {
      const result = whiteLabelConfigSchema.safeParse(makeWhiteLabelConfig())

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data).toEqual({
          brandName: null,
          logoUrl: null,
          faviconUrl: null,
          accentColor: null,
          background: null,
          footerText: null,
          loginTitle: null,
          loginSubtitle: null,
          registerTitle: null,
          registerSubtitle: null,
        })
      }
    })
  })

  describe('form can hold empty inputs before normalization', () => {
    // The form schema intentionally accepts empty strings: an admin may clear a
    // field on screen before clicking save. Empty-string -> null normalization
    // happens later in `toUpdateWhiteLabelConfigRequest`, not at the schema level.
    it.each(STRING_FIELDS)('accepts empty string for %s', (field) => {
      const result = whiteLabelConfigSchema.safeParse(makeWhiteLabelConfig({ [field]: '' }))

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data[field as keyof typeof result.data]).toBe('')
      }
    })

    it('accepts whitespace-only strings (schema performs no trimming)', () => {
      const result = whiteLabelConfigSchema.safeParse(makeWhiteLabelConfig({ logoUrl: '   ' }))

      expect(result.success).toBe(true)
      if (result.success) {
        // Schema does not trim; the request builder does. This keeps the two
        // responsibilities decoupled and is asserted here to guard the boundary.
        expect(result.data.logoUrl).toBe('   ')
      }
    })
  })

  describe('background.type enum enforcement', () => {
    it.each(['image', 'gradient'] as const)('accepts background.type=%s', (type) => {
      const result = whiteLabelConfigSchema.safeParse(
        makeWhiteLabelConfig({ background: { type, value: 'https://cdn.example.com/bg' } })
      )

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.background).toEqual({ type, value: 'https://cdn.example.com/bg' })
      }
    })

    it.each(['video', 'solid', 'color', 'IMAGE', ''])(
      'rejects background.type=%s (not part of the enum)',
      (type) => {
        const result = whiteLabelConfigSchema.safeParse(
          makeWhiteLabelConfig({ background: { type, value: 'x' } })
        )

        expect(result.success).toBe(false)
      }
    )

    it('accepts background.value as any string including empty', () => {
      // value is a free-form URL or gradient string; empty is allowed at the schema
      // level so the form can temporarily hold a cleared value.
      const result = whiteLabelConfigSchema.safeParse(
        makeWhiteLabelConfig({ background: { type: 'gradient', value: '' } })
      )

      expect(result.success).toBe(true)
    })

    it('rejects background missing the value field', () => {
      const result = whiteLabelConfigSchema.safeParse(
        makeWhiteLabelConfig({ background: { type: 'image' } })
      )

      expect(result.success).toBe(false)
    })

    it('accepts background: null (no background configured)', () => {
      const result = whiteLabelConfigSchema.safeParse(makeWhiteLabelConfig({ background: null }))

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.background).toBeNull()
      }
    })
  })

  describe('logoUrl accepts any string (no schema-level URL validation)', () => {
    // NOTE: the design contract mentions URL-shape validation, but the schema as
    // implemented is `z.string().nullable()` with no `.url()` refinement. URL
    // scheme/shape enforcement lives on the backend (PUT /draft validates
    // http/https); the frontend schema only guarantees "string or null". These
    // assertions pin the actual frontend contract so downstream form tests can
    // rely on it.
    it.each([
      'https://cdn.example.com/logo.svg',
      'http://intranet.example/logo.png',
      'not-a-url',
      'ftp://files.example/logo',
      '/local/path.svg',
    ])('accepts logoUrl=%s', (logoUrl) => {
      const result = whiteLabelConfigSchema.safeParse(makeWhiteLabelConfig({ logoUrl }))

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data.logoUrl).toBe(logoUrl)
      }
    })
  })

  describe('string fields reject non-string values', () => {
    it.each(STRING_FIELDS)('rejects number for %s', (field) => {
      const result = whiteLabelConfigSchema.safeParse(makeWhiteLabelConfig({ [field]: 5 }))

      expect(result.success).toBe(false)
    })

    it('rejects boolean for logoUrl', () => {
      const result = whiteLabelConfigSchema.safeParse(makeWhiteLabelConfig({ logoUrl: true }))

      expect(result.success).toBe(false)
    })

    it('rejects object for accentColor', () => {
      const result = whiteLabelConfigSchema.safeParse(
        makeWhiteLabelConfig({ accentColor: { hex: '#fff' } })
      )

      expect(result.success).toBe(false)
    })

    // WHY: accentColor rides the public-config channel verbatim, so only hex
    // colors (and the empty "unset" string) are valid — a CSS function or
    // free text must be rejected client-side, mirroring the backend's
    // validate_hex_color.
    it('rejects non-hex accentColor strings but accepts hex and empty', () => {
      const invalid = ['url(https://evil.example/x)', 'red', 'rgb(37, 99, 235)', '2563eb']
      for (const value of invalid) {
        expect(
          whiteLabelConfigSchema.safeParse(makeWhiteLabelConfig({ accentColor: value })).success
        ).toBe(false)
      }

      const valid = ['#2563eb', '#fff', '#ffffff80', '']
      for (const value of valid) {
        expect(
          whiteLabelConfigSchema.safeParse(makeWhiteLabelConfig({ accentColor: value })).success
        ).toBe(true)
      }
    })
  })

  describe('every field is nullable but required (never optional)', () => {
    // Distinguishing nullable (T | null) from optional (T | null | undefined):
    // the form always carries every key, so an absent key is a programmer error
    // and must fail parsing. This protects downstream code that reads these keys
    // without guarding for undefined.
    it.each(ALL_FIELDS)('rejects when %s is missing', (field) => {
      const config = makeWhiteLabelConfig()
      delete config[field]

      expect(whiteLabelConfigSchema.safeParse(config).success).toBe(false)
    })

    it.each(ALL_FIELDS)('rejects when %s is undefined', (field) => {
      const config = makeWhiteLabelConfig({ [field]: undefined })

      expect(whiteLabelConfigSchema.safeParse(config).success).toBe(false)
    })

    it('rejects an empty object', () => {
      expect(whiteLabelConfigSchema.safeParse({}).success).toBe(false)
    })

    it('rejects top-level null and undefined', () => {
      expect(whiteLabelConfigSchema.safeParse(null).success).toBe(false)
      expect(whiteLabelConfigSchema.safeParse(undefined).success).toBe(false)
    })
  })

  describe('unknown fields are stripped, not rejected', () => {
    // Default zod object behavior: extra keys are silently dropped. This is the
    // intended contract — `normalizeWhiteLabelConfig` relies on it to ignore
    // backend-added fields (e.g. `updatedAt`) without failing.
    it('drops extra keys and keeps the known ones', () => {
      const result = whiteLabelConfigSchema.safeParse({
        ...makeWhiteLabelConfig({ logoUrl: 'https://x/logo.svg' }),
        updatedAt: '2026-07-08T00:00:00Z',
        extra: 'should-be-removed',
      })

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.data).not.toHaveProperty('updatedAt')
        expect(result.data).not.toHaveProperty('extra')
        expect(result.data.logoUrl).toBe('https://x/logo.svg')
      }
    })
  })
})

describe('whiteLabelBackgroundSchema', () => {
  // Covered indirectly through whiteLabelConfigSchema above, but asserted in
  // isolation here to pin the standalone export used by typing helpers.
  it.each(['image', 'gradient'] as const)('accepts type=%s with a value', (type) => {
    expect(whiteLabelBackgroundSchema.safeParse({ type, value: 'v' }).success).toBe(true)
  })

  it('requires both type and value', () => {
    expect(whiteLabelBackgroundSchema.safeParse({ type: 'image' }).success).toBe(false)
    expect(whiteLabelBackgroundSchema.safeParse({ value: 'v' }).success).toBe(false)
  })
})
