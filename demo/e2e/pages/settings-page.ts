/**
 * Settings Page Object
 *
 * Encapsulates Settings page operations.
 * Provides methods for managing Realm configuration (TOTP, Registration).
 *
 * @see ../../../spec/demo/e2e-testing.md#page-object-model-pom-规范
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from '../helpers/unified-logger'

/**
 * TOTP Configuration Data
 */
export interface TOTPConfigData {
  enabled: boolean          // Realm 是否启用 TOTP
  force_enabled: boolean    // 是否强制所有用户启用
}

/**
 * Registration Configuration Data
 */
export interface RegistrationConfigData {
  enabled: boolean                        // 是否允许注册
  require_email_verification: boolean     // 是否需要邮箱验证
}

/**
 * OAuth Provider Configuration Data
 */
export interface OAuthProviderConfigData {
  providerType: 'google' | 'github' | 'facebook' | 'apple' | 'wechat' | 'wechat_miniprogram'
  clientId: string
  clientSecret: string
  scopes?: string[]
  enabled?: boolean
}

/**
 * Email Configuration Form Data
 * Matches frontend EmailConfigForm schema.
 */
export interface EmailConfigFormData {
  provider: 'resend' | 'smtp'
  fromAddress: string
  resendApiKey?: string
  smtpHost?: string
  smtpPort?: string
  smtpEncryption?: 'starttls' | 'ssl'
  smtpUsername?: string
  smtpPassword?: string
}

/**
 * White-label background form fragment.
 * Matches frontend WhiteLabelBackgroundForm (`type` is image|gradient).
 */
export interface WhiteLabelBackgroundForm {
  type: 'image' | 'gradient'
  value: string
}

/**
 * White-label Configuration Form Data
 *
 * Mirrors the frontend `WhiteLabelConfigForm` schema (see
 * frontend/src/lib/schemas/realm-config.ts and the form component
 * white-label-config-form.tsx). All fields are nullable; an empty/null
 * value makes the public auth-page wrapper fall back to Herald defaults.
 *
 * `background` is `null` when no background is configured (the form's
 * background-type select sits at `none`); when set, `type` is `image`
 * (URL) or `gradient` (CSS gradient string).
 */
export interface WhiteLabelFormValues {
  logoUrl: string | null
  accentColor: string | null
  background: WhiteLabelBackgroundForm | null
  footerText: string | null
  loginTitle: string | null
  loginSubtitle: string | null
  registerTitle: string | null
  registerSubtitle: string | null
}

/**
 * Custom-domain Configuration Form Data
 *
 * Mirrors the frontend `CustomDomainConfigForm` schema (see
 * frontend/src/lib/schemas/realm-config.ts and the form component
 * custom-domain-config-form.tsx). The only field is `hostname` — the custom
 * login domain a realm admin CNAMEs to the Herald-owned `cnameTarget`.
 */
export interface CustomDomainFormValues {
  hostname: string
}

/**
 * Email-OTP Configuration Data
 *
 * Mirrors the frontend `EmailOtpConfigForm` schema (see
 * frontend/src/lib/schemas/realm-config.ts and the OTP section of
 * email-config-form.tsx). Field names use snake_case to match the existing
 * `TOTPConfigData` convention even though the wire schema is camelCase.
 */
export interface EmailOtpConfigData {
  enabled: boolean       // Whether Email-OTP login is enabled for the realm
  auto_register: boolean // Whether to auto-register unverified emails on success
}

/**
 * Settings Page Object
 *
 * Represents Settings page at /admin/settings
 *
 * @example
 * ```typescript
 * const settingsPage = new SettingsPage(page, logger, 'admin')
 * await settingsPage.goto()
 * await settingsPage.enableTOTP()
 * await settingsPage.saveTOTPConfig()
 * ```
 */
export class SettingsPage extends BasePage {
  private readonly realmId: string

  // Page container
  readonly container: Locator
  readonly heading: Locator

  // Tabs
  readonly totpTab: Locator
  readonly registrationTab: Locator
  readonly providersTab: Locator

  // TOTP Configuration elements
  readonly totpEnabledSwitch: Locator
  readonly totpForceEnabledSwitch: Locator
  readonly totpSaveButton: Locator

  // Registration Configuration elements
  readonly allowRegistrationSwitch: Locator
  readonly requireEmailVerificationSwitch: Locator
  readonly registrationSaveButton: Locator

  // Platform Self-Service Signup Configuration elements (admin realm only)
  readonly platformSignupTab: Locator
  readonly platformSignupEnabledSwitch: Locator
  readonly platformSignupSaveButton: Locator

  // OAuth Provider Configuration elements
  readonly addProviderButton: Locator
  readonly providerTypeSelect: Locator
  readonly clientIdInput: Locator
  readonly clientSecretInput: Locator
  readonly scopesInput: Locator
  readonly enabledCheckbox: Locator
  readonly saveProviderButton: Locator
  readonly cancelProviderButton: Locator

  // Email Configuration elements
  readonly emailTab: Locator
  readonly emailStatusBadge: Locator
  readonly emailStatusError: Locator
  readonly emailProviderResend: Locator
  readonly emailProviderSmtp: Locator
  readonly emailFromAddressInput: Locator
  readonly emailResendApiKeyInput: Locator
  readonly emailSmtpHostInput: Locator
  readonly emailSmtpPortInput: Locator
  readonly emailSmtpEncryptionSelect: Locator
  readonly emailSmtpUsernameInput: Locator
  readonly emailSmtpPasswordInput: Locator
  readonly emailTestRecipientInput: Locator
  readonly emailTestButton: Locator
  readonly emailTestError: Locator
  readonly emailTestSuccess: Locator
  readonly emailSaveButton: Locator
  readonly emailSaveError: Locator

  // White-label Configuration elements
  readonly whiteLabelTab: Locator
  readonly whiteLabelLogoUrlInput: Locator
  readonly whiteLabelAccentColorPicker: Locator
  readonly whiteLabelAccentColorInput: Locator
  readonly whiteLabelAccentWarning: Locator
  readonly whiteLabelBackgroundTypeSelect: Locator
  readonly whiteLabelBackgroundValueTextarea: Locator
  readonly whiteLabelFooterTextInput: Locator
  readonly whiteLabelLoginTitleInput: Locator
  readonly whiteLabelLoginSubtitleInput: Locator
  readonly whiteLabelRegisterTitleInput: Locator
  readonly whiteLabelRegisterSubtitleInput: Locator
  readonly whiteLabelDraftNotice: Locator
  readonly whiteLabelSaveDraftButton: Locator
  readonly whiteLabelPublishButton: Locator
  readonly whiteLabelDiscardDraftButton: Locator
  readonly whiteLabelRestoreButton: Locator
  readonly whiteLabelRestoreDialog: Locator
  readonly whiteLabelRestoreConfirmButton: Locator
  readonly whiteLabelPreviewLoginPanel: Locator
  readonly whiteLabelPreviewRegisterPanel: Locator

  // Custom-domain Configuration elements
  readonly customDomainTab: Locator
  readonly customDomainHostnameInput: Locator
  readonly customDomainCnameGuidance: Locator
  readonly customDomainStatusCname: Locator
  readonly customDomainStatusTls: Locator
  readonly customDomainRefreshStatusButton: Locator
  readonly customDomainSaveButton: Locator

  // Email-OTP Configuration elements
  //
  // NOTE: Email-OTP is no longer a top-level tab. After frontend commit 364767b2
  // ("merged email-otp settings"), the two switches and save button live inside
  // the `email` tab as a `data-testid="email-otp-section"` sub-block of
  // EmailConfigForm. There is no `email-otp-tab` testid anymore — navigation to
  // the OTP controls goes through `email-tab` (see switchToEmailOtpTab below).
  readonly emailOtpSection: Locator
  readonly emailOtpEnabledSwitch: Locator
  readonly emailOtpAutoRegisterSwitch: Locator
  readonly emailOtpSaveButton: Locator

  // LDAP (corporate directory) Configuration — standalone `ldap` tab. The form
  // card root has no testid; the url input doubles as the form-ready anchor.
  readonly ldapTab: Locator
  readonly ldapEnabledSwitch: Locator
  readonly ldapUrlInput: Locator
  readonly ldapStarttlsSwitch: Locator
  readonly ldapBaseDnInput: Locator
  readonly ldapBindDnInput: Locator
  readonly ldapBindPasswordInput: Locator
  readonly ldapUserFilterInput: Locator
  readonly ldapMailAttributeInput: Locator
  readonly ldapSaveButton: Locator

