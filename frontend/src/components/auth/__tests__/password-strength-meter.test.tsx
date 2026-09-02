import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { PasswordStrengthMeter } from '../password-strength-meter'
import type { PasswordConfig } from '@/lib/password-strength'

describe('PasswordStrengthMeter', () => {
  const defaultConfig: PasswordConfig = {
    minLength: 8,
    requireUppercase: true,
    requireLowercase: true,
    requireNumber: true,
    requireSpecialChar: true,
  }

  describe('rendering', () => {
    it('GIVEN empty password WHEN rendering THEN renders nothing', async () => {
      const screen = render(<PasswordStrengthMeter password="" config={defaultConfig} />)
      expect(screen.container.textContent).toBe('')
      expect(screen.container.querySelector('ul')).not.toBeInTheDocument()
    })
  })

  describe('suggestions', () => {
    it('GIVEN password with missing requirements WHEN rendering THEN displays all relevant suggestions', async () => {
      const screen = render(<PasswordStrengthMeter password="abc" config={defaultConfig} />)
      expect(screen.getByText(/must be at least 8 characters/)).toBeInTheDocument()
      expect(screen.getByText(/must contain uppercase letters/)).toBeInTheDocument()
      expect(screen.getByText(/must contain numbers/)).toBeInTheDocument()
      expect(screen.getByText(/must contain special characters/)).toBeInTheDocument()
      const list = screen.container.querySelector('ul')
      expect(list).toBeInTheDocument()
    })

    it('GIVEN password missing lowercase WHEN rendering THEN displays lowercase suggestion', async () => {
      const screen = render(
        <PasswordStrengthMeter password="PASSWORD123!" config={defaultConfig} />
      )
      expect(screen.getByText(/must contain lowercase letters/)).toBeInTheDocument()
    })

    it('GIVEN strong password WHEN rendering THEN hides suggestions', async () => {
      const screen = render(
        <PasswordStrengthMeter password="Password123!" config={defaultConfig} />
      )
      const list = screen.container.querySelector('ul')
      expect(list).not.toBeInTheDocument()
    })
  })
})
