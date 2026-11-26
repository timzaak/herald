import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import type { UnifiedLogger } from 'playwright-unified-logger'

/**
 * Page Object for the Payment Providers management page (billing admin).
 *
 * Route: /{realmId}/manage/billing/payment-providers
 *
 * Frontend source:
 * - frontend/src/components/billing/payment-providers-page.tsx (page shell,
 *   provider rows, add/edit/delete buttons)
 * - frontend/src/components/billing/DeleteConfirmDialog.tsx (delete confirm)
 * - frontend/src/components/billing/AppleIapConfigForm.tsx (Apple config form)
 * - frontend/src/components/billing/GooglePlayConfigForm.tsx (Google config form)
 *
 * User stories:
 * - US-IAP-001: Configure App Store / Google Play IAP credentials (provider
 *   create/edit-keep/delete).
 *
 * Branching pattern: the provider row only renders `edit-${provider}-button`
 * when the provider is configured; otherwise an `add-${provider}-button` is
 * rendered in the unconfigured-providers toolbar. `configureIapProvider`
 * branches on `isConfigured` to click the correct entry — mirrors the
 * `configureStripe` precedent
 * (`demo/e2e/billing-admin/stripe-payment-comprehensive-demo.e2e.ts:358`).
 *
 * LOUD NOTE — edit-sensitive retention: in edit mode both Apple's `.p8`
 * private key and Google's service-account JSON are "leave empty to keep"
 * secret fields (`requireFieldOnCreate` skips the required check on edit;
 * `useSaveConfigMutation` only upserts keys that are present). To assert that
 * a secret is RETAINED across an edit, pass `editSensitiveLeaveEmpty: true`
 * and DO NOT fill the sensitive field — the successful save (return to the
 * provider list) is the retention proof, NOT a toast.
 */
export class PaymentProvidersPage extends BasePage {
  // --- Page shell -----------------------------------------------------------
  readonly container: Locator
  readonly providerList: Locator

  // --- Provider rows + row actions ------------------------------------------
  readonly appleProviderRow: Locator
  readonly googleProviderRow: Locator
  readonly wechatProviderRow: Locator
  readonly editAppleButton: Locator
  readonly editGoogleButton: Locator
  readonly editWechatButton: Locator
  readonly deleteAppleButton: Locator
  readonly deleteGoogleButton: Locator

  // --- Delete confirm dialog ------------------------------------------------
  readonly deleteConfirmDialog: Locator
  readonly deleteConfirmButton: Locator
  readonly deleteCancelButton: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.container = page.locator(SELECTORS.iap.paymentProvidersPage)
    this.providerList = page.locator(SELECTORS.iap.providerList)

    this.appleProviderRow = page.locator(SELECTORS.iap.appleProviderRow)
    this.googleProviderRow = page.locator(SELECTORS.iap.googleProviderRow)
    this.wechatProviderRow = page.locator(SELECTORS.iap.wechatProviderRow)
    this.editAppleButton = page.locator(SELECTORS.iap.editAppleButton)
    this.editGoogleButton = page.locator(SELECTORS.iap.editGoogleButton)
    this.editWechatButton = page.locator(SELECTORS.iap.editWechatButton)
    this.deleteAppleButton = page.locator(SELECTORS.iap.deleteAppleButton)
    this.deleteGoogleButton = page.locator(SELECTORS.iap.deleteGoogleButton)