  constructor(page: Page, logger?: UnifiedLogger, realmId: string = 'admin') {
    super(page, logger)
    this.realmId = realmId

    // Page container
    this.container = page.getByTestId('settings-page')
    this.heading = page.getByRole('heading', { name: 'Settings' })

    // Tabs - using data-testid selectors for reliability
    this.totpTab = page.getByTestId('totp-tab')
    this.registrationTab = page.getByTestId('registration-tab')
    this.providersTab = page.getByTestId('providers-tab')

    // TOTP Configuration - using data-testid (semantic selectors not available)
    // ⚠️ Note: The Switch component doesn't set aria-labelledby,
    // so we use data-testid as the primary reliable selector
    // Future improvement: Add aria-label or aria-labelledby to Switch component
    this.totpEnabledSwitch = page.getByTestId('totp-enabled-switch')
    this.totpForceEnabledSwitch = page.getByTestId('totp-force-enabled-switch')

    // Save button in TOTP form
    this.totpSaveButton = page.getByTestId('totp-save-button')

    // Registration Configuration - using data-testid
    this.allowRegistrationSwitch = page.getByTestId('reg-enabled-switch')
    this.requireEmailVerificationSwitch = page.getByTestId('reg-require-email-switch')

    // Save button in Registration form
    this.registrationSaveButton = page.getByTestId('reg-save-button')

    // Platform Self-Service Signup Configuration (admin realm only, DEC-001/009).
    // The tab trigger only renders when realmId === 'admin'. Switch testid is
    // derived from ConfigSwitchField `id="platform-signup"` → `${id}-switch`.
    this.platformSignupTab = page.getByTestId('platform-signup-tab')
    this.platformSignupEnabledSwitch = page.getByTestId('platform-signup-switch')
    this.platformSignupSaveButton = page.getByTestId('platform-signup-save-button')

    // OAuth Provider Configuration - using data-testid selectors
    this.addProviderButton = page.getByTestId('add-provider-button')
    this.providerTypeSelect = page.getByTestId('oauth-provider-type-select')
    this.clientIdInput = page.getByTestId('oauth-client-id-input')
    this.clientSecretInput = page.getByTestId('oauth-client-secret-input')
    this.scopesInput = page.getByLabel('Scopes')
    this.enabledCheckbox = page.getByTestId('oauth-enabled-checkbox')
    this.saveProviderButton = page.getByTestId('oauth-save-provider-button')
    this.cancelProviderButton = page.getByTestId('oauth-cancel-provider-button')

    // Email Configuration - using data-testid selectors
    this.emailTab = page.getByTestId('email-tab')
    this.emailStatusBadge = page.getByTestId('email-config-status-badge')
    this.emailStatusError = page.getByTestId('email-status-error')
    this.emailProviderResend = page.getByTestId('email-provider-resend')
    this.emailProviderSmtp = page.getByTestId('email-provider-smtp')
    this.emailFromAddressInput = page.getByTestId('email-from-address-input')
    this.emailResendApiKeyInput = page.getByTestId('email-resend-api-key-input')
    this.emailSmtpHostInput = page.getByTestId('email-smtp-host-input')
    this.emailSmtpPortInput = page.getByTestId('email-smtp-port-input')
    this.emailSmtpEncryptionSelect = page.getByTestId('email-smtp-encryption-select')
    this.emailSmtpUsernameInput = page.getByTestId('email-smtp-username-input')
    this.emailSmtpPasswordInput = page.getByTestId('email-smtp-password-input')
    this.emailTestRecipientInput = page.getByTestId('email-test-recipient-input')
    this.emailTestButton = page.getByTestId('email-test-button')
    this.emailTestError = page.getByTestId('email-test-error')
    this.emailTestSuccess = page.getByTestId('email-test-success')
    this.emailSaveButton = page.getByTestId('email-save-button')
    this.emailSaveError = page.getByTestId('email-save-error')

    // White-label Configuration - using data-testid selectors
    this.whiteLabelTab = page.getByTestId('white-label-tab')
    this.whiteLabelLogoUrlInput = page.getByTestId('white-label-logo-url')
    this.whiteLabelAccentColorPicker = page.getByTestId('white-label-accent-color-picker')
    this.whiteLabelAccentColorInput = page.getByTestId('white-label-accent-color')
    this.whiteLabelAccentWarning = page.getByTestId('white-label-accent-warning')
    this.whiteLabelBackgroundTypeSelect = page.getByTestId('white-label-background-type')
    this.whiteLabelBackgroundValueTextarea = page.getByTestId('white-label-background-value')
    this.whiteLabelFooterTextInput = page.getByTestId('white-label-footer-text')
    this.whiteLabelLoginTitleInput = page.getByTestId('white-label-login-title')
    this.whiteLabelLoginSubtitleInput = page.getByTestId('white-label-login-subtitle')
    this.whiteLabelRegisterTitleInput = page.getByTestId('white-label-register-title')
    this.whiteLabelRegisterSubtitleInput = page.getByTestId('white-label-register-subtitle')
    this.whiteLabelDraftNotice = page.getByTestId('white-label-draft-notice')
    this.whiteLabelSaveDraftButton = page.getByTestId('white-label-save-draft')
    this.whiteLabelPublishButton = page.getByTestId('white-label-publish')
    this.whiteLabelDiscardDraftButton = page.getByTestId('white-label-discard-draft')
    this.whiteLabelRestoreButton = page.getByTestId('white-label-restore')
    this.whiteLabelRestoreDialog = page.getByTestId('white-label-restore-dialog')
    this.whiteLabelRestoreConfirmButton = page.getByTestId('white-label-restore-confirm')
    this.whiteLabelPreviewLoginPanel = page.getByTestId('white-label-preview-login-panel')
    this.whiteLabelPreviewRegisterPanel = page.getByTestId('white-label-preview-register-panel')

    // Custom-domain Configuration - using data-testid selectors
    this.customDomainTab = page.getByTestId('custom-domain-tab')
    this.customDomainHostnameInput = page.getByTestId('custom-domain-hostname')
    this.customDomainCnameGuidance = page.getByTestId('custom-domain-cname-guidance')
    this.customDomainStatusCname = page.getByTestId('custom-domain-status-cname')
    this.customDomainStatusTls = page.getByTestId('custom-domain-status-tls')
    this.customDomainRefreshStatusButton = page.getByTestId('custom-domain-refresh-status')
    this.customDomainSaveButton = page.getByTestId('custom-domain-save')

    // Email-OTP Configuration - using data-testid selectors (mirrors the TOTP
    // block, which locates elements with page.getByTestId(...) directly rather
    // than via selectors.totp.*).
    //
    // The OTP controls were merged into the `email` tab by frontend commit
    // 364767b2, so there is no standalone `email-otp-tab` to click. We locate
    // the `email-otp-section` sub-block instead; navigation to it is handled by
    // switchToEmailTab() (see switchToEmailOtpTab).
    this.emailOtpSection = page.getByTestId('email-otp-section')
    this.emailOtpEnabledSwitch = page.getByTestId('email-otp-enabled-switch')
    this.emailOtpAutoRegisterSwitch = page.getByTestId('email-otp-auto-register-switch')
    this.emailOtpSaveButton = page.getByTestId('email-otp-save-button')

    // LDAP (corporate directory) Configuration — anchors live in the shared
    // SELECTORS.ldap registry (demo/e2e/selectors.ts).
    this.ldapTab = page.locator(SELECTORS.ldap.settingsTab)
    this.ldapEnabledSwitch = page.locator(SELECTORS.ldap.enabledSwitch)
    this.ldapUrlInput = page.locator(SELECTORS.ldap.urlInput)
    this.ldapStarttlsSwitch = page.locator(SELECTORS.ldap.starttlsSwitch)
    this.ldapBaseDnInput = page.locator(SELECTORS.ldap.baseDnInput)
    this.ldapBindDnInput = page.locator(SELECTORS.ldap.bindDnInput)
    this.ldapBindPasswordInput = page.locator(SELECTORS.ldap.bindPasswordInput)
    this.ldapUserFilterInput = page.locator(SELECTORS.ldap.userFilterInput)
    this.ldapMailAttributeInput = page.locator(SELECTORS.ldap.mailAttributeInput)
    this.ldapSaveButton = page.locator(SELECTORS.ldap.saveButton)
  }

  /**
   * Navigate to Settings page
   */
  async goto(): Promise<void> {
    // 通过点击侧边栏菜单来导航，模拟真实用户操作
    // 这样可以避免权限加载的时序问题
    const settingsMenuLink = this.page.locator(SELECTORS.sidebar.menuSettings)
    await this.smartClick(settingsMenuLink)

    // 等待页面加载完成
    await this.waitForReady()
  }

  /**
   * Wait for Settings page to be ready
   */
  async waitForReady(): Promise<void> {
    await expect(this.container).toBeVisible()
    await expect(this.heading).toBeVisible()
  }

  /**
   * Switch to Security/OTP Tab
   *
   * ✅ Fix: Increased timeout to 10 seconds to handle re-login scenarios.
   * Wait accounts for: navigation, API loading, React Query cache update, component re-rendering.
   */
  async switchToTOTPTab(): Promise<void> {
    await this.smartClick(this.totpTab)

    // Wait for tab content to be visible with longer timeout
    // Account for: navigation, API loading, React Query cache update, re-rendering
    await expect(this.totpEnabledSwitch).toBeVisible({ timeout: 10000 })

    // Additional wait to ensure React state is fully settled
    await this.page.waitForLoadState('networkidle')
  }

  /**
   * Switch to Registration Tab
   *
   * ✅ Fix: Increased timeout to 10 seconds to handle re-login scenarios.
   * Wait accounts for: navigation, API loading, React Query cache update, component re-rendering.
   */
  async switchToRegistrationTab(): Promise<void> {
    await this.smartClick(this.registrationTab)

    // Wait for tab content to be visible with longer timeout
    // Account for: navigation, API loading, React Query cache update, re-rendering
    await expect(this.allowRegistrationSwitch).toBeVisible({ timeout: 10000 })

    // Additional wait to ensure React state is fully settled
    await this.page.waitForLoadState('networkidle')
  }

  // ============================================================================
  // Email Configuration Methods
  // ============================================================================

  /**
   * Switch to Email Tab
   *
   * Follows the same pattern as switchToTOTPTab/switchToRegistrationTab.
   * Waits for the save button to indicate tab content is fully loaded.
   */
  async switchToEmailTab(): Promise<void> {
    await this.smartClick(this.emailTab)

    // Wait for tab content to be visible with longer timeout
    await expect(this.emailSaveButton).toBeVisible({ timeout: 10000 })

    // Additional wait to ensure React state is fully settled
    await this.page.waitForLoadState('networkidle')
  }

  /**
   * Configure Resend email provider
   *
   * Selects the Resend radio and fills the from address and API key fields.
   */
  async configureResend(config: EmailConfigFormData): Promise<void> {
    // Select Resend provider
    await this.smartClick(this.emailProviderResend)

    // Wait for Resend-specific fields to be visible
    await expect(this.emailResendApiKeyInput).toBeVisible({ timeout: 5000 })

    // Fill from address
    await this.fillField(this.emailFromAddressInput, config.fromAddress)

    // Fill API key if provided
    if (config.resendApiKey) {
      await this.fillField(this.emailResendApiKeyInput, config.resendApiKey)
    }
  }

  /**
   * Configure SMTP email provider
   *
   * Selects the SMTP radio and fills all SMTP fields (host, port, encryption, username, password).
   */
  async configureSmtp(config: EmailConfigFormData): Promise<void> {
    // Select SMTP provider
    await this.smartClick(this.emailProviderSmtp)

    // Wait for SMTP-specific fields to be visible
    await expect(this.emailSmtpHostInput).toBeVisible({ timeout: 5000 })

    // Fill from address
    await this.fillField(this.emailFromAddressInput, config.fromAddress)

    // Fill SMTP fields if provided
    if (config.smtpHost) {
      await this.fillField(this.emailSmtpHostInput, config.smtpHost)
    }

    if (config.smtpPort) {
      await this.fillField(this.emailSmtpPortInput, config.smtpPort)
    }

    if (config.smtpEncryption) {
      // Radix Select: click trigger, then select option from dropdown
      await this.smartClick(this.emailSmtpEncryptionSelect)
      const option = this.page.getByRole('option', { name: config.smtpEncryption === 'starttls' ? 'STARTTLS' : 'SSL', exact: true })
      await this.smartClick(option)
    }

    if (config.smtpUsername) {
      await this.fillField(this.emailSmtpUsernameInput, config.smtpUsername)
    }

    if (config.smtpPassword) {
      await this.fillField(this.emailSmtpPasswordInput, config.smtpPassword)
    }
  }

  /**
   * Save Email Configuration
   *
   * Clicks the save button and waits for the button text to return to 'Save',
   * matching the pattern used in saveTOTPConfig/saveRegistrationConfig.
   */
  async saveEmailConfig(): Promise<void> {
    await this.smartClick(this.emailSaveButton)

    // Wait for button text to return to "Save" (indicates save completed)
    await expect(async () => {
      const buttonText = await this.emailSaveButton.textContent()
      expect(buttonText).toBe('Save')
    }).toPass({ timeout: 15000 })
  }

  /**
   * Send a test email
   *
   * Fills the test recipient input and clicks the send test email button.
   */
  async sendTestEmail(recipient: string): Promise<void> {
    await this.fillField(this.emailTestRecipientInput, recipient)
    await this.smartClick(this.emailTestButton)
  }

  /**
   * Get the text content of the email status badge
   */
  async getEmailStatusBadgeText(): Promise<string> {
    return await this.getText(this.emailStatusBadge)
  }

  /**
   * Check if email is configured (based on status badge text)
   */
  async isEmailConfigured(): Promise<boolean> {
    const text = await this.getEmailStatusBadgeText()
    return text.toLowerCase().includes('configured') && !text.toLowerCase().includes('not configured')
  }

  // ============================================================================
  // TOTP Configuration Methods
  // ============================================================================

  /**
   * Enable TOTP
   */
  async enableTOTP(): Promise<void> {
    await this.setCheckbox(this.totpEnabledSwitch, true)
  }

  /**
   * Disable TOTP
   */
  async disableTOTP(): Promise<void> {
    await this.setCheckbox(this.totpEnabledSwitch, false)
  }

  /**
   * Enable Force TOTP
   */
  async enableForceTOTP(): Promise<void> {
    await this.setCheckbox(this.totpForceEnabledSwitch, true)
  }

  /**
   * Disable Force TOTP
   */
  async disableForceTOTP(): Promise<void> {
    await this.setCheckbox(this.totpForceEnabledSwitch, false)
  }

  /**
   * Save TOTP Configuration
   *
   * ✅ Verification: Relies on button state change as confirmation of save completion
   * ⚠️ Note: Removed waitForResponse because Playwright's network monitoring
   * was not capturing the POST response to /api/configs/admin/batch reliably.
   * Button state change (text returning to "Save") is a more reliable indicator.
   */
  async saveTOTPConfig(): Promise<void> {
    // Click save button
    await this.smartClick(this.totpSaveButton)

    // Wait for button text to return to "Save" (indicates save completed)
    await expect(async () => {
      const buttonText = await this.totpSaveButton.textContent()
      expect(buttonText).toBe('Save')
    }).toPass({ timeout: 15000 }) // Increased timeout for API processing
  }

  /**
   * Get current TOTP Configuration state
   */
  async getTOTPConfig(): Promise<TOTPConfigData> {
    const enabled = await this.totpEnabledSwitch.isChecked()
    const force_enabled = await this.totpForceEnabledSwitch.isChecked()

    return { enabled, force_enabled }
  }

  /**
   * Verify TOTP Configuration matches expected values
   */
  async verifyTOTPConfig(expected: TOTPConfigData): Promise<void> {
    const actual = await this.getTOTPConfig()

    expect(actual.enabled).toBe(expected.enabled)
    expect(actual.force_enabled).toBe(expected.force_enabled)
  }

  // ============================================================================
  // Registration Configuration Methods
  // ============================================================================

  /**
   * Enable user registration
   */
  async allowRegistration(): Promise<void> {
    await this.setSwitch(this.allowRegistrationSwitch, true)
  }

  /**
   * Disable user registration
   */
  async disallowRegistration(): Promise<void> {
    await this.setSwitch(this.allowRegistrationSwitch, false)
  }

  /**
   * Enable email verification requirement
   */
  async requireEmailVerification(): Promise<void> {
    await this.setSwitch(this.requireEmailVerificationSwitch, true)
  }

  /**
   * Disable email verification requirement
   */
  async disableEmailVerification(): Promise<void> {
    await this.setSwitch(this.requireEmailVerificationSwitch, false)
  }