    this.deleteConfirmDialog = page.locator(SELECTORS.iap.deleteConfirmDialog)
    this.deleteConfirmButton = page.locator(SELECTORS.iap.deleteConfirmButton)
    this.deleteCancelButton = page.locator(SELECTORS.iap.deleteCancelButton)
  }

  /**
   * Navigate to the payment-providers page for a given realm and wait for the
   * page shell to mount.
   */
  async goto(realmId: string = 'admin'): Promise<void> {
    await super.goto(`/${realmId}/manage/billing/payment-providers`)
    await expect(this.container).toBeVisible()
  }

  /**
   * Whether a provider is currently configured (the `edit-${provider}-button`
   * is visible). Returns false if the provider is unconfigured (only the
   * `add-${provider}-button` is present).
   */
  async isConfigured(provider: 'apple' | 'google' | 'wechat'): Promise<boolean> {
    return this.isVisible(this.getEditButton(provider))
  }

  /**
   * Get the locator for a provider's add button (rendered only when the
   * provider is unconfigured).
   */
  getAddButton(provider: 'apple' | 'google' | 'wechat'): Locator {
    return this.page.getByTestId(`add-${provider}-button`)
  }

  /**
   * Get the locator for a provider's edit button.
   */
  getEditButton(provider: 'apple' | 'google' | 'wechat'): Locator {
    const selector =
      provider === 'apple'
        ? SELECTORS.iap.editAppleButton
        : provider === 'google'
          ? SELECTORS.iap.editGoogleButton
          : SELECTORS.iap.editWechatButton
    return this.page.locator(selector)
  }

  /**
   * Configure (create or edit) an IAP provider, branching on whether it is
   * already configured. Mirrors the `configureStripe` branching precedent.
   *
   * - If the provider is configured → click `edit-${provider}-button` (edit).
   * - Otherwise → click `add-${provider}-button` (create).
   *
   * Then waits for the config form page to mount, fills the supplied fields,
   * submits, and waits for the return to the provider list.
   *
   * `editSensitiveLeaveEmpty` (edit only): when true, the sensitive secret
   * field (Apple `.p8` / Google service-account JSON) is NOT filled — this
   * asserts the prior secret is RETAINED (the backend keeps the existing key
   * when none is sent). The successful save back to the list is the retention
   * proof.
   */
  async configureIapProvider(
    provider: 'apple' | 'google',
    fields: AppleFields | GoogleFields,
    opts?: { editSensitiveLeaveEmpty?: boolean },
  ): Promise<void> {
    const configured = await this.isConfigured(provider)
    const editSensitiveLeaveEmpty = opts?.editSensitiveLeaveEmpty ?? false

    if (configured) {
      await this.smartClick(this.getEditButton(provider))
    } else {
      await this.smartClick(this.getAddButton(provider))
    }

    await this.page.waitForURL(`**/payment-providers/${provider}`, { timeout: 10000 })

    if (provider === 'apple') {
      await this.fillAppleForm(fields as AppleFields, {
        isEdit: configured,
        skipSensitive: editSensitiveLeaveEmpty,
      })
    } else {
      await this.fillGoogleForm(fields as GoogleFields, {
        isEdit: configured,
        skipSensitive: editSensitiveLeaveEmpty,
      })
    }
  }

  /**
   * Configure (create or edit) the WeChat Pay provider. Same branching,
   * retention and return-to-list contract as `configureIapProvider`; the
   * sensitive leave-empty-to-keep fields are the merchant private key PEM and
   * the APIv3 key (`is_secret` keys per `is_empty_secret_to_preserve`).
   */
  async configureWechatProvider(
    fields: WechatFields,
    opts?: { editSensitiveLeaveEmpty?: boolean },
  ): Promise<void> {
    const configured = await this.isConfigured('wechat')
    const skipSensitive = opts?.editSensitiveLeaveEmpty ?? false

    if (configured) {
      await this.smartClick(this.getEditButton('wechat'))
    } else {
      await this.smartClick(this.getAddButton('wechat'))
    }

    await this.page.waitForURL('**/payment-providers/wechat', { timeout: 10000 })
    await this.fillWechatForm(fields, { isEdit: configured, skipSensitive })
  }

  /**
   * Delete an IAP provider via the delete-confirm dialog. Assumes the provider
   * has no active subscriptions (otherwise `delete-confirm-button` is not
   * rendered — see the DeleteConfirmDialog selector note).
   *
   * Clicks `delete-${provider}-button`, waits for the confirm dialog, clicks
   * confirm, then waits for the provider row to disappear from the list.
   */
  async deleteIapProvider(provider: 'apple' | 'google'): Promise<void> {
    const deleteButton = this.page.locator(
      provider === 'apple' ? SELECTORS.iap.deleteAppleButton : SELECTORS.iap.deleteGoogleButton,
    )
    await this.smartClick(deleteButton)

    // The confirm dialog mounts (Radix AlertDialog). The confirm action button
    // only renders when there are no active subscriptions.
    await expect(this.deleteConfirmDialog).toBeVisible({ timeout: 5000 })
    await expect(this.deleteConfirmButton).toBeVisible({ timeout: 5000 })
    await this.smartClick(this.deleteConfirmButton)

    // Wait for the list refresh + the provider row to disappear. The delete
    // mutation invalidates the providers query on success, which re-renders
    // the list without the deleted row.
    const providerRow = this.page.locator(
      provider === 'apple' ? SELECTORS.iap.appleProviderRow : SELECTORS.iap.googleProviderRow,
    )
    await expect(providerRow).toHaveCount(0, { timeout: 15000 })
  }

  // ==================== Form fill helpers (private) ====================

  /**
   * Fill the Apple config form. In create mode all fields are filled; in edit
   * mode with `skipSensitive`, the `.p8` private key is left empty to assert
   * retention of the prior secret.
   */
  private async fillAppleForm(
    fields: AppleFields,
    opts: { isEdit: boolean; skipSensitive: boolean },
  ): Promise<void> {
    await expect(this.page.getByTestId('apple-config-form-page')).toBeVisible()

    // bundleId / issuerId / keyId are always fillable (no secret retention).
    await this.fillField(this.page.getByTestId('apple-bundle-id-input'), fields.bundleId)
    await this.fillField(this.page.getByTestId('apple-issuer-id-input'), fields.issuerId)
    await this.fillField(this.page.getByTestId('apple-key-id-input'), fields.keyId)

    // The .p8 private key is a secret. On create it is required; on edit with
    // skipSensitive it is intentionally left empty (leave-empty-to-keep).
    if (!opts.isEdit || !opts.skipSensitive) {
      await this.fillField(this.page.getByTestId('apple-private-key-p8-input'), fields.privateKeyP8)
    }

    // Environment is a Radix Select; open the trigger and pick the option by
    // its value. The option `<SelectItem value="production|sandbox">` is matched
    // via getByRole('option') (Radix Select precedent —
    // admin-subscription-history-demo.e2e.ts:226).
    await this.selectAppleEnvironment(fields.environment)

    await this.page.getByTestId('apple-config-page-submit-button').click()
    await this.page.waitForURL('**/payment-providers', { timeout: 15000 })
  }

  /**
   * Fill the Google config form. In create mode all fields are filled; in edit
   * mode with `skipSensitive`, the service-account JSON is left empty to assert
   * retention of the prior secret.
   */
  private async fillGoogleForm(
    fields: GoogleFields,
    opts: { isEdit: boolean; skipSensitive: boolean },
  ): Promise<void> {
    await expect(this.page.getByTestId('google-config-form-page')).toBeVisible()

    // packageName is always fillable.
    await this.fillField(this.page.getByTestId('google-package-name-input'), fields.packageName)

    // The service-account JSON is a secret. On create it is required; on edit
    // with skipSensitive it is intentionally left empty (leave-empty-to-keep).
    if (!opts.isEdit || !opts.skipSensitive) {
      await this.fillField(
        this.page.getByTestId('google-service-account-json-input'),
        fields.serviceAccountJson,
      )
    }

    await this.page.getByTestId('google-config-page-submit-button').click()
    await this.page.waitForURL('**/payment-providers', { timeout: 15000 })
  }

  /**
   * Fill the WeChat Pay config form. In create mode all required fields are
   * filled; in edit mode with `skipSensitive`, the merchant private key and
   * APIv3 key are left empty to assert retention of the prior secrets.
   */
  private async fillWechatForm(
    fields: WechatFields,
    opts: { isEdit: boolean; skipSensitive: boolean },
  ): Promise<void> {
    await expect(this.page.getByTestId('wechat-config-form-page')).toBeVisible()

    await this.fillField(this.page.getByTestId('wechat-app-id-input'), fields.appId)
    await this.fillField(this.page.getByTestId('wechat-mch-id-input'), fields.mchId)
    await this.fillField(this.page.getByTestId('wechat-serial-no-input'), fields.serialNo)
    await this.fillField(this.page.getByTestId('wechat-notify-url-input'), fields.notifyUrl)

    // The merchant private key and APIv3 key are secrets. On create they are
    // required; on edit with skipSensitive they are intentionally left empty
    // (leave-empty-to-keep).
    if (!opts.isEdit || !opts.skipSensitive) {
      await this.fillField(this.page.getByTestId('wechat-private-key-input'), fields.privateKeyPem)
      await this.fillField(this.page.getByTestId('wechat-v3-key-input'), fields.apiV3Key)
    }

    if (fields.platformPublicKeyPem) {
      await this.fillField(
        this.page.getByTestId('wechat-platform-public-key-input'),
        fields.platformPublicKeyPem,
      )
    }

    await this.page.getByTestId('wechat-config-page-submit-button').click()
    await this.page.waitForURL('**/payment-providers', { timeout: 15000 })
  }

  /**
   * Select the Apple notification environment (production/sandbox) via the
   * Radix Select trigger + option.
   *
   * The option's accessible name is the LOCALIZED label (e.g. "Production" /
   * "生产"), so matching by role+name is locale-coupled and brittle. The codebase
   * `SelectItem` wrapper (frontend/src/components/ui/select.tsx) exposes
   * `data-value={value}` on each option, so we match on that stable attribute
   * instead — locale-independent and survives translation.
   */
  private async selectAppleEnvironment(environment: 'production' | 'sandbox'): Promise<void> {
    await this.smartClick(this.page.getByTestId('apple-environment-select-trigger'))
    // Radix Select content is portaled; match the option by its `data-value`
    // (the SelectItem value prop), not its localized visible label.
    await this.page
      .locator(`[role="option"][data-value="${environment}"]`)
      .click()
  }
}