  /**
   * Save Registration Configuration
   *
   * ✅ Verification: Handles page navigation that occurs after save
   * ⚠️ Note: Page reloads after clicking save, causing tab to reset to default (TOTP)
   * This method waits for navigation, re-selects Registration tab, then verifies button state.
   */
  async saveRegistrationConfig(): Promise<void> {
    // Click save button
    await this.smartClick(this.registrationSaveButton)

    // Wait for navigation to complete (page reloads after save)
    await this.page.waitForLoadState('networkidle')

    // Re-navigate to Registration tab (page may have reset to default TOTP tab)
    await this.switchToRegistrationTab()

    // Wait for button text to return to "Save" (indicates save completed)
    await expect(async () => {
      const buttonText = await this.registrationSaveButton.textContent()
      expect(buttonText).toBe('Save')
    }).toPass({ timeout: 15000 }) // Increased timeout for API processing
  }

  /**
   * Get current Registration Configuration state
   */
  async getRegistrationConfig(): Promise<RegistrationConfigData> {
    // Read the Radix Switch `data-state` (checked/unchecked) instead of
    // `isChecked()` to stay consistent with setSwitch() and avoid reading a
    // mid-toggle state.
    const enabledState = await this.allowRegistrationSwitch.getAttribute('data-state')
    const requireEmailState = await this.requireEmailVerificationSwitch.getAttribute('data-state')
    const enabled = enabledState === 'checked'
    const require_email_verification = requireEmailState === 'checked'

    return { enabled, require_email_verification }
  }

  /**
   * Verify Registration Configuration matches expected values
   */
  async verifyRegistrationConfig(expected: RegistrationConfigData): Promise<void> {
    const actual = await this.getRegistrationConfig()

    expect(actual.enabled).toBe(expected.enabled)
    expect(actual.require_email_verification).toBe(expected.require_email_verification)
  }

  // ============================================================================
  // OAuth Provider Configuration Methods
  // ============================================================================

  /**
   * Switch to Providers Tab
   *
   * ✅ Fix: Improved reliability with retry mechanism to handle Vite dev mode reloads.
   * - Uses retry wrapper to handle page reloads that may occur during tab switching
   * - Verifies tab is fully selected (aria-selected='true') before proceeding
   * - Waits for tab panel to be visible, not just the add button
   * - Adds network idle wait to ensure page is fully stable after tab switch
   * - Increased timeout to 15 seconds to accommodate retries
   */
  async switchToProvidersTab(): Promise<void> {
    // Retry mechanism to handle Vite dev mode reloads
    await expect(async () => {
      // Check current tab state
      const isSelected = await this.providersTab.getAttribute('aria-selected')

      // If not selected, click the tab
      if (isSelected !== 'true') {
        await this.smartClick(this.providersTab)
      }

      // Wait for tab to be selected (handle async state updates)
      await expect(async () => {
        const selected = await this.providersTab.getAttribute('aria-selected')
        expect(selected).toBe('true')
      }).toPass({ timeout: 5000 })

      // Wait for tab panel to be visible (indicates tab content is fully loaded)
      const tabPanel = this.page.getByRole('tabpanel', { name: 'Providers' })
      await expect(tabPanel).toBeVisible({ timeout: 5000 })

      // Wait for add button to be visible (secondary verification)
      await expect(this.addProviderButton).toBeVisible({ timeout: 5000 })
    }).toPass({ timeout: 15000 }) // Increased timeout for retries

    // The visible tabpanel + add-button checks above are a stronger readiness
    // signal than `waitForLoadState('networkidle')`. The former `networkidle`
    // wait flaked-timeout under persistent network activity (TanStack Query
    // refetch intervals, devtools, websocket heartbeats), aborting tests whose
    // tab switch had already succeeded.
  }

  /**
   * Add new OAuth Provider
   *
   * ✅ Fix: Wait for provider to appear in list instead of POST response.
   * - Forces switch to Providers tab with retry logic built into switchToProvidersTab()
   * - Adds explicit verification that we're on the right tab (tab panel visible)
   * - Prevents page reload in Vite dev mode that interrupts waitForResponse listeners
   * - Ensures addProviderButton is stable and clickable after tab switch
   * - Adds network idle wait before form operations
   * - Waits for dialog to close and provider to appear in list as success condition
   * - This approach is more reliable than waitForResponse for POST requests
   *   because it verifies the actual user-visible state (provider appears in list)
   */
  async addProvider(config: OAuthProviderConfigData): Promise<void> {
    // ✅ Fix: Force switch to Providers tab with retry logic
    // This ensures we're on the correct tab even if Vite dev mode caused a reload
    await this.switchToProvidersTab()

    // ✅ Fix: Add explicit verification that we're on the right tab
    // Verify the Providers tab panel is visible, not just the tab button
    const tabPanel = this.page.getByRole('tabpanel', { name: 'Providers' })
    await expect(tabPanel).toBeVisible({ timeout: 5000 })

    // ✅ Fix: Additional check to ensure addProviderButton is stable and clickable
    // Wait for button to be fully visible and enabled before clicking
    await expect(async () => {
      const isVisible = await this.addProviderButton.isVisible()
      const isEnabled = await this.addProviderButton.isEnabled()
      expect(isVisible).toBeTruthy()
      expect(isEnabled).toBeTruthy()
    }).toPass({ timeout: 5000 })

    // Click "Add Provider" button
    await this.smartClick(this.addProviderButton)

    // Wait for dialog to open (indicated by provider type select being visible)
    await expect(this.providerTypeSelect).toBeVisible({ timeout: 5000 })

    // ✅ Fix: Wait for network idle to ensure dialog is fully rendered
    await this.page.waitForLoadState('networkidle')

    // Fill in form fields
    await this.smartClick(this.providerTypeSelect)
    // Use exact matching to avoid matching multiple providers (e.g., 'wechat' matching both 'WeChat' and 'WeChat Mini Program')
    const displayName = this.getProviderDisplayName(config.providerType)
    await this.page.getByRole('option', { name: displayName, exact: true }).click()
    await this.fillField(this.clientIdInput, config.clientId)
    await this.fillField(this.clientSecretInput, config.clientSecret)

    // Set enabled state
    if (config.enabled !== undefined) {
      const isEnabled = await this.enabledCheckbox.isChecked()
      if (isEnabled !== config.enabled) {
        await this.smartClick(this.enabledCheckbox)
      }
    }

    // ✅ Fix: Wait for save button to be enabled (form validation complete)
    // TanStack Form's canSubmit state must be true before the click will submit
    await expect(async () => {
      const isDisabled = await this.saveProviderButton.isDisabled()
      expect(isDisabled).toBeFalsy()
    }).toPass({ timeout: 5000 })

    // ✅ Fix: Save provider configuration - DON'T wait for POST response
    // The real success condition is that the provider appears in the list
    // Not that we captured the POST response (which may be missed due to page navigation)
    await this.smartClick(this.saveProviderButton)

    // Wait for dialog to close (provider type select should not be visible)
    await expect(this.providerTypeSelect).not.toBeVisible({ timeout: 5000 })

    // Wait for network to be idle (POST request has been sent and processed)
    await this.page.waitForLoadState('networkidle')

    // Force refresh the providers tab to ensure React Query updates
    await this.switchToProvidersTab()

    // ✅ Fix: Verify provider exists in list (this is the real success condition)
    // This confirms the POST request succeeded and data was persisted
    await this.waitForProviderInList(config.providerType, { timeout: 15000 })
  }

  /**
   * Toggle Provider enabled/disabled state
   */
  async toggleProvider(providerType: string): Promise<void> {
    const providerRow = this.getProviderRow(providerType)
    const toggleButton = providerRow.getByTestId(`provider-toggle-button-${providerType}`)

    // Wait for button to be visible
    await expect(toggleButton).toBeVisible({ timeout: 10000 })

    // Click the toggle button and wait for API to complete
    await this.smartClick(toggleButton)

    // Wait for the API request to complete (simple timeout instead of waitForResponse)
    await this.page.waitForTimeout(2000)
  }

  /**
   * Edit Provider configuration
   *
   * ✅ Fix: Wait for provider to remain in list instead of PUT response.
   * - Added comprehensive page state checks and error handling to prevent race conditions
   * - Waits for provider to remain visible with updated configuration as success condition
   * - This approach is more reliable than waitForResponse for PUT requests
   *   because it verifies the actual user-visible state (provider remains in list)
   */
  async editProvider(providerType: string, config: Partial<OAuthProviderConfigData>): Promise<void> {
    const providerRow = this.getProviderRow(providerType)

    // Click edit button
    const editButton = providerRow.getByTestId(`provider-edit-button-${providerType}`)
    await this.smartClick(editButton)

    // Wait for dialog to open
    await expect(this.providerTypeSelect).toBeVisible({ timeout: 5000 })

    // ✅ Fix: Wait for network idle to ensure dialog is fully rendered
    await this.page.waitForLoadState('networkidle')

    // Update fields if provided
    if (config.clientId !== undefined) {
      // Clear the input first
      await this.clientIdInput.clear()
      // Use type() instead of fill() to ensure React onChange is triggered
      await this.clientIdInput.type(config.clientId)
      // Wait for React state to update
      await this.page.waitForTimeout(500)
    }

    if (config.clientSecret !== undefined && config.clientSecret !== '') {
      await this.fillField(this.clientSecretInput, config.clientSecret)
    }

    if (config.scopes !== undefined && config.scopes.length > 0) {
      await this.fillField(this.scopesInput, config.scopes.join(', '))
    }

    if (config.enabled !== undefined) {
      const isEnabled = await this.enabledCheckbox.isChecked()
      if (isEnabled !== config.enabled) {
        await this.smartClick(this.enabledCheckbox)
      }
    }

    // ✅ Fix: Wait for save button to be enabled (form validation complete)
    // TanStack Form's canSubmit state must be true before the click will submit
    await expect(async () => {
      const isDisabled = await this.saveProviderButton.isDisabled()
      expect(isDisabled).toBeFalsy()
    }).toPass({ timeout: 5000 })

    // ✅ Fix: Save provider configuration - DON'T wait for PUT response
    // The real success condition is that the provider remains in the list
    await this.smartClick(this.saveProviderButton)

    // Wait for dialog to close (provider type select should not be visible)
    await expect(this.providerTypeSelect).not.toBeVisible({ timeout: 3000 })

    // Wait for network to be idle (PUT request has been sent and processed)
    await this.page.waitForLoadState('networkidle')

    // Force refresh the providers tab to ensure React Query updates
    // ✅ Fix: Only switch if not already active to prevent unnecessary page navigation
    const isActive = await this.providersTab.getAttribute('aria-selected')
    if (isActive !== 'true') {
      await this.switchToProvidersTab()
    }

    // ✅ Fix: Verify provider still exists in list (edit was successful)
    await this.waitForProviderInList(providerType, { timeout: 15000 })
  }

  /**
   * Delete Provider
   *
   * ✅ Fix: Directly wait for UI update to verify final user-visible state.
   * Matches the pattern used in addProvider() and editProvider().
   * Backend deletion may take longer, so we use 15000ms timeout.
   */
  async deleteProvider(providerType: string): Promise<void> {
    const providerRow = this.getProviderRow(providerType)

    // Click delete button
    const deleteButton = providerRow.getByTestId(`provider-delete-button-${providerType}`)
    await this.smartClick(deleteButton)

    // Confirm deletion in dialog
    const confirmDeleteButton = this.page.getByTestId('provider-delete-confirm-button')
    await this.smartClick(confirmDeleteButton)

    // Wait for provider to be removed from list (UI verification)
    await this.waitForProviderNotInList(providerType, { timeout: 15000 })
  }

  /**
   * Check if Provider exists in list
   *
   * ✅ Fix: Use waitFor() instead of isVisible() to ensure reliability.
   * isVisible() returns false when element is not in viewport, causing false negatives.
   * waitFor() waits for the element to become visible anywhere in the DOM.
   */
  async providerExists(providerType: string): Promise<boolean> {
    try {
      // Wait for the provider row to be visible in the DOM (not just viewport)
      await this.page.getByTestId(`provider-row-${providerType}`).waitFor({ timeout: 5000 })
      return true
    } catch {
      return false
    }
  }

  /**
   * Get Provider status (enabled/disabled)
   */
  async getProviderStatus(providerType: string): Promise<boolean> {
    const providerRow = this.getProviderRow(providerType)
    const statusBadge = providerRow.getByTestId(`provider-status-${providerType}`)
    const text = await statusBadge.textContent()
    return text === 'Enabled'
  }

  /**
   * Get Client ID for a specific provider
   */
  async getClientId(providerType: string): Promise<string | null> {
    const providerRow = this.getProviderRow(providerType)
    const clientIdElement = providerRow.getByTestId(`provider-client-id-${providerType}`)
    // Wait for element to be visible before reading text
    await expect(clientIdElement).toBeVisible({ timeout: 10000 })
    const text = await clientIdElement.textContent()
    // Extract the actual client ID from "clientId: xxx" format
    const match = text?.match(/clientId:\s*(.+)/)
    return match ? match[1] : null
  }

  /**
   * Get all Client IDs from provider list
   */
  async getClientIds(): Promise<string[]> {
    const elements = await this.page.locator('[data-testid^="provider-client-id-"]').all()
    const ids = await Promise.all(elements.map(async el => {
      const text = await el.textContent()
      const match = text?.match(/clientId:\s*(.+)/)
      return match ? match[1] : null
    }))
    return ids.filter((id): id is string => id !== null && id !== undefined)
  }

  // ============================================================================
  // White-label Configuration Methods (US-WL-001/002/003/004)
  //
  // Mirrors switchToTOTPTab/saveTOTPConfig/getTOTPConfig patterns. Drives the
  // white-label-config-form (frontend/src/components/realm-config/white-label-
  // config-form.tsx). Background is a {type,value} object edited through a type
  // select + conditional value textarea (only renders when type !== 'none').
  // @see docs/user-stories/core/white-label.md, docs/prd/core/ui-custom.md
  // ============================================================================

  /**
   * Switch to White-label Tab.
   *
   * Follows switchToTOTPTab/switchToEmailTab: clicks the tab, waits for the
   * save-draft button to confirm tab content is loaded, then networkidle.
   */
  async switchToWhiteLabelTab(): Promise<void> {
    await this.smartClick(this.whiteLabelTab)

    // Wait for tab content to be visible with longer timeout (re-login timing)
    await expect(this.whiteLabelSaveDraftButton).toBeVisible({ timeout: 10000 })

    // Additional wait to ensure React state is fully settled
    await this.page.waitForLoadState('networkidle')
  }

  /**
   * Fill white-label form fields.
   *
   * Only the provided fields are touched. Background requires driving the
   * type Select first, then the conditional value Textarea (which only
   * renders when type !== 'none'). Pass `background: null` to clear the
   * background (selects `none`).
   */
  async fillWhiteLabelForm(values: Partial<WhiteLabelFormValues>): Promise<void> {
    // `null` clears a field (matches the form schema, where null === "use
    // default / unset"). Coerce null to '' so Playwright .fill() receives a
    // string; `undefined` (field omitted) leaves the field untouched.
    if (values.logoUrl !== undefined) {
      await this.fillField(this.whiteLabelLogoUrlInput, values.logoUrl ?? '')
    }

    if (values.accentColor !== undefined) {
      // Drive the hex text input (the native color picker mirrors it); using
      // the text input keeps the test resilient to native picker popovers.
      await this.fillField(this.whiteLabelAccentColorInput, values.accentColor ?? '')
    }

    if (values.background !== undefined) {
      await this.selectBackgroundValue(values.background)
    }

    if (values.footerText !== undefined) {
      await this.fillField(this.whiteLabelFooterTextInput, values.footerText ?? '')
    }

    if (values.loginTitle !== undefined) {
      await this.fillField(this.whiteLabelLoginTitleInput, values.loginTitle ?? '')
    }

    if (values.loginSubtitle !== undefined) {
      await this.fillField(this.whiteLabelLoginSubtitleInput, values.loginSubtitle ?? '')
    }

    if (values.registerTitle !== undefined) {
      await this.fillField(this.whiteLabelRegisterTitleInput, values.registerTitle ?? '')
    }

    if (values.registerSubtitle !== undefined) {
      await this.fillField(this.whiteLabelRegisterSubtitleInput, values.registerSubtitle ?? '')
    }
  }