// ==================== Field type contracts ====================

/**
 * Apple IAP config form fields (AppleIapConfigForm.tsx).
 */
export interface AppleFields {
  /** Bundle ID, e.g. "com.example.app". */
  bundleId: string
  /** App Store Connect Issuer ID (UUID-ish). */
  issuerId: string
  /** App Store Connect Key ID (10-char). */
  keyId: string
  /** .p8 private key (PEM). Required on create; leave empty to keep on edit. */
  privateKeyP8: string
  /** Notification environment. */
  environment: 'production' | 'sandbox'
}

/**
 * Google Play config form fields (GooglePlayConfigForm.tsx).
 */
export interface GoogleFields {
  /** Android package name, e.g. "com.example.app". */
  packageName: string
  /** Service-account JSON literal. Required on create; leave empty on edit. */
  serviceAccountJson: string
}

/**
 * WeChat Pay config form fields (WechatConfigForm.tsx).
 */
export interface WechatFields {
  appId: string
  mchId: string
  /** Merchant certificate serial number. */
  serialNo: string
  /** Merchant private key PEM. Required on create; leave empty on edit. */
  privateKeyPem: string
  /** APIv3 key, exactly 32 chars. Required on create; leave empty on edit. */
  apiV3Key: string
  notifyUrl: string
  /** Optional manual platform-public-key override (non-secret). */
  platformPublicKeyPem?: string
}