  /**
   * Save Draft (writes the unpublished draft).
   *
   * Clicks the submit button and waits for the button text to return to the
   * idle label, mirroring saveTOTPConfig's toPass pattern. The idle label is
   * localized; we assert the button is NOT showing the in-flight "Saving..."
   * label by waiting for it to become re-enabled with the idle text.
   */
  async saveDraft(): Promise<void> {
    await this.smartClick(this.whiteLabelSaveDraftButton)

    // Wait for the save-draft button to settle back to idle (enabled, not
    // showing the in-flight label). Mirrors saveTOTPConfig's toPass pattern.
    await expect(async () => {
      const disabled = await this.whiteLabelSaveDraftButton.isDisabled()
      expect(disabled).toBeFalsy()
    }).toPass({ timeout: 15000 })
    await this.page.waitForLoadState('networkidle')
  }

  /**
   * Publish (writes the published settings).
   *
   * Clicks the publish button and waits for it to return to idle.
   */
  async publish(): Promise<void> {
    await this.smartClick(this.whiteLabelPublishButton)

    await expect(async () => {
      const disabled = await this.whiteLabelPublishButton.isDisabled()
      expect(disabled).toBeFalsy()
    }).toPass({ timeout: 15000 })
    await this.page.waitForLoadState('networkidle')
  }

  /**
   * Discard the saved draft (resets editor to published config).
   *
   * Clicks the discard button and waits for it to return to idle. The button
   * is disabled when no draft exists, so callers must ensure a draft is present.
   */
  async discardDraft(): Promise<void> {
    await this.smartClick(this.whiteLabelDiscardDraftButton)

    await expect(async () => {
      const disabled = await this.whiteLabelDiscardDraftButton.isDisabled()
      expect(disabled).toBeFalsy()
    }).toPass({ timeout: 15000 })
    await this.page.waitForLoadState('networkidle')
  }

  /**
   * Restore the previous published config.
   *
   * Drives the restore AlertDialog: open via the restore button, then confirm
   * via white-label-restore-confirm. Waits for the dialog to close and the
   * restore button to return to idle. Requires a previous version to exist
   * (the restore button is disabled otherwise).
   */
  async restore(): Promise<void> {
    // Open the restore confirmation dialog
    await this.smartClick(this.whiteLabelRestoreButton)
    await expect(this.whiteLabelRestoreDialog).toBeVisible({ timeout: 5000 })

    // Confirm the restore action
    await this.smartClick(this.whiteLabelRestoreConfirmButton)

    // Wait for the dialog to close (restore action settled)
    await expect(this.whiteLabelRestoreDialog).toBeHidden({ timeout: 15000 })
    await expect(async () => {
      const disabled = await this.whiteLabelRestoreButton.isDisabled()
      expect(disabled).toBeFalsy()
    }).toPass({ timeout: 15000 })
    await this.page.waitForLoadState('networkidle')
  }

  /**
   * Whether the draft notice is currently visible (draft exists or form is dirty).
   */
  async hasDraftNotice(): Promise<boolean> {
    return await this.whiteLabelDraftNotice.isVisible().catch(() => false)
  }

  /**
   * Whether the WCAG AA accent contrast warning is currently visible.
   */
  async hasAccentWarning(): Promise<boolean> {
    return await this.whiteLabelAccentWarning.isVisible().catch(() => false)
  }

  /**
   * Read the current white-label form values.
   *
   * Returns the values as currently rendered in the form inputs. Background is
   * reconstructed from the type Select + value Textarea: a `none` type yields
   * `null`; otherwise `{type, value}`.
   */
  async getFormValues(): Promise<WhiteLabelFormValues> {
    const logoUrl = (await this.whiteLabelLogoUrlInput.inputValue().catch(() => '')) || null
    const accentColor = (await this.whiteLabelAccentColorInput.inputValue().catch(() => '')) || null
    const footerText = (await this.whiteLabelFooterTextInput.inputValue().catch(() => '')) || null
    const loginTitle = (await this.whiteLabelLoginTitleInput.inputValue().catch(() => '')) || null
    const loginSubtitle = (await this.whiteLabelLoginSubtitleInput.inputValue().catch(() => '')) || null
    const registerTitle = (await this.whiteLabelRegisterTitleInput.inputValue().catch(() => '')) || null
    const registerSubtitle = (await this.whiteLabelRegisterSubtitleInput.inputValue().catch(() => '')) || null

    const background = await this.readBackgroundValue()

    return {
      logoUrl,
      accentColor,
      background,
      footerText,
      loginTitle,
      loginSubtitle,
      registerTitle,
      registerSubtitle,
    }
  }

  /**
   * Publish an empty baseline config for the current realm.
   *
   * Restore-balanced teardown helper. White-label has no DB-side demo helper,
   * so the teardown is driven in-page: switch to the white-label tab, set the
   * background type to `none`, clear every text field, then publish. This
   * ensures no published brand leaks across test runs and no dangling draft
   * remains. Mirrors resetRealmTOTP() intent. Callers must already be logged
   * in as the realm's admin and navigated to the Settings page.
   */
  async resetWhiteLabelConfig(): Promise<void> {
    try {
      await this.switchToWhiteLabelTab()

      // Clear every field + collapse background to none.
      await this.fillWhiteLabelForm({
        logoUrl: '',
        accentColor: '',
        background: null,
        footerText: '',
        loginTitle: '',
        loginSubtitle: '',
        registerTitle: '',
        registerSubtitle: '',
      })

      // Publish the cleared baseline. Publish is the committed state; saving a
      // draft would leave a dangling draft notice for the next run.
      await this.publish()
    } catch (error) {
      // Teardown must never hard-fail the test run; log and continue.
      console.warn(`[SettingsPage] resetWhiteLabelConfig failed for realm "${this.realmId}":`, error)
    }
  }

  // ===========================================================================
  // Custom-domain Configuration (US-CD-001 — single-state save model)
  //
  // Custom-domain is a single `hostname` field with one Save button (PUT
  // /config/custom-domain writes the hostname + host→realm mapping in one step).
  // There is no draft/publish/restore lifecycle. The CNAME guidance panel
  // renders the configured `cname_target` (from [custom_domain].cname_target in
  // demo.toml); getCnameGuidanceText() reads it back so tests can assert it is
  // non-empty.
  // ===========================================================================

  /**
   * Switch to Custom-domain Tab.
   *
   * Clicks the tab, waits for the save button to confirm the tab content is
   * loaded, then networkidle.
   */
  async switchToCustomDomainTab(): Promise<void> {
    await this.smartClick(this.customDomainTab)

    // Wait for tab content to be visible with longer timeout (re-login timing)
    await expect(this.customDomainSaveButton).toBeVisible({ timeout: 10000 })

    // Additional wait to ensure React state is fully settled
    await this.page.waitForLoadState('networkidle')
  }

  /**
   * Fill the custom-domain hostname form field.
   *
   * Hostname is the only field. Coerce null/undefined to '' so Playwright
   * .fill() receives a string (empty clears the hostname).
   */
  async fillCustomDomainForm(values: Partial<CustomDomainFormValues>): Promise<void> {
    if (values.hostname !== undefined) {
      await this.fillField(this.customDomainHostnameInput, values.hostname ?? '')
    }
  }

  /**
   * Save the custom-domain config (writes the hostname + host→realm mapping).
   *
   * Clicks the save button and waits for the PUT response. Critically waits for
   * the actual response (not just button-idle): tests read `published.hostname`
   * via a direct API GET right after this returns, and button-idle resolves as
   * soon as React Query receives the response headers, which can race the
   * backend's commit sequence. Waiting for the PUT response guarantees the
   * server-side write is acknowledged before the caller reads persisted state.
   */
  async saveCustomDomain(): Promise<void> {
    const saveResponse = this.page
      .waitForResponse(
        (r) =>
          /\/api\/realms\/[^/]+\/config\/custom-domain$/.test(r.url()) &&
          r.request().method() === 'PUT',
        { timeout: 15000 },
      )
      .then((r) => r)
    await this.smartClick(this.customDomainSaveButton)
    await expect((await saveResponse).ok(), 'save PUT must succeed').toBeTruthy()

    await expect(async () => {
      const disabled = await this.customDomainSaveButton.isDisabled()
      expect(disabled).toBeFalsy()
    }).toPass({ timeout: 15000 })
    await this.page.waitForLoadState('networkidle')
  }

  /**
   * Read the text content of the CNAME guidance panel.
   *
   * The panel renders the configured `cnameTarget` (from
   * [custom_domain].cname_target). Tests assert the configured target appears
   * so the panel is never empty.
   */
  async getCnameGuidanceText(): Promise<string> {
    await expect(this.customDomainCnameGuidance).toBeVisible({ timeout: 10000 })
    return (await this.customDomainCnameGuidance.textContent()) || ''
  }

  /**
   * Read the current custom-domain form values (the hostname input value).
   */
  async getCustomDomainFormValues(): Promise<CustomDomainFormValues> {
    const hostname = (await this.customDomainHostnameInput.inputValue().catch(() => '')) || ''
    return { hostname }
  }

  /**
   * Tear down the custom-domain config for the current realm.
   *
   * Clears the hostname by saving an empty value (PUT with `{ hostname: null }`
   * removes all mapping rows for the realm). Best-effort: catches + warns
   * internally so teardown never hard-fails the test run. Callers must already
   * be logged in as the realm's admin and navigated to the Settings page.
   */
  async resetCustomDomainConfig(): Promise<void> {
    try {
      await this.switchToCustomDomainTab()
      await this.fillCustomDomainForm({ hostname: null })
      await this.saveCustomDomain()
    } catch (error) {
      // Teardown must never hard-fail the test run; log and continue.
      console.warn(`[SettingsPage] resetCustomDomainConfig failed for realm "${this.realmId}":`, error)
    }
  }

  // ============================================================================
  // Email-OTP Configuration Methods
  //
  // Mirrors the TOTP block (switchToTOTPTab / enableTOTP / disableTOTP /
  // saveTOTPConfig / getTOTPConfig). Drives the Email-OTP section that was
  // merged into the `email` tab's EmailConfigForm by frontend commit 364767b2
  // (frontend/src/components/realm-config/email-config-form.tsx). The two
  // switches carry `data-testid` via the shared config-switch-field.tsx.
  // @see docs/user-stories/auth/email-otp-login.md
  // ============================================================================

  /**
   * Navigate to the Email-OTP configuration controls.
   *
   * After frontend commit 364767b2 ("merged email-otp settings"), Email-OTP is
   * no longer a standalone tab. The two switches and the save button render
   * inside the `email` tab as a `data-testid="email-otp-section"` sub-block of
   * EmailConfigForm (only when `onSaveEmailOtp` is provided). So we navigate to
   * the `email` tab via switchToEmailTab(), then assert the OTP section is
   * visible before any switch/save interaction.
   *
   * The method name is retained for call-site compatibility
   * (helpers/email-otp-setup.ts and the OTP config demo both call it).
   */
  async switchToEmailOtpTab(): Promise<void> {
    // switchToEmailTab() clicks the `email-tab` and waits for the email save
    // button to confirm the tab content has loaded.
    await this.switchToEmailTab()

    // Assert the Email-OTP sub-block is rendered before interacting with its
    // switches. The section only appears when `onSaveEmailOtp` is wired up,
    // which is the case on the realm settings page.
    await expect(this.emailOtpSection).toBeVisible({ timeout: 10000 })
  }

  /**
   * Enable Email-OTP login for the realm.
   *
   * Uses setSwitch() (Radix Switch-aware) instead of setCheckbox(): the OTP
   * switches are Radix `<button role="switch">` toggles, and a force-click on
   * the switch root did not reliably flip data-state (Phase 3 true→false
   * stalled with the switch still checked). setSwitch reads `data-state` and
   * waits for the transition.
   */
  async enableEmailOtp(): Promise<void> {
    await this.setSwitch(this.emailOtpEnabledSwitch, true)
  }

  /**
   * Disable Email-OTP login for the realm.
   */
  async disableEmailOtp(): Promise<void> {
    await this.setSwitch(this.emailOtpEnabledSwitch, false)
  }

  /**
   * Enable auto-registration of unverified emails on successful OTP verify.
   */
  async enableAutoRegister(): Promise<void> {
    await this.setSwitch(this.emailOtpAutoRegisterSwitch, true)
  }

  /**
   * Disable auto-registration of unverified emails.
   */
  async disableAutoRegister(): Promise<void> {
    await this.setSwitch(this.emailOtpAutoRegisterSwitch, false)
  }

  /**
   * Save Email-OTP Configuration.
   *
   * Mirrors saveTOTPConfig: clicks the save button and waits for the button
   * text to return to 'Save' (indicates the PUT settled).
   */
  async saveEmailOtpConfig(): Promise<void> {
    await this.smartClick(this.emailOtpSaveButton)

    // Wait for button text to return to "Save" (indicates save completed)
    await expect(async () => {
      const buttonText = await this.emailOtpSaveButton.textContent()
      expect(buttonText).toBe('Save')
    }).toPass({ timeout: 15000 }) // Increased timeout for API processing
  }

  /**
   * Get current Email-OTP Configuration state.
   *
   * Reads the Radix Switch `data-state` attribute (checked/unchecked) instead
   * of `isChecked()` to stay consistent with setSwitch() and avoid reading the
   * switch mid-transition.
   */
  async getEmailOtpConfig(): Promise<EmailOtpConfigData> {
    const enabledState = await this.emailOtpEnabledSwitch.getAttribute('data-state')
    const autoRegisterState = await this.emailOtpAutoRegisterSwitch.getAttribute('data-state')
    const enabled = enabledState === 'checked'
    const auto_register = autoRegisterState === 'checked'

    return { enabled, auto_register }
  }

  /**
   * Best-effort teardown of the Email-OTP config for the current realm.
   *
   * Disables both switches and saves, mirroring resetWhiteLabelConfig's
   * try/catch so teardown never hard-fails the test run. Callers must already
   * be logged in as the realm's admin and navigated to the Settings page.
   */
  async resetEmailOtpConfig(): Promise<void> {
    try {
      await this.switchToEmailOtpTab()
      await this.disableEmailOtp()
      await this.disableAutoRegister()
      await this.saveEmailOtpConfig()
    } catch (error) {
      // Teardown must never hard-fail the test run; log and continue.
      console.warn(`[SettingsPage] resetEmailOtpConfig failed for realm "${this.realmId}":`, error)
    }
  }

  // ============================================================================
  // LDAP (Corporate Directory) Configuration Methods
  //
  // Drives the standalone `ldap` tab: Radix Switch toggles via setSwitch, text
  // fields via fillField (blur included so TanStack Form validation runs),
  // save + wait for the button text to settle back to 'Save'.
  // ============================================================================

  /**
   * Switch to the Corporate directory (LDAP) tab.
   *
   * Asserts the URL input is visible to confirm the tab content loaded — the
   * form card root has no testid, so the first field doubles as the anchor.
   */
  async switchToLdapTab(): Promise<void> {
    await this.smartClick(this.ldapTab)
    await expect(this.ldapUrlInput).toBeVisible({ timeout: 10000 })
  }

  /**
   * Fill the LDAP config form. Only the provided fields are touched; the
   * enable switch is driven separately via setLdapEnabled (its own gate
   * semantics — enable with a bindDn requires a stored password — deserve an
   * explicit step in the demos).
   */
  async fillLdapConfig(values: {
    url?: string
    baseDn?: string
    bindDn?: string
    bindPassword?: string
    userFilter?: string
    mailAttribute?: string
  }): Promise<void> {
    if (values.url !== undefined) {
      await this.fillField(this.ldapUrlInput, values.url)
    }
    if (values.baseDn !== undefined) {
      await this.fillField(this.ldapBaseDnInput, values.baseDn)
    }
    if (values.bindDn !== undefined) {
      await this.fillField(this.ldapBindDnInput, values.bindDn)
    }
    if (values.bindPassword !== undefined) {
      await this.fillField(this.ldapBindPasswordInput, values.bindPassword)
    }
    if (values.userFilter !== undefined) {
      await this.fillField(this.ldapUserFilterInput, values.userFilter)
    }
    if (values.mailAttribute !== undefined) {
      await this.fillField(this.ldapMailAttributeInput, values.mailAttribute)
    }
  }

  /**
   * Read back the current LDAP form values (persistence assertions after a
   * reload). The password field reads as '' — the stored value is masked
   * server-side and never echoed back.
   */
  async getLdapFormValues(): Promise<{
    url: string
    baseDn: string
    bindDn: string
    bindPassword: string
    userFilter: string
    mailAttribute: string
    enabled: boolean
  }> {
    return {
      url: await this.ldapUrlInput.inputValue(),
      baseDn: await this.ldapBaseDnInput.inputValue(),
      bindDn: await this.ldapBindDnInput.inputValue(),
      bindPassword: await this.ldapBindPasswordInput.inputValue(),
      userFilter: await this.ldapUserFilterInput.inputValue(),
      mailAttribute: await this.ldapMailAttributeInput.inputValue(),
      enabled: await this.isLdapEnabled(),
    }
  }

  /**
   * Whether the LDAP enable switch is on. Reads the Radix Switch `data-state`
   * (consistent with getEmailOtpConfig / isPlatformSignupEnabled), NOT
   * isChecked(), to avoid reading mid-transition.
   */
  async isLdapEnabled(): Promise<boolean> {
    const state = await this.ldapEnabledSwitch.getAttribute('data-state')
    return state === 'checked'
  }

  /**
   * Whether the StartTLS switch is locked off — the form disables it while the
   * URL is ldaps:// (TLS comes from the scheme; StartTLS would be redundant
   * and is rejected by the backend).
   */
  async isLdapStarttlsLocked(): Promise<boolean> {
    return this.ldapStarttlsSwitch.isDisabled()
  }

  /**
   * Enable/disable corporate account login for the realm.
   */
  async setLdapEnabled(enabled: boolean): Promise<void> {
    await this.setSwitch(this.ldapEnabledSwitch, enabled)
  }

  /**
   * Save LDAP Configuration.
   *
   * Mirrors savePlatformSignupConfig: clicks save and waits for the button
   * text to return to 'Save' (indicates the batch upsert settled).
   */
  async saveLdapConfig(): Promise<void> {
    await this.smartClick(this.ldapSaveButton)

    await expect(async () => {
      const buttonText = await this.ldapSaveButton.textContent()
      expect(buttonText).toBe('Save')
    }).toPass({ timeout: 15000 })
  }

  // ============================================================================
  // Platform Self-Service Signup Configuration Methods (admin realm only)
  //
  // Mirrors the Email-OTP config methods: Radix Switch toggles via setSwitch,
  // save + wait for button text to settle. The platform-signup tab is rendered
  // only when realmId === 'admin' (frontend settings.tsx), so the caller must
  // be logged into the admin realm.
  // ============================================================================

  /**
   * Switch to the Platform Self-Service Signup tab (admin realm only).
   *
   * Asserts the enable switch is visible to confirm the tab content loaded,
   * mirroring switchToTOTPTab/switchToRegistrationTab.
   */
  async switchToPlatformSignupTab(): Promise<void> {
    await this.smartClick(this.platformSignupTab)
    await expect(this.platformSignupEnabledSwitch).toBeVisible({ timeout: 10000 })
  }

  /**
   * Enable the platform self-service signup entry.
   */
  async enablePlatformSignup(): Promise<void> {
    await this.setSwitch(this.platformSignupEnabledSwitch, true)
  }

  /**
   * Disable the platform self-service signup entry (fail-closed default).
   */
  async disablePlatformSignup(): Promise<void> {
    await this.setSwitch(this.platformSignupEnabledSwitch, false)
  }

  /**
   * Save Platform Self-Service Signup Configuration.
   *
   * Mirrors saveTOTPConfig/saveEmailOtpConfig: clicks save and waits for the
   * button text to return to 'Save' (indicates the PUT settled).
   */
  async savePlatformSignupConfig(): Promise<void> {
    await this.smartClick(this.platformSignupSaveButton)

    await expect(async () => {
      const buttonText = await this.platformSignupSaveButton.textContent()
      expect(buttonText).toBe('Save')
    }).toPass({ timeout: 15000 })
  }

  /**
   * Get the current platform self-service signup enabled state.
   *
   * Reads the Radix Switch `data-state` attribute (consistent with
   * getEmailOtpConfig / setSwitch), NOT isChecked(), to avoid reading the
   * switch mid-transition.
   */
  async isPlatformSignupEnabled(): Promise<boolean> {
    const state = await this.platformSignupEnabledSwitch.getAttribute('data-state')
    return state === 'checked'
  }

  /**
   * Best-effort teardown: disable platform self-service signup.
   *
   * The startup seed leaves the toggle at its fail-closed default (false), so
   * resetting to disabled keeps the demo env in its default state and avoids
   * leaking an open public signup entry into other demos. Mirrors
   * resetEmailOtpConfig's try/catch so teardown never hard-fails the run.
   */
  async resetPlatformSignupConfig(): Promise<void> {
    try {
      await this.switchToPlatformSignupTab()
      await this.disablePlatformSignup()
      await this.savePlatformSignupConfig()
    } catch (error) {
      console.warn(
        `[SettingsPage] resetPlatformSignupConfig failed for realm "${this.realmId}":`,
        error
      )
    }
  }

  // --- White-label private helpers -------------------------------------------

  /**
   * Drive the background type Select + conditional value Textarea.
   *
   * `none` (null) collapses the value input; `image`/`gradient` reveal it.
   * Selects the type via the Radix Select trigger, then fills the Textarea.
   */
  private async selectBackgroundValue(background: WhiteLabelBackgroundForm | null): Promise<void> {
    const targetOption = background ? background.type : 'none'

    // Drive the Radix Select via the shared helper (handles listbox open/close).
    await this.selectRadixOption(this.whiteLabelBackgroundTypeSelect, targetOption)

    if (!background) {
      // type=none: the value textarea is not rendered; nothing more to do.
      return
    }

    // The value textarea only renders when type !== none; wait for it.
    await expect(this.whiteLabelBackgroundValueTextarea).toBeVisible({ timeout: 5000 })
    await this.fillField(this.whiteLabelBackgroundValueTextarea, background.value)
  }

  /**
   * Read the background value from the current form state.
   *
   * Returns null when the type select is at `none` (or the value textarea is
   * absent); otherwise reconstructs {type, value} from the rendered inputs.
   */
  private async readBackgroundValue(): Promise<WhiteLabelBackgroundForm | null> {
    // The value textarea presence encodes whether a type !== none is selected.
    const valueVisible = await this.whiteLabelBackgroundValueTextarea.isVisible().catch(() => false)
    if (!valueVisible) {
      return null
    }

    // Read the value; the type is whatever non-none option is active. We infer
    // the type from the select's trigger text since Radix does not expose a
    // value attribute on the trigger that we can reliably read cross-locale.
    const triggerText = (await this.whiteLabelBackgroundTypeSelect.textContent())?.trim().toLowerCase() || ''
    const type: WhiteLabelBackgroundForm['type'] = triggerText.includes('image') ? 'image' : 'gradient'
    const value = (await this.whiteLabelBackgroundValueTextarea.inputValue().catch(() => '')) || ''

    return { type, value }
  }

  // ============================================================================
  // Private Helper Methods
  // ============================================================================

  /**
   * Get display name for provider type
   * Maps internal provider types to UI display names
   */
  private getProviderDisplayName(providerType: string): string {
    const displayNames: Record<string, string> = {
      google: 'Google',
      github: 'GitHub',
      facebook: 'Facebook',
      apple: 'Apple',
      wechat: 'WeChat',
      wechat_miniprogram: 'WeChat Mini Program',
    }
    return displayNames[providerType] || providerType
  }

  /**
   * Get provider row element by provider type
   */
  private getProviderRow(providerType: string): Locator {
    return this.page.getByTestId(`provider-row-${providerType}`)
  }

  /**
   * Wait for provider to appear in list
   *
   * ✅ Fix: Use polling with count() instead of toBeVisible() for reliability.
   * count() directly checks element presence in DOM, better than visibility checks.
   * Compatible with React Query async updates.
   */
  private async waitForProviderInList(providerType: string, options?: { timeout?: number }): Promise<void> {
    const timeout = options?.timeout || 10000
    const startTime = Date.now()

    while (Date.now() - startTime < timeout) {
      const count = await this.getProviderRow(providerType).count()
      if (count > 0) {
        const isVisible = await this.getProviderRow(providerType).isVisible()
        if (isVisible) {
          return
        }
      }
      await this.page.waitForTimeout(100)
    }

    throw new Error(`Provider "${providerType}" did not appear in the list within ${timeout}ms`)
  }

  /**
   * Wait for provider to be removed from list
   *
   * ✅ Fix: Use polling with count() instead of not.toBeVisible() for reliability.
   * count() directly checks element presence in DOM, better than visibility checks.
   * Compatible with React Query async updates.
   */
  private async waitForProviderNotInList(providerType: string, options?: { timeout?: number }): Promise<void> {
    const timeout = options?.timeout || 2000
    const startTime = Date.now()

    while (Date.now() - startTime < timeout) {
      const count = await this.getProviderRow(providerType).count()
      if (count === 0) {
        return // Element has been removed from DOM
      }
      await this.page.waitForTimeout(100)
    }

    throw new Error(`Provider "${providerType}" was not removed from the list within ${timeout}ms`)
  }
}
