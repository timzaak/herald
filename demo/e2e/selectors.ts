/**
 * Centralized selectors for Herald frontend E2E tests.
 */

export const SELECTORS = {
  login: {
    container: '[data-testid="login-card"], [data-testid="login-container"]',
    title: '[data-testid="login-title"]',
    usernameInput: '[data-testid="email-input"]',
    emailInput: '[data-testid="email-input"]',
    passwordInput: '[data-testid="password-input"]',
    submitButton: '[data-testid="login-submit-button"]',
    errorMessage: '[data-testid="login-error-message"]',
  },

  dashboard: {
    heading: 'h1:has-text("Dashboard")',
    statsRow: '[data-testid="dashboard-stats-row"]',
    totalUsersCard: '[data-testid="dashboard-total-users-card"]',
    newUsersCard: '[data-testid="dashboard-new-users-card"]',
    activeUsersCard: '[data-testid="dashboard-active-users-card"]',
    authTrendChart: '[data-testid="dashboard-auth-trend-chart"]',
    quickNav: '[data-testid="dashboard-quick-nav"]',
    quickNavUsers: '[data-testid="dashboard-users-card"]',
    quickNavRoles: '[data-testid="dashboard-roles-card"]',
    quickNavPermissions: '[data-testid="dashboard-permissions-card"]',
    quickNavClientApps: '[data-testid="dashboard-client-apps-card"]',
    quickNavRealms: '[data-testid="dashboard-realms-card"]',
    quickNavSettings: '[data-testid="dashboard-settings-card"]',
    errorState: '[data-testid="dashboard-error"]',
    retryButton: '[data-testid="dashboard-retry-button"]',
    chartSkeleton: '[data-testid="dashboard-chart-skeleton"]',
  },

  sidebar: {
    container: '[data-testid="admin-sidebar"]',
    menuDashboard: '[data-testid="sidebar-menu-dashboard"]',
    menuUsers: '[data-testid="sidebar-menu-users"]',
    menuRoles: '[data-testid="sidebar-menu-roles"]',
    menuSettings: '[data-testid="sidebar-menu-settings"]',
    menuRealms: '[data-testid="sidebar-menu-realms"]',
    menuClientApps: '[data-testid="sidebar-menu-client-apps"]',
    menuAuthorization: '[data-testid="sidebar-menu-authorization"]',
    menuPermissions: '[data-testid="sidebar-menu-permissions"]',
    menuApiKeys: '[data-testid="sidebar-menu-api-keys"]',
    menuAuditLog: '[data-testid="sidebar-menu-audit-log"]',
    menuEntitlementMappings: '[data-testid="sidebar-menu-entitlement-mappings"]',
  },

  languageSwitcher: {
    trigger: '[data-testid="language-switcher"]',
    enItem: '[data-testid="language-switcher-item-en"]',
    zhItem: '[data-testid="language-switcher-item-zh-CN"]',
  },

  header: {
    container: '[data-testid="admin-header"]',
    userAvatar: '[data-testid="user-avatar"]',
    logoutButton: '[data-testid="logout-menu-item"]',
    userMenu: '[data-testid="user-menu"]',
    userDisplayName: '[data-testid="user-display-name"]',
  },

  audit: {
    container: '[data-testid="audit-page"]',
    heading: '[data-testid="audit-heading"]',
    table: '[data-testid="audit-table"]',
    filterBar: '[data-testid="audit-filter-bar"]',
    filterCategory: '[data-testid="audit-category-select"]',
    filterAction: '[data-testid="audit-action-select"]',
    filterActorId: '[data-testid="audit-actor-input"]',
    filterStartDate: '[data-testid="audit-start-date-input"]',
    filterEndDate: '[data-testid="audit-end-date-input"]',
    filterClear: '[data-testid="audit-clear-filters-button"]',
    tableLoading: '[data-testid="audit-table-loading"]',
    detailSheet: '[data-testid="audit-detail-sheet"]',
    detailError: '[data-testid="audit-detail-error"]',
    detailClose: '[data-testid="audit-detail-close-button"]',
    detailJson: '[data-testid="audit-detail-json"]',
    detailResult: '[data-testid="audit-detail-result"]',
    pagination: '[data-testid="audit-pagination"]',
    paginationPrevious: '[data-testid="audit-pagination-previous"]',
    paginationNext: '[data-testid="audit-pagination-next"]',
  },

  users: {
    container: '[data-testid="users-page"]',
    heading: '[data-testid="users-heading"]',
    table: '[data-testid="users-table"]',
    addButton:
      '[data-testid="add-user-button"], [data-testid="create-user-button"]',
    searchInput: '[data-testid="users-search-input"]',
    roleCheckbox: '[data-testid="user-create-role-checkbox"]',
    deleteDialog: '[data-testid="delete-user-dialog"]',
    confirmDeleteButton: '[data-testid="confirm-delete-user-button"]',
  },

  /**
   * Reset Password Dialog Selectors
   * Triggered from: /{realmId}/manage/users (user table row action)
   *
   * The row-level reset password button uses a dynamic testid
   * (user-table-{row.index}-reset-password-button) and is constructed
   * per-row in the POM via row-relative locator.
   */
  resetPassword: {
    confirmDialog: '[data-testid="reset-password-dialog"]',
    confirmButton: '[data-testid="confirm-reset-password-button"]',
    resultDialog: '[data-testid="reset-password-result-dialog"]',
    newPasswordText: '[data-testid="new-password-text"]',
    copyButton: '[data-testid="copy-password-button"]',
  },

  /**
   * User Sessions Dialog Selectors (US-RA-020)
   *
   * Triggered from: /{realmId}/manage/users (user table row "Manage Sessions"
   * button). Every selector below is sourced verbatim from
   * `frontend/src/components/users/user-sessions-dialog.tsx` (line numbers cited).
   *
   * Empty-state GAP: `user-sessions-dialog.tsx:114` is a bare `<p>` rendering
   * `m['users.sessions.empty']()` with NO `data-testid`. Do NOT invent a testid.
   * Assert empty state via `expectRevokeAllButtonAbsent` (the revoke-all button
   * only renders when the list is non-empty — `:84` `{hasSessions && ...}`).
   * This is locale-independent and stable.
   *
   * The per-row entry button lives on the user table row (NOT this dialog) at
   * `frontend/src/components/users/user-table.tsx:131` and uses a dynamic testid
   * `user-table-${row.index}-sessions-button`; the POM resolves it via a
   * row-relative suffix-match locator (`[data-testid$="-sessions-button"]`).
   */
  userSessions: {
    // DialogContent root — user-sessions-dialog.tsx:77/84
    dialog: '[data-testid="user-sessions-dialog"]',
    // Rendered ONLY when the session list is non-empty — :89/99
    revokeAllButton: '[data-testid="user-sessions-revoke-all-button"]',
    // Error-state retry — :108/118
    retryButton: '[data-testid="user-sessions-retry-button"]',
    // Per-row revoke button — :149/159 (dynamic index)
    revokeRowButton: (index: number) =>
      `[data-testid="user-sessions-table-${index}-revoke-button"]`,
    // Revoke-one ConfirmDialog props — user-sessions-dialog.tsx:180-182
    revokeConfirmDialog: '[data-testid="user-sessions-revoke-confirm-dialog"]',
    revokeCancelButton: '[data-testid="user-sessions-revoke-cancel-button"]',
    revokeConfirmButton: '[data-testid="user-sessions-revoke-confirm-button"]',
    // Revoke-all ConfirmDialog props — :194-196
    revokeAllConfirmDialog:
      '[data-testid="user-sessions-revoke-all-confirm-dialog"]',
    revokeAllCancelButton:
      '[data-testid="user-sessions-revoke-all-cancel-button"]',
    revokeAllConfirmButton:
      '[data-testid="user-sessions-revoke-all-confirm-button"]',
  },

  roles: {
    container: '[data-testid="roles-page"]',
    heading: '[data-testid="roles-heading"]',
    table: '[data-testid="role-table"]',
    addButton: '[data-testid="role-create-button"]',
    permissionsButton: '[data-testid="permissions-button"]',
    createNameInput: '[data-testid="role-create-name-input"]',
    createDescriptionInput: '[data-testid="role-create-description-input"]',
    createSubmitButton: '[data-testid="role-create-submit-button"]',
    editNameInput: '[data-testid="role-edit-name-input"]',
    editDescriptionInput: '[data-testid="role-edit-description-input"]',
    editSubmitButton: '[data-testid="role-edit-submit-button"]',
  },

  permissions: {
    container: '[data-testid="permissions-page"]',
    heading: '[data-testid="permissions-heading"]',
    table: '[data-testid="permissions-table"]',
    addButton: '[data-testid="permission-create-button"]',
    createNameInput: '[data-testid="permission-create-name-input"]',
    createDescriptionInput:
      '[data-testid="permission-create-description-input"]',
    createSubmitButton: '[data-testid="permission-create-submit-button"]',
    editNameInput: '[data-testid="permission-edit-name-input"]',
    editDescriptionInput: '[data-testid="permission-edit-description-input"]',
    editSubmitButton: '[data-testid="permission-edit-submit-button"]',
  },

  realms: {
    container: '[data-testid="realms-page"]',
    heading: '[data-testid="realms-heading"]',
    table: '[data-testid="realms-table"]',
    addButton:
      '[data-testid="add-realm-button"], [data-testid="create-realm-button"]',
  },

  profile: {
    container: '[data-testid="profile-page"]',
    heading: '[data-testid="profile-heading"]',
    sidebarContainer: '[data-testid="profile-sidebar"]',
    menuProfile: '[data-testid="profile-menu-profile"]',
    menuSecurity: '[data-testid="profile-menu-security"]',
    logoutButton: '[data-testid="profile-logout-button"]',
    headerContainer: '[data-testid="profile-header"]',
    emailField: '[data-testid="profile-email"]',
    nicknameField: '[data-testid="profile-nickname"]',
    nicknameInput: '[data-testid="nickname-input"]',
    statusField: '[data-testid="profile-status"]',
    saveButton: '[data-testid="save-profile-button"]',
    changePasswordHeading: '[data-testid="change-password-heading"]',
    oldPasswordInput: '[data-testid="change-password-old-input"]',
    newPasswordInput: '[data-testid="change-password-new-input"]',
    confirmPasswordInput: '[data-testid="change-password-confirm-input"]',
    changePasswordSubmitButton: '[data-testid="change-password-submit-button"]',
  },

  security: {
    pageTitle: '[data-testid="security-page-title"]',
    passwordSectionTitle: '[data-testid="password-section-title"]',
    totpSectionTitle: '[data-testid="totp-section-title"]',
    totpStatusCard: '[data-testid="totp-status-card"]',
    totpStatusCardEnabled: '[data-testid="totp-status-card-enabled"]',
    totpEnableButton: '[data-testid="totp-enable-button"]',
    totpDisableButton: '[data-testid="totp-disable-button"]',
    totpRegenerateButton: '[data-testid="totp-regenerate-button"]',
    totpSetupPage: '[data-testid="totp-setup-page"]',
    totpSetupPageTitle: '[data-testid="totp-setup-page-title"]',
    totpSetupPageDescription: '[data-testid="totp-setup-page-description"]',
    totpSetupBackToSecurity: '[data-testid="totp-setup-back-to-security"]',
    totpSetupStepPassword: '[data-testid="totp-setup-step-password"]',
    totpSetupPasswordInput: '[data-testid="totp-setup-password-input"]',
    totpSetupGenerateButton: '[data-testid="totp-setup-generate-button"]',
    totpPasswordError: '[data-testid="totp-password-error"]',
    totpSetupStepQRCode: '[data-testid="totp-setup-step-qr-code"]',
    totpQRCodeContainer: '[data-testid="totp-qr-code-container"]',
    totpQRCode: '[data-testid="totp-qr-code"]',
    totpSecretKey: '[data-testid="totp-qr-code-container"]',
    totpSetupBackButton: '[data-testid="totp-setup-back-button"]',
    totpSetupNextButton: '[data-testid="totp-setup-next-button"]',
    totpSetupStepVerify: '[data-testid="totp-setup-step-verify"]',
    totpOtpInput: '[data-testid="totp-otp-input"]',
    totpOtpDigit: (index: number) => `[data-testid="totp-otp-digit-${index}"]`,
    totpVerifyBackButton: '[data-testid="totp-verify-back-button"]',
    totpVerifySubmitButton: '[data-testid="totp-verify-submit-button"]',
    totpVerifyLoading: '[data-testid="totp-verify-loading"]',
    totpSavedBackupCodesCheckbox:
      '[data-testid="totp-saved-backup-codes-checkbox"]',
    totpSavedBackupCodesLabel: '[data-testid="totp-saved-backup-codes-label"]',
    totpDisablePasswordInput: '[data-testid="totp-disable-password-input"]',
    totpDisableCancelButton: '[data-testid="totp-disable-cancel-button"]',
    totpDisableSubmitButton: '[data-testid="totp-disable-submit-button"]',
    totpRegeneratePasswordInput:
      '[data-testid="totp-regenerate-password-input"]',
    totpRegenerateCancelButton: '[data-testid="totp-regenerate-cancel-button"]',
    backupCodesCopyAllButton: '[data-testid="backup-codes-copy-all-button"]',
    backupCode: (index: number) => `[data-testid="backup-code-${index}"]`,
    totpEnabledAt: '[data-testid="totp-enabled-at"]',
    totpLastVerifiedAt: '[data-testid="totp-last-verified-at"]',
    totpRemainingBackupCodes: '[data-testid="totp-remaining-backup-codes"]',
  },

  clientApps: {
    page: '[data-testid="client-apps-page"]',
    heading: '[data-testid="client-apps-heading"]',
    table: '[data-testid="client-apps-table"]',
    addButton: '[data-testid="add-client-app-button"]',
    searchInput: '[data-testid="client-apps-search-input"]',
    rowByIndex: (index: number) => `[data-testid="client-app-row-${index}"]`,
    row: (appId: string) => `[data-app-id="${appId}"]`,
    rowByClientId: (clientId: string) => `[data-client-id="${clientId}"]`,
    editButton: (appId: string) =>
      `[data-app-id="${appId}"] [data-testid="edit-client-app-button"]`,
    deleteButton: (appId: string) =>
      `[data-app-id="${appId}"] [data-testid="delete-client-app-button"]`,
    enabledSwitch: (appId: string) =>
      `[data-app-id="${appId}"] [data-testid="client-app-enabled-switch"]`,
  },

  clientAppForm: {
    page: '[data-testid="client-app-form-page"]',
    pageTitle: '[data-testid="page-title"]',
    tabBasic: '[data-testid="tab-basic"]',
    tabRedirectUris: '[data-testid="tab-redirect-uris"]',
    tabSecurity: '[data-testid="tab-security"]',
    tabAppearance: '[data-testid="tab-appearance"]',
    clientIdInput: '[data-testid="client-id-input"]',
    clientIdDisplay: '[data-testid="client-id-display"]',
    nameInput: '[data-testid="client-app-name-input"]',
    descriptionInput: '[data-testid="client-app-description-input"]',
    redirectUrisInput: '[data-testid="redirect-uris-input-field"]',
    enabledSwitch: '[data-testid="client-app-enabled-switch"]',
    // Security-tab TTL field was renamed sessionTtlSeconds → browserRefreshAbsoluteTtlSeconds
    // (commit f3b8d48a browser bearer-token model). The frontend input testid is
    // `browser-refresh-ttl-input` with preset buttons `browser-refresh-ttl-preset-{label}`.
    sessionTtlInput: '[data-testid="browser-refresh-ttl-input"]',
    sessionTtlPreset: (label: string) =>
      `[data-testid="browser-refresh-ttl-preset-${label}"]`,
    sessionRenewalTtlInput: '[data-testid="session-renewal-ttl-input"]',
    deviceCodeGrantSwitch: '[data-testid="device-code-grant-switch"]',
    regenerateSecretSwitch: '[data-testid="regenerate-secret-switch"]',
    iconUrlInput: '[data-testid="icon-url-input"]',
    cancelButton: '[data-testid="cancel-button"]',
    submitButton: '[data-testid="submit-button"]',
  },

  apiKeys: {
    page: '[data-testid="api-keys-page"]',
    heading: '[data-testid="api-keys-heading"]',
    table: '[data-testid="api-keys-table"]',
    addButton: '[data-testid="add-api-key-button"]',
    name: '[data-testid="api-key-name"]',
    enabledSwitch: '[data-testid="api-key-enabled-switch"]',
    statusBadge: '[data-testid="api-key-status-badge"]',
    expires: '[data-testid="api-key-expires"]',
    lastUsed: '[data-testid="api-key-last-used"]',
    clientApp: '[data-testid="api-key-client-app"]',
    editButton: '[data-testid="edit-api-key-button"]',
    deleteButton: '[data-testid="delete-api-key-button"]',
    rolesCell: '[data-testid="api-key-roles-cell"]',
    rolesOverflow: '[data-testid="api-key-roles-overflow"]',
    manageRolesButton: '[data-testid="manage-api-key-roles-button"]',
  },

  apiKeyForm: {
    page: '[data-testid="api-key-form-page"]',
    pageTitle: '[data-testid="page-title"]',
    backButton: '[data-testid="api-key-form-back-button"]',
    nameInput: '[data-testid="api-key-name-input"]',
    clientAppSelectorTrigger: '[data-testid="client-app-selector-trigger"]',
    clientAppSelectorSearch: '[data-testid="client-app-selector-search"]',
    clientAppSelectorDefault: '[data-testid="client-app-selector-default"]',
    clientAppSelectorItem: (appId: string) =>
      `[data-testid="client-app-selector-item-${appId}"]`,
    enabledSwitch: '[data-testid="api-key-enabled-switch"]',
    expiresAtInput: '[data-testid="api-key-expires-at-input"]',
    expiresAtClearButton: '[data-testid="api-key-expires-at-clear-button"]',
    cancelButton: '[data-testid="cancel-button"]',
    submitButton: '[data-testid="submit-button"]',
  },

  apiKeyReveal: {
    page: '[data-testid="api-key-reveal-page"]',
    pageTitle: '[data-testid="page-title"]',
    backButton: '[data-testid="api-key-reveal-back-button"]',
    keyValue: '[data-testid="api-key-reveal-value"]',
    copyButton: '[data-testid="copy-api-key-button"]',
    doneButton: '[data-testid="api-key-reveal-done-button"]',
  },

  apiKeyDelete: {
    dialog: '[data-testid="delete-confirmation-dialog"]',
    cancelButton: '[data-testid="cancel-delete-button"]',
    confirmButton: '[data-testid="confirm-delete-button"]',
  },

  apiKeyRoles: {
    dialogContent: '[data-testid="api-key-roles-dialog-content"]',
    dialogTitle: '[data-testid="api-key-roles-dialog-title"]',
    dialogName: '[data-testid="api-key-roles-dialog-name"]',
    dialogClose: '[data-testid="api-key-roles-dialog-close"]',
    roleSelectorTrigger: '[data-testid="role-selector-trigger"]',
    roleSelectorSearch: '[data-testid="role-selector-search"]',
    roleSelectorItem: (roleId: string) =>
      `[data-testid="role-selector-item-${roleId}"]`,
  },

  /**
   * Common Component Selectors
   */
  common: {
    dialog: '[data-testid="dialog"]',
    dialogTitle: '[data-testid="dialog-title"]',
    dialogContent: '[data-testid="dialog-content"]',
    dialogCloseButton: '[data-testid="dialog-close-button"]',
    dialogCancelButton: '[data-testid="dialog-cancel-button"]',
    dialogSubmitButton: '[data-testid="dialog-submit-button"]',

    form: '[data-testid="form"]',
    formEmailInput: '[data-testid="email-input"]',
    formPasswordInput: '[data-testid="password-input"]',
    formNicknameInput: '[data-testid="nickname-input"]',
    formNameInput: '[data-testid="name-input"]',

    // Sonner toast uses .data-[state=open]:animate-in to show toasts.
    toast: '[data-testid="toast"], [data-sonner-toast]',
    toastMessage:
      '[data-testid="toast-message"], [data-sonner-toast] [data-description]',
    successMessage:
      '[data-testid="success-message"], [data-sonner-toast].success',
    errorMessage: '[data-testid="error-message"], [data-sonner-toast].error',

    loading: '[data-testid="loading"]',
    spinner: '[data-testid="spinner"]',
  },

  subscriptionHistory: {
    page: '[data-testid="subscription-history-page"]',
    filterContainer: '[data-testid="subscription-history-filter"]',
    listContainer: '[data-testid="subscription-history-list"]',
    backButton: '[data-testid="back-button"]',
    filterUserId: '[data-testid="filter-user-id"]',
    filterPlan: '[data-testid="filter-plan"]',
    filterEventType: '[data-testid="filter-event-type"]',
    filterStatus: '[data-testid="filter-status"]',
    filterFromDate: '[data-testid="filter-from-date"]',
    filterToDate: '[data-testid="filter-to-date"]',
    filterSortBy: '[data-testid="filter-sort-by"]',
    filterSortOrder: '[data-testid="filter-sort-order"]',
    resetFilterButton: '[data-testid="reset-filter-button"]',
    applyFilterButton: '[data-testid="apply-filter-button"]',
    historyRow: (index: number) => `[data-testid="history-row-${index}"]`,
    previousPageButton: '[data-testid="previous-page-button"]',
    nextPageButton: '[data-testid="next-page-button"]',
  },

  subscriptionDetailHistory: {
    page: '[data-testid="subscription-detail-history-page"]',
    timelineContainer: '[data-testid="subscription-timeline"]',
    timelineLoading: '[data-testid="timeline-loading"]',
    timelineEmpty: '[data-testid="timeline-empty"]',
    backButton: '[data-testid="back-button"]',
    timelineEvent: (index: number) => `[data-testid="timeline-event-${index}"]`,
    viewEventDetailsButton: (index: number) =>
      `[data-testid="view-event-details-${index}"]`,
    eventDetailDialog: '[data-testid="event-detail-dialog"]',
    eventBadge: (
      type:
        | "created"
        | "upgraded"
        | "downgraded"
        | "canceled"
        | "renewed"
        | "reactivated"
        | "expired",
    ) => `[data-testid="event-badge-${type}"]`,
  },

  billing: {
    page: '[data-testid="billing-page"]',
    navEntitlementMappings: '[data-testid="billing-nav-entitlement-mappings"]',
    navSubscriptions: '[data-testid="billing-nav-subscriptions"]',
  },

  adminSubscriptionList: {
    page: '[data-testid="admin-subscription-list-page"]',
    heading: '[data-testid="admin-subscription-list-heading"]',
    table: '[data-testid="admin-subscription-list-table"]',
    entitlementKeyFilterInput: '[data-testid="entitlement-key-filter-input"]',
    statusFilterSelect: '[data-testid="status-filter-select"]',
    paymentProviderFilterSelect: '[data-testid="payment-provider-filter-select"]',
    subscriptionRow: (id: string) => `[data-testid="subscription-row-${id}"]`,
    firstSubscriptionRow: () => '[data-testid^="subscription-row-"]',
    emptyState: '[data-testid="admin-subscription-list-empty-state"]',
    pagination: '[data-testid="admin-subscription-list-pagination"]',
  },

  /**
   * Points Management Page Selectors (Admin)
   */
  pointsAdmin: {
    accountsPage: '[data-testid="points-wallets-page"]',
    heading: 'h1:has-text("Points Management")',
    accountsSection: '[data-testid="points-wallets-page"]',
    accountsTable: '[data-testid="points-wallets-page"]',
    accountsSearch: '[data-testid="wallets-search-input"]',
    accountRow: (userId: string) => `[data-testid="wallet-row-${userId}"]`,
    firstAccountRow: () => '[data-testid^="wallet-row-"]',
    transactionsSection:
      '[data-testid="transaction-history-table"], [data-testid="no-transactions"]',
    transactionsTable: '[data-testid="transaction-history-table"]',
    transactionRow: (index: number) =>
      `[data-testid="transaction-row-${index}"]`,
    transactionType: (index: number) =>
      `[data-testid="transaction-type-${index}"]`,
    transactionAmount: (index: number) =>
      `[data-testid="transaction-amount-${index}"]`,
    transactionBalance: (index: number) =>
      `[data-testid="transaction-balance-${index}"]`,
    transactionDescription: (index: number) =>
      `[data-testid="transaction-description-${index}"]`,
    transactionTime: (index: number) =>
      `[data-testid="transaction-time-${index}"]`,
    transactionFilters: '[data-testid="transaction-filters"]',
    filterType: '[data-testid="filter-transaction-type"]',
    filterStartTime: '[data-testid="filter-from-date"]',
    filterEndTime: '[data-testid="filter-to-date"]',
    filterClientApp: '[data-testid="filter-client-app"]',
    resetFiltersButton: '[data-testid="clear-filters-button"]',
    applyFiltersButton: '[data-testid="apply-filters-button"]',
    // Credit-bucket admin wallets (US-CB cross-tenant bucket view):
    // each row keyed by (userId, bucketId). Bucket filter Select + optional
    // cross-bucket total card rendered above the list.
    walletRowByBucket: (userId: string, bucketId: string) =>
      `[data-testid="admin-wallet-row-${userId}-${bucketId}"]`,
    bucketFilter: '[data-testid="admin-wallets-bucket-filter"]',
    crossBucketTotal: '[data-testid="admin-wallets-cross-bucket-total"]',
  },
  /**
   * Points User Page Selectors
   */
  pointsUser: {
    page: '[data-testid="user-points-page"]',
    heading: 'h1:has-text("My Points")',
    balanceCard: '[data-testid="points-balance-card"]',
    balanceAmount: '[data-testid="points-balance"]',
    accountStatus: '[data-testid="points-wallet-status"]',
    // Rendered when the user holds no points buckets at all (no wallets).
    // Distinct from `pointsUsageDashboard.emptyState` (points-empty-state),
    // which renders INSIDE a bucket card when the bucket exists but has no
    // quota and no balance.
    balanceEmpty: '[data-testid="points-balance-empty"]',
    // Credit-bucket: the bucket-aware UI renders one
    // `points-balance-card-${bucketId}` per held bucket (PointsBalanceCard.tsx).
    // The flat `points-balance-card` testid above only matches the loading
    // skeleton or a null-bucket fallback card. Sibling demos that just need to
    // assert "the user has at least one balance card rendered" (without
    // resolving a specific bucket UUID) use this prefix locator. Demos that
    // care about a SPECIFIC bucket use `balanceCardByBucket(bucketId)`.
    firstBalanceCard: '[data-testid^="points-balance-card-"]',
    // Bucket-grouped balances (credit-bucket US-CB-005). Per-bucket card +
    // per-type chip + cross-bucket total (only rendered when ≥2 buckets held).
    balanceCardByBucket: (bucketId: string) =>
      `[data-testid="points-balance-card-${bucketId}"]`,
    balanceCardDisabledBadge: (bucketId: string) =>
      `[data-testid="points-balance-card-disabled-${bucketId}"]`,
    balanceTotalByBucket: (bucketId: string) =>
      `[data-testid="points-balance-total-${bucketId}"]`,
    balanceType: (bucketId: string, typeKey: string) =>
      `[data-testid="points-balance-type-${bucketId}-${typeKey}"]`,
    crossBucketTotal: '[data-testid="user-points-cross-bucket-total"]',
    // Transaction bucket dimension (credit-bucket US-CB-006).
    // No header testid exists; only per-row bucket cells — assert on row cells.
    transactionBucketCell: (rowIndex: number) =>
      `[data-testid="transaction-bucket-${rowIndex}"]`,
    filterBucket: '[data-testid="filter-bucket"]',
    transactionsSection:
      '[data-testid="transaction-history-table"], [data-testid="no-transactions"]',
    transactionsTable: '[data-testid="transaction-history-table"]',
    transactionRow: (index: number) =>
      `[data-testid="transaction-row-${index}"]`,
    transactionType: (index: number) =>
      `[data-testid="transaction-type-${index}"]`,
    transactionAmount: (index: number) =>
      `[data-testid="transaction-amount-${index}"]`,
    transactionDescription: (index: number) =>
      `[data-testid="transaction-description-${index}"]`,
    transactionTime: (index: number) =>
      `[data-testid="transaction-time-${index}"]`,
    filterType: '[data-testid="filter-transaction-type"]',
    filterStartTime: '[data-testid="filter-from-date"]',
    filterEndTime: '[data-testid="filter-to-date"]',
    resetFiltersButton: '[data-testid="clear-filters-button"]',
    applyFiltersButton: '[data-testid="apply-filters-button"]',
    exportButton: '[data-testid="export-transactions-button"]',
    firstTransactionRow: () => '[data-testid^="transaction-row-"]',
  },

  /**
   * Grant Points Dialog Selectors
   */
  grantPoints: {
    grantPointsButton: '[data-testid="grant-points-button"]',
    formDialog: '[data-testid="grant-points-form-dialog"]',
    userSearchInput: '[data-testid="grant-points-user-search-input"] input',
    amountInput: '[data-testid="grant-points-amount-input"]',
    validityDaysInput: '[data-testid="grant-points-validity-days-input"]',
    permanentToggle: '[data-testid="grant-points-permanent-toggle"]',
    reasonInput: '[data-testid="grant-points-reason-input"]',
    cancelButton: '[data-testid="grant-points-cancel-button"]',
    submitButton: '[data-testid="grant-points-submit-button"]',
    confirmDialog: '[data-testid="grant-points-confirm-dialog"]',
    confirmButton: '[data-testid="grant-points-confirm-button"]',
    errorMessage: '[data-testid="grant-points-error-message"]',
    // Target Bucket Select (credit-bucket US-CB: bucketId is required).
    bucketSelect: '[data-testid="grant-points-bucket-select"]',
  },

  /**
   * Points Configuration Selectors (Admin)
   */
  points: {
    registrationBonusPointsInput:
      '[data-testid="registration-bonus-points-input"]',
    freePeriodicPointsAmountInput:
      '[data-testid="free-periodic-points-amount-input"]',
    freePeriodicGrantPeriodTypeSelect:
      '[data-testid="grant-period-type-select"]',
    freePeriodicValidityDaysInput:
      '[data-testid="free-periodic-validity-days-input"]',
    saveButton: '[data-testid="save-config-button"]',
    successMessage: '[data-testid="success-message"]',
    errorMessage: '[data-testid="error-message"]',

    totalFreeUsers: '[data-testid="total-free-users"]',
    activeFreeUsers: '[data-testid="active-free-users"]',
    upgradeRate: '[data-testid="upgrade-rate"]',
    userGrowthChart: '[data-testid="user-growth-chart"]',
    pointsGrantedChart: '[data-testid="points-granted-chart"]',
    upgradeRateChart: '[data-testid="upgrade-rate-chart"]',
    dateRangeFilter: '[data-testid="date-range-filter"]',
    userSearch: '[data-testid="user-search"]',

    expiryWarning: '[data-testid="expiry-warning"]',
  },

  /**
   * Registration Page Selectors
   */
  registration: {
    card: '[data-testid="register-card"]',
    title: '[data-testid="register-title"]',
    emailInput: '[data-testid="register-email-input"]',
    passwordInput: '[data-testid="register-password-input"]',
    confirmPasswordInput: '[data-testid="register-confirm-password-input"]',
    nicknameInput: '[data-testid="register-nickname-input"]',
    registerButton: '[data-testid="register-submit-button"]',
    turnstileContainer: '.turnstile-widget-container',
  },

  /**
   * Platform Self-Service Signup Page Selectors
   *
   * Admin-realm-hosted public self-service realm signup entry (DEC-realm-create-001).
   * Frontend testids live in:
   *   frontend/src/routes/$realmId/auth/signup.tsx
   *   frontend/src/components/auth/signup-form.tsx
   *
   * These mirror the `registration` block (data-testid-based stable selectors)
   * rather than role/label selectors: the signup labels are i18n-derived and the
   * disabled-notice / card states are asserted across locales.
   */
  platformSignup: {
    card: '[data-testid="signup-card"]',
    title: '[data-testid="signup-title"]',
    subtitle: '[data-testid="signup-subtitle"]',
    realmNameInput: '[data-testid="signup-realm-name-input"]',
    realmSlugInput: '[data-testid="signup-realm-slug-input"]',
    emailInput: '[data-testid="signup-email-input"]',
    passwordInput: '[data-testid="signup-password-input"]',
    submitButton: '[data-testid="signup-submit-button"]',
    // Fail-closed notice rendered when the toggle is off or the status query
    // fails (DEC-realm-create-013). Asserted in US-SR-004 (disabled) scenarios.
    disabledNotice: '[data-testid="signup-disabled-notice"]',
    loginLink: '[data-testid="login-link"]',
  },

  /**
   * Unified Purchase - Purchase Points Page (User)
   */
  purchasePoints: {
    page: '[data-testid="purchase-points-page"]',
    stepIndicator: '[data-testid="purchase-step-indicator"]',
    backButton: '[data-testid="purchase-back-button"]',
    nextButton: '[data-testid="purchase-next-button"]',
    stepPackages: '[data-testid="purchase-step-packages"]',
    stepPayment: '[data-testid="purchase-step-payment"]',
    stepProcessing: '[data-testid="purchase-step-processing"]',
    stepComplete: '[data-testid="purchase-step-complete"]',
  },

  paymentMethodSelector: {
    container: '[data-testid="payment-method-selector"]',
    button: (platform: string) =>
      `[data-testid="payment-method-button-${platform}"]`,
    select: (platform: string) =>
      `[data-testid="payment-method-select-${platform}"]`,
    selected: (platform: string) =>
      `[data-testid="payment-method-selected-${platform}"]`,
  },

  paymentStatus: {
    container: '[data-testid="payment-status-display"]',
    pending: '[data-testid="payment-status-pending"]',
    requiresAction: '[data-testid="payment-status-requires-action"]',
    succeeded: '[data-testid="payment-status-succeeded"]',
    failed: '[data-testid="payment-status-failed"]',
    cancelled: '[data-testid="payment-status-cancelled"]',
    expired: '[data-testid="payment-status-expired"]',
    countdownTimer: '[data-testid="payment-countdown-timer"]',
    retryButton: '[data-testid="payment-retry-button"]',
    cancelButton: '[data-testid="payment-cancel-button"]',
  },

  paymentProviderUI: {
    redirectPrompt: '[data-testid="payment-redirect-prompt"]',
    redirectManualLink: '[data-testid="payment-redirect-manual-link"]',
    contextDegraded: '[data-testid="payment-context-degraded"]',
    cancelButton: '[data-testid="payment-cancel-button"]',
    countdownTimer: '[data-testid="payment-countdown-timer"]',
  },

  /**
   * WeChat Pay Native (QR) pending payment
   * (frontend/src/components/purchase/WechatQrCodePayment.tsx)
   */
  wechatQrPayment: {
    container: '[data-testid="wechat-qr-payment"]',
    code: '[data-testid="wechat-qr-code"]',
    countdown: '[data-testid="wechat-qr-countdown"]',
  },

  /**
   * WeChat Pay JSAPI (in-WeChat) pending payment
   * (frontend/src/components/purchase/payment-attempt-status.tsx)
   */
  wechatJsapiPayment: {
    pending: '[data-testid="wechat-jsapi-pending"]',
    invokeButton: '[data-testid="wechat-jsapi-invoke-button"]',
    // fail and bridge_unavailable share the same destructive feedback region
    resultFail: '[data-testid="wechat-jsapi-result-fail"]',
  },

  /**
   * WeChat Pay provider configuration form
   * (frontend/src/components/billing/WechatConfigForm.tsx). Fields used only
   * by the page object (`fillWechatForm`) are inlined there, matching the
   * Apple/Google form convention.
   */
  wechatConfigForm: {
    page: '[data-testid="wechat-config-form-page"]',
    appIdInput: '[data-testid="wechat-app-id-input"]',
    privateKeyInput: '[data-testid="wechat-private-key-input"]',
    v3KeyInput: '[data-testid="wechat-v3-key-input"]',
    notifyUrlInput: '[data-testid="wechat-notify-url-input"]',
  },

  /**
   * Email Configuration Selectors (Settings > Email tab)
   */
  emailConfig: {
    emailTab: '[data-testid="email-tab"]',
    statusBadge: '[data-testid="email-config-status-badge"]',
    statusError: '[data-testid="email-status-error"]',
    providerResend: '[data-testid="email-provider-resend"]',
    providerSmtp: '[data-testid="email-provider-smtp"]',
    fromAddressInput: '[data-testid="email-from-address-input"]',
    resendApiKeyInput: '[data-testid="email-resend-api-key-input"]',
    smtpHostInput: '[data-testid="email-smtp-host-input"]',
    smtpPortInput: '[data-testid="email-smtp-port-input"]',
    smtpEncryptionSelect: '[data-testid="email-smtp-encryption-select"]',
    smtpUsernameInput: '[data-testid="email-smtp-username-input"]',
    smtpPasswordInput: '[data-testid="email-smtp-password-input"]',
    testRecipientInput: '[data-testid="email-test-recipient-input"]',
    testButton: '[data-testid="email-test-button"]',
    testError: '[data-testid="email-test-error"]',
    testSuccess: '[data-testid="email-test-success"]',
    saveButton: '[data-testid="email-save-button"]',
    saveError: '[data-testid="email-save-error"]',
  },

  /**
   * White-label Configuration Selectors (Settings > White-label tab)
   *
   * Anchors calibrated against:
   * - frontend/src/routes/$realmId/manage/settings.tsx (`white-label-tab`)
   * - frontend/src/components/realm-config/white-label-config-form.tsx (all others)
   *
   * The `white-label-background-value` Textarea ONLY renders when the background
   * type select is set to `image` or `gradient` (NOT `none`).
   *
   * User stories: US-WL-001/002/003/004
   */
  whiteLabel: {
    tab: '[data-testid="white-label-tab"]',
    // Form fields
    logoUrlInput: '[data-testid="white-label-logo-url"]',
    accentColorPicker: '[data-testid="white-label-accent-color-picker"]',
    accentColorInput: '[data-testid="white-label-accent-color"]',
    accentWarning: '[data-testid="white-label-accent-warning"]',
    backgroundTypeSelect: '[data-testid="white-label-background-type"]',
    backgroundValueTextarea: '[data-testid="white-label-background-value"]',
    footerTextInput: '[data-testid="white-label-footer-text"]',
    loginTitleInput: '[data-testid="white-label-login-title"]',
    loginSubtitleInput: '[data-testid="white-label-login-subtitle"]',
    registerTitleInput: '[data-testid="white-label-register-title"]',
    registerSubtitleInput: '[data-testid="white-label-register-subtitle"]',
    draftNotice: '[data-testid="white-label-draft-notice"]',
    // Action buttons (idle vs in-flight text differs)
    saveDraftButton: '[data-testid="white-label-save-draft"]',
    publishButton: '[data-testid="white-label-publish"]',
    discardDraftButton: '[data-testid="white-label-discard-draft"]',
    restoreButton: '[data-testid="white-label-restore"]',
    restoreDialog: '[data-testid="white-label-restore-dialog"]',
    restoreConfirmButton: '[data-testid="white-label-restore-confirm"]',
    // In-form preview panels (AuthPageWrapper in login/register variant)
    previewLoginTab: '[data-testid="white-label-preview-login"]',
    previewRegisterTab: '[data-testid="white-label-preview-register"]',
    previewLoginPanel: '[data-testid="white-label-preview-login-panel"]',
    previewRegisterPanel: '[data-testid="white-label-preview-register-panel"]',
  },

  /**
   * Realm settings page container
   * (frontend/src/routes/$realmId/manage/settings.tsx `settings-page`).
   */
  settingsPage: {
    container: '[data-testid="settings-page"]',
  },

  /**
   * Custom-domain Configuration Selectors (Settings > Custom-domain tab)
   *
   * Anchors calibrated against:
   * - frontend/src/routes/$realmId/manage/settings.tsx (`custom-domain-tab`)
   * - frontend/src/components/realm-config/custom-domain-config-form.tsx (all form testids)
   *
   * The custom-domain tab surfaces the single-state config (one Save writes the
   * hostname + host→realm mapping) for US-CD-001 scenarios 1/2/3. Host→realm
   * routing (US-CD-002/004, host-resolution part of US-CD-005) was reverted
   * 2026-07-09 and is NOT covered by these selectors.
   *
   * User stories: US-CD-001 (config save), US-CD-003 (authorize gate)
   */
  customDomain: {
    tab: '[data-testid="custom-domain-tab"]',
    hostnameInput: '[data-testid="custom-domain-hostname"]',
    // CNAME guidance panel (renders configured cname_target)
    cnameGuidance: '[data-testid="custom-domain-cname-guidance"]',
    statusCname: '[data-testid="custom-domain-status-cname"]',
    statusTls: '[data-testid="custom-domain-status-tls"]',
    refreshStatusButton: '[data-testid="custom-domain-refresh-status"]',
    // Action button (single Save: writes hostname + mapping in one step)
    saveButton: '[data-testid="custom-domain-save"]',
  },

  /**
   * Auth page wrapper brand elements (end-user login/register pages)
   *
   * Anchors calibrated against frontend/src/components/auth/auth-page-wrapper.tsx.
   * `auth-brand-logo` renders only when logoUrl is present AND the image loaded;
   * on load failure it switches to `auth-brand-text` ("Herald" fallback).
   * The wrapper root carries inline `--primary`/`--ring` CSS vars when a valid
   * accent color is configured — assert accent via that style, NOT via class.
   */
  authBrand: {
    wrapper: '[data-testid="auth-brand-logo"], [data-testid="auth-brand-text"]',
    logo: '[data-testid="auth-brand-logo"]',
    text: '[data-testid="auth-brand-text"]',
    footer: '[data-testid="auth-brand-footer"]',
  },

  /**
   * Platform Self-Service Signup config (admin Settings > Platform tab)
   *
   * Mounted only for realmId === 'admin' (DEC-realm-create-001/009). The switch
   * testid is derived from ConfigSwitchField `id="platform-signup"` →
   * `${id}-switch` (see config-switch-field.tsx). Used to enable/disable the
   * public signup entry (fail-closed default false, DEC-013) before running the
   * public signup flow.
   */
  platformSignupConfig: {
    tab: '[data-testid="platform-signup-tab"]',
    switch: '[data-testid="platform-signup-switch"]',
    saveButton: '[data-testid="platform-signup-save-button"]',
  },

  /**
   * Points Usage Dashboard Selectors (rate-dashboard quota view)
   *
   * `points-window-resets-in-{bucketId}-{winKey}` is intentionally omitted;
   * read the resets-in copy from within `points-window-row-{bucketId}-{winKey}`.
   */
  pointsUsageDashboard: {
    page: '[data-testid="points-usage-dashboard"], [data-testid^="points-usage-dashboard-"]',
    spendableNow: '[data-testid="points-spendable-now"]',
    spendableFormula: '[data-testid="points-spendable-formula"]',
    windowRow: (bucketId: string, winKey: string) =>
      `[data-testid="points-window-row-${bucketId}-${winKey}"]`,
    windowBar: (bucketId: string, winKey: string) =>
      `[data-testid="points-window-bar-${bucketId}-${winKey}"]`,
    exhaustedAlert: '[data-testid="points-window-exhausted-alert"]',
    overspendTopupAlert: '[data-testid="points-overspend-topup-alert"]',
    insufficientAlert: '[data-testid="points-insufficient-alert"]',
    emptyState: '[data-testid="points-empty-state"]',
  },

  /**
   * Multi-Window Quota Editor Selectors
   *
   * Used by both entitlement-mapping (`prefix = 'quota-window'`) and realm
   * default free-periodic config (`prefix = 'realm-default-window'`).
   * Save buttons are owned by the hosting page.
   */
  pointsQuotaEditor: {
    editor: (prefix: string) => `[data-testid="${prefix}-editor"]`,
    impactAlert: (prefix: string) => `[data-testid="${prefix}-impact-alert"]`,
    emptyRow: (prefix: string) => `[data-testid="${prefix}-empty-row"]`,
    row: (prefix: string, n: number) => `[data-testid="${prefix}-row-${n}"]`,
    lengthRow: (prefix: string, n: number) =>
      `[data-testid="${prefix}-length-row-${n}"]`,
    unitRow: (prefix: string, n: number) =>
      `[data-testid="${prefix}-unit-row-${n}"]`,
    limitRow: (prefix: string, n: number) =>
      `[data-testid="${prefix}-limit-row-${n}"]`,
    deleteRow: (prefix: string, n: number) =>
      `[data-testid="${prefix}-delete-row-${n}"]`,
    addButton: (prefix: string) => `[data-testid="${prefix}-add-button"]`,
    windowCap: (prefix: string) => `[data-testid="${prefix}-window-cap"]`,
    saveMappingButton: '[data-testid="save-mapping-button"]',
    saveConfigButton: '[data-testid="save-config-button"]',
  },

  pointRule: {
    list: '[data-testid="point-rule-list"]',
    row: (ruleId: string) => `[data-testid="point-rule-${ruleId}"]`,
    firstRow: '[data-testid^="point-rule-"]',
    addButton: '[data-testid="point-rule-add"]',
    bucketSelect: '[data-testid="point-rule-bucket"]',
    modeSelect: '[data-testid="point-rule-mode"]',
    trigger: (trigger: string) => `[data-testid="point-rule-trigger-${trigger}"]`,
    amountInput: (ruleId: string) => `[data-testid="point-rule-amount-${ruleId}"]`,
    validityInput: (ruleId: string) => `[data-testid="point-rule-validity-${ruleId}"]`,
    periodSelect: (ruleId: string) => `[data-testid="point-rule-period-${ruleId}"]`,
    enabledSwitch: (ruleId: string) => `[data-testid="point-rule-enabled-${ruleId}"]`,
    registrationRulesSave: '[data-testid="registration-rules-save"]',
  },

  purchasePointRule: {
    row: (ruleId: string) => `[data-testid="purchase-point-rule-${ruleId}"]`,
  },

  /**
   * Device Verification Page Selectors
   */
  deviceVerification: {
    card: '[data-testid="device-verification-card"]',
    title: '[data-testid="device-verification-title"]',
    error: '[data-testid="device-verification-error"]',
    result: '[data-testid="device-verification-result"]',
    codeInput: '[data-testid="device-code-input"]',
    codeSubmit: '[data-testid="device-code-submit"]',
    authorizeButton: '[data-testid="device-authorize-button"]',
    denyButton: '[data-testid="device-deny-button"]',
  },

  /**
   * Credit Bucket Directory (Admin)
   *
   * LOUD NOTE — i18n-dependent sidebar entry:
   * The sidebar menu item testid `sidebar-menu-credit-buckets` is derived from
   * the localized label and differs per locale. Demo tests MUST navigate by route
   * (`/{realmId}/manage/billing/credit-buckets`), NOT by clicking the
   * locale-derived sidebar testid.
   *
   * User stories: US-CB-001 (admin CRUD), US-CB-002 (coverage set),
   * US-CB-003 (mapping→bucket assignment).
   */
  creditBucket: {
    // Directory page container + toolbar
    directoryPage: '[data-testid="credit-buckets-directory-page"]',
    searchInput: '[data-testid="credit-bucket-search-input"]',
    newButton: '[data-testid="credit-bucket-new-button"]',
    emptyNewButton: '[data-testid="credit-bucket-empty-new-button"]',
    listItem: (bucketId: string) =>
      `[data-testid="credit-bucket-list-item-${bucketId}"]`,
    listItemDisabledBadge: (bucketId: string) =>
      `[data-testid="credit-bucket-list-item-${bucketId}-disabled-badge"]`,
    emptyState: '[data-testid="credit-buckets-empty-state"]',
    noSelection: '[data-testid="credit-buckets-no-selection"]',
    editor: '[data-testid="credit-bucket-editor"]',
    editorName: '[data-testid="credit-bucket-editor-name"]',
    editorBucketKey: '[data-testid="credit-bucket-editor-bucket-key"]',
    editorDescription: '[data-testid="credit-bucket-editor-description"]',
    editorEnabled: '[data-testid="credit-bucket-editor-enabled"]',
    // Read-only distribution-rule references block (rendered only in edit mode).
    // Maps `credit-bucket-editor.tsx` `data-testid="credit-bucket-rule-references"`.
    editorRuleReferences: '[data-testid="credit-bucket-rule-references"]',
    editorSubmit: '[data-testid="credit-bucket-editor-submit"]',
    deleteButton: '[data-testid="credit-bucket-delete-button"]',
    deleteConfirmDialog: '[data-testid="delete-bucket-confirm-dialog"]',
    deleteErrorMessage: '[data-testid="delete-bucket-error-message"]',
    deleteConfirmButton: '[data-testid="delete-bucket-confirm-button"]',
    deleteCancelButton: '[data-testid="delete-bucket-cancel-button"]',
    overviewPage: '[data-testid="credit-bucket-overview-page"]',
    overviewToolbar: '[data-testid="credit-bucket-overview-toolbar"]',
    overviewTable: '[data-testid="credit-bucket-overview-table"]',
    overviewGrandTotal: '[data-testid="credit-bucket-overview-grandtotal"]',
    overviewGrandTotalByKey: (key: string) =>
      `[data-testid="credit-bucket-overview-grandtotal-${key}"]`,
    overviewColTotalByKey: (key: string) =>
      `[data-testid="credit-bucket-overview-col-total-${key}"]`,
    overviewRow: (bucketId: string) =>
      `[data-testid="credit-bucket-overview-row-${bucketId}"]`,
    overviewCell: (bucketId: string, key: string) =>
      `[data-testid="credit-bucket-overview-cell-${bucketId}-${key}"]`,
    overviewDisabled: (bucketId: string) =>
      `[data-testid="credit-bucket-overview-disabled-${bucketId}"]`,
    overviewRegistration: (bucketId: string) =>
      `[data-testid="credit-bucket-overview-registration-${bucketId}"]`,
    overviewDetail: (bucketId: string) =>
      `[data-testid="credit-bucket-overview-detail-${bucketId}"]`,
    overviewEmptyState: '[data-testid="credit-bucket-overview-empty-state"]',
    overviewEmptyCta: '[data-testid="credit-bucket-overview-empty-cta"]',
    coverageMultiselect: '[data-testid="bucket-coverage-multiselect"]',
    coverageMultiselectSearch: '[data-testid="bucket-coverage-multiselect-search"]',
    coverageMultiselectError: '[data-testid="bucket-coverage-multiselect-error"]',
    coverageMultiselectItem: (clientAppId: string) =>
      `[data-testid="bucket-coverage-multiselect-item-${clientAppId}"]`,
  },

  /**
   * Multi-Price Entitlement Mappings — master-detail page
   *
   * LOUD NOTE — price-edit-row testid fallback:
   * The price row testid is `price-edit-row-${externalPriceId ?? mappingId}`.
   * For Stripe rows (non-NULL external_price_id) the suffix is the price id; for
   * Creem rows (NULL external_price_id — price-less provider) the
   * suffix falls back to the mapping id. Test fixtures mixing Stripe + Creem
   * MUST account for this — see `multi-price-seed-ids.ts` for the seeded ids and
   * `priceEditRow(priceKey)` below (accepts either the price id or mapping id).
   *
   * LOUD NOTE — ProtectedPriceConfirmDialog:
   * The 409 dialog renders ONLY `protected-price-active-subs` +
   * `protected-price-confirm-cancel`. There is NO `protected-price-confirm-proceed`:
   * the active-subscription lock is enforced authoritatively by the backend 409
   * (the batch rolls back; the client offers no "force" path). Tests assert the
   * dialog surfaces the active-sub count, not a confirm action.
   */
  multiPriceMapping: {
    page: '[data-testid="entitlement-mappings-page"]',
    readonlyPermBanner: '[data-testid="readonly-perm-banner"]',
    webhookPriceUnresolvedBanner: '[data-testid="webhook-price-unresolved-banner"]',
    emptyState: '[data-testid="entitlement-mappings-empty-state"]',
    emptySyncButton: '[data-testid="empty-sync-button"]',
    mappingProductList: '[data-testid="mapping-product-list"]',
    mappingProductRow: (productId: string) =>
      `[data-testid="mapping-product-row-${productId}"]`,
    firstMappingProductRow: () => '[data-testid^="mapping-product-row-"]',
    mappingDetailPanel: '[data-testid="mapping-detail-panel"]',
    detailHead: '[data-testid="detail-head"]',
    sharedKeyChip: (entitlementKey: string) =>
      `[data-testid="shared-key-chip-${entitlementKey}"]`,
    // Per-price edit row. `priceKey` is external_price_id for Stripe, mapping id
    // for Creem (NULL price) — see the loud note above.
    priceEditRow: (priceKey: string) => `[data-testid="price-edit-row-${priceKey}"]`,
    priceMetadataBlock: (priceKey: string) =>
      `[data-testid="price-metadata-block-${priceKey}"]`,
    metadataEntry: (scope: 'product' | 'price', key: string) =>
      `[data-testid="metadata-entry-${scope}-${key}"]`,
    priceBillingType: (priceKey: string) =>
      `[data-testid="price-billing-type-${priceKey}"]`,
    priceEnabledToggle: (priceKey: string) =>
      `[data-testid="price-enabled-toggle-${priceKey}"]`,
    // Save (batch PUT). Rendered only when canManage.
    saveMappingButton: '[data-testid="save-mapping-button"]',
    // Provider sync. `provider-sync-button` is a wrapper `<div>`; the clickable
    // Button inside carries `sync-button`. Sync result spans appear post-sync.
    providerSyncButton: '[data-testid="provider-sync-button"]',
    syncButton: '[data-testid="sync-button"]',
    syncResultProducts: '[data-testid="sync-result-products"]',
    syncResultPrices: '[data-testid="sync-result-prices"]',
    // Protected-price 409 dialog (Cancel-only; no Proceed action — see loud note).
    protectedPriceConfirmDialog: '[data-testid="protected-price-confirm-dialog"]',
    protectedPriceActiveSubs: '[data-testid="protected-price-active-subs"]',
    protectedPriceConfirmCancel: '[data-testid="protected-price-confirm-cancel"]',
  },

  /**
   * Multi-Price Purchase — price-card grid (section IA, no period toggle)
   *
   * Section IA (purchase-entry-optimization — current frontend
   * `frontend/src/routes/$realmId/user/purchase-points.tsx`):
   * The page splits options into two sections by billing type, and there is NO
   * period toggle. Monthly + annual recurring options are shown together:
   * - **Subscriptions section** (`purchase-section-subscriptions`): ALL
   *   recurring options (both monthly and annual) rendered together under the
   *   single grid `purchase-price-grid-subscriptions`.
   * - **Credit packs section** (`purchase-section-credit-packs`): one_time
   *   options only, rendered under `purchase-price-grid-credit-packs`.
   *
   * Price-card testid is period-invariant: always the bare
   * `purchase-price-card-${priceId}` (NO `-annual` suffix). The same priceId
   * never appears in two grids; disambiguation is by the containing grid.
   * `priceId` is `externalPriceId ?? mappingId` (Creem NULL-price rows fall
   * back to mapping id).
   *
   * `priceGrid(period)` is retained for call-site compatibility but now points
   * at the single Subscriptions grid regardless of `period` (there is no
   * period-keyed grid anymore). New code should prefer `subscriptionsGrid`.
   */
  purchasePriceCard: {
    page: '[data-testid="purchase-points-page"]',
    noClientAppMessage: '[data-testid="no-client-app-message"]',
    /** Subscriptions-section grid (all recurring options, both periods). */
    subscriptionsGrid: '[data-testid="purchase-price-grid-subscriptions"]',
    /**
     * Subscriptions-section grid. The `period` argument is accepted for
     * call-site compatibility but is IGNORED — the current frontend renders a
     * single Subscriptions grid with all recurring options; there is no
     * period-keyed grid. Equivalent to `subscriptionsGrid`.
     */
    priceGrid: (_period: 'month' | 'year') =>
      '[data-testid="purchase-price-grid-subscriptions"]',
    /** Credit-packs-section grid (one_time options). */
    creditPacksGrid: '[data-testid="purchase-price-grid-credit-packs"]',
    /**
     * Price-card locator. `priceId` is `externalPriceId ?? mappingId`. The card
     * testid is period-invariant (no `-annual` suffix); select an annual
     * recurring card directly by its priceId — no period switch is required.
     */
    priceCard: (priceId: string) =>
      `[data-testid="purchase-price-card-${priceId}"]`,
    priceCardReason: (priceId: string) =>
      `[data-testid="purchase-price-card-${priceId}-reason"]`,
    emptyState: '[data-testid="purchase-empty-state"]',
    nextButton: '[data-testid="purchase-next-button"]',
    backButton: '[data-testid="purchase-back-button"]',
  },

  /**
   * Currency-grouped purchase blocks (multiple-currency feature).
   *
   * Anchors calibrated against
   * frontend/src/components/billing/currency-purchase-group.tsx:
   * - `entitlement(slug)` is one entitlement's block; `slug` is the kebab-case
   *   entitlement key.
   * - The switcher renders ONLY for switchable groups (all rows Stripe-priced
   *   AND ≥2 currencies). Store-priced or single-currency groups degrade to a
   *   flat list — assert the ABSENCE of `switch(slug)` for the degrade story.
   * - A switchable group renders NO price cards until the user explicitly
   *   picks a currency (no default), then renders ONLY the picked currency's
   *   price cards — asserted via visible price cards (persistent page state),
   *   not via transient styling.
   * - `option(slug, currency)` takes an uppercase ISO code; the testid is
   *   lowercase (DEC-multiple_currency-012 case normalization).
   *
   * User stories: US-MC-003 (grouping + explicit selection),
   * US-MC-004 (explicit-row checkout), US-MC-006 (degrade display).
   */
  purchaseCurrencyGroup: {
    entitlement: (slug: string) => `[data-testid="purchase-entitlement-${slug}"]`,
    switch: (slug: string) => `[data-testid="purchase-currency-switch-${slug}"]`,
    option: (slug: string, currency: string) =>
      `[data-testid="purchase-currency-option-${slug}-${currency.toLowerCase()}"]`,
    adaptivePricingNote: (slug: string) =>
      `[data-testid="purchase-adaptive-pricing-note-${slug}"]`,
  },

  /**
   * Stripe Checkout Button
   *
   * The testid is keyed by the entitlement MAPPING id (not entitlement_key) so
   * multiple prices under one shared key each resolve their own checkout target.
   */
  stripeCheckoutButton: (mappingId: string) =>
    `[data-testid="stripe-checkout-button-${mappingId}"]`,

  /**
   * Unified Purchase - Purchase History (User)
   */
  purchaseHistory: {
    page: '[data-testid="purchase-records-page"]',
    list: '[data-testid="purchase-history-list"]',
    loading: '[data-testid="purchase-history-loading"]',
    empty: '[data-testid="purchase-history-empty"]',
    error: '[data-testid="purchase-history-error"]',
    item: (id: string) => `[data-testid="purchase-history-item-${id}"]`,
    detailsButton: (id: string) =>
      `[data-testid="purchase-history-details-button-${id}"]`,
    filterProvider: '[data-testid="filter-provider"]',
    filterStatus: '[data-testid="filter-status"]',
    filterFromDate: '[data-testid="filter-from-date"]',
    filterToDate: '[data-testid="filter-to-date"]',
    resetFiltersButton: '[data-testid="reset-filters-button"]',
    applyFiltersButton: '[data-testid="apply-filters-button"]',
  },

  /**
   * Legal Consent & Account Deletion Selectors
   */
  legalConsent: {
    registerConsentCheckbox: '[data-testid="register-consent-checkbox"]',
    registerConsentError: '[data-testid="register-consent-error"]',
    termsOfServiceLink: '[data-testid="terms-of-service-link"]',
    privacyPolicyLink: '[data-testid="privacy-policy-link"]',

    agreementCard: '[data-testid="agreement-card"]',
    agreementTitle: '[data-testid="agreement-title"]',
    agreementVersion: '[data-testid="agreement-version"]',
    agreementEffectiveDate: '[data-testid="agreement-effective-date"]',
    agreementBody: '[data-testid="agreement-body"]',

    loginReconsentView: '[data-testid="login-reconsent-view"]',
    loginReconsentAgreement: (type: string) =>
      `[data-testid="login-reconsent-agreement-${type}"]`,
    loginReconsentAgreementVersion: (type: string) =>
      `[data-testid="login-reconsent-agreement-${type}-version"]`,
    loginAgreeAndContinueButton: '[data-testid="login-agree-and-continue-button"]',
    loginDeclineBackButton: '[data-testid="login-decline-back-button"]',
    loginConsentStatement: '[data-testid="login-consent-statement"]',

    reconsentDialogTitle: '[data-testid="reconsent-dialog-title"]',
    reconsentDialogDescription: '[data-testid="reconsent-dialog-description"]',
    reconsentAgreement: (type: string) => `[data-testid="reconsent-agreement-${type}"]`,
    reconsentAgreeButton: '[data-testid="reconsent-agree-button"]',
    reconsentLogoutButton: '[data-testid="reconsent-logout-button"]',

    legalTab: '[data-testid="legal-tab"]',
    legalAgreementsTab: '[data-testid="legal-agreements-tab"]',
    legalAgreementCard: (type: string) => `[data-testid="legal-agreement-card-${type}"]`,
    legalAgreementTitle: (type: string) => `[data-testid="legal-agreement-title-${type}"]`,
    legalAgreementMeta: (type: string) => `[data-testid="legal-agreement-meta-${type}"]`,
    sourceBadge: (source: 'default' | 'custom') => `[data-testid="source-badge-${source}"]`,
    legalVersionLabelInput: (type: string) =>
      `[data-testid="legal-version-label-input-${type}"]`,
    legalContentEnInput: (type: string) =>
      `[data-testid="legal-content-en-input-${type}"]`,
    legalModeSelect: (type: string) => `[data-testid="legal-mode-select-${type}"]`,
    legalModeBadge: (mode: string) => `[data-testid="mode-badge-${mode}"]`,
    legalExternalUrlInput: (type: string) =>
      `[data-testid="legal-external-url-input-${type}"]`,
    legalPublishButton: (type: string) => `[data-testid="legal-publish-button-${type}"]`,
    legalSaveDraftButton: (type: string) => `[data-testid="legal-save-draft-button-${type}"]`,
    legalPreviewButton: (type: string) => `[data-testid="legal-preview-button-${type}"]`,
    legalPreviewDialog: (type: string) => `[data-testid="legal-preview-dialog-${type}"]`,
    legalDiscardDraftButton: (type: string) =>
      `[data-testid="legal-discard-draft-button-${type}"]`,
    legalDiscardDraftConfirmButton: (type: string) =>
      `[data-testid="legal-discard-draft-confirm-${type}"]`,
    legalRevertButton: (type: string) => `[data-testid="legal-revert-button-${type}"]`,
    legalRevertConfirmButton: (type: string) =>
      `[data-testid="legal-revert-confirm-${type}"]`,
    legalHistoryTable: (type: string) => `[data-testid="legal-history-table-${type}"]`,
    legalHistoryRow: (type: string, versionId: string) =>
      `[data-testid="legal-history-row-${type}-${versionId}"]`,

    deleteAccountOpenButton: '[data-testid="delete-account-open-button"]',
    deleteAccountDialog: '[data-testid="delete-account-dialog"]',
    deleteAccountDialogTitle: '[data-testid="delete-account-dialog-title"]',
    deleteAccountPasswordInput: '[data-testid="delete-account-password-input"]',
    deleteAccountSubmitButton: '[data-testid="delete-account-submit-button"]',
    deleteAccountCancelButton: '[data-testid="delete-account-cancel-button"]',
    deleteAccountErrorAlert: '[data-testid="delete-account-error-alert"]',
    deleteAccountErrorMessage: '[data-testid="delete-account-error-message"]',
  },

  /**
   * Email-OTP Login Selectors
   *
   * SHIPPED `data-testid` values (frontend phase is `completed`). Verified
   * against:
   * - `frontend/src/routes/$realmId/auth/login.tsx` (`email-otp-toggle` — the
   *   login-route OTP entry toggle; note it is `email-otp-toggle`, NOT
   *   `email-otp-login-toggle` which does not exist)
   * - `frontend/src/components/auth/email-otp-login-form.tsx` (every user-side
   *   form testid below)
   * - `frontend/src/routes/$realmId/manage/settings.tsx` (Email-OTP controls
   *   are NOT a standalone tab — since commit 364767b2 "merged email-otp
   *   settings" they live inside the `email` tab as a
   *   `data-testid="email-otp-section"` sub-block of EmailConfigForm)
   * - `frontend/src/components/realm-config/email-config-form.tsx` + shared
   *   `frontend/src/components/realm-config/config-switch-field.tsx`
   *   (`config-switch-field` emits `data-testid={`${id}-switch`}`, yielding
   *   `email-otp-enabled-switch` / `email-otp-auto-register-switch`)
   *
   * The per-digit code input testids are `email-otp-code-digit-0` …
   * `email-otp-code-digit-5` (6 digits). There is NO consent checkbox —
   * consent for auto-register is expressed by clicking
   * `email-otp-agree-and-continue-button`.
   *
   * User stories: US-EO-001 / US-EO-002 (login flow), US-EO-003 (admin config).
   */
  emailOtp: {
    // --- Login route (end-user) ----------------------------------------------
    // Toggle on the password login route that switches into the OTP form.
    loginRouteToggle: '[data-testid="email-otp-toggle"]',

    // --- OTP login form (user side) ------------------------------------------
    form: '[data-testid="email-otp-login-form"]',
    emailInput: '[data-testid="email-otp-email-input"]',
    sendButton: '[data-testid="email-otp-send-btn"]',
    // Wrapper around the 6 `<input>` digits; per-digit inputs resolved below.
    codeInput: '[data-testid="email-otp-code-input"]',
    codeDigit: (index: number) => `[data-testid="email-otp-code-digit-${index}"]`,
    verifyButton: '[data-testid="email-otp-verify-btn"]',
    resendButton: '[data-testid="email-otp-resend-btn"]',
    resendCountdown: '[data-testid="email-otp-resend-countdown"]',
    // "use password instead" — back to password login (email step).
    backButton: '[data-testid="email-otp-back-button"]',
    // Back to email step from the code step.
    backToEmailButton: '[data-testid="email-otp-back-to-email-button"]',
    // Consent gate (auto-register): NO checkbox — consent is expressed by
    // clicking this button.
    agreeAndContinueButton: '[data-testid="email-otp-agree-and-continue-button"]',
    agreementBackButton: '[data-testid="email-otp-agreement-back-button"]',
    errorMessage: '[data-testid="email-otp-error-message"]',
    // 409 email_not_registered guidance (auto-register off).
    notRegisteredMessage: '[data-testid="email-otp-not-registered-message"]',
    registerLink: '[data-testid="email-otp-register-link"]',
    backAfterConflictButton: '[data-testid="email-otp-back-after-conflict-button"]',

    // --- Admin Settings → Email-OTP section (inside the `email` tab) ---------
    // Since frontend commit 364767b2 ("merged email-otp settings"), Email-OTP is
    // NOT a standalone tab. The two config switches + save button render inside
    // the `email` tab as a `data-testid="email-otp-section"` sub-block of
    // EmailConfigForm. `tab` is retained only for selector discoverability;
    // navigation to the OTP controls must go through `email-tab` (see
    // SettingsPage.switchToEmailOtpTab).
    tab: '[data-testid="email-otp-section"]',
    enabledSwitch: '[data-testid="email-otp-enabled-switch"]',
    autoRegisterSwitch: '[data-testid="email-otp-auto-register-switch"]',
    saveButton: '[data-testid="email-otp-save-button"]',
  },

  /**
   * IAP (App Store / Google Play) — provider configuration + create-mapping page
   *
   * Anchors calibrated against shipped source (all `data-testid` verified in the
   * DE-D01 session):
   * - frontend/src/components/billing/payment-providers-page.tsx (page shell,
   *   provider rows, add/edit/delete buttons)
   * - frontend/src/components/billing/DeleteConfirmDialog.tsx (delete dialog)
   * - frontend/src/components/billing/AppleIapConfigForm.tsx (Apple config form)
   * - frontend/src/components/billing/GooglePlayConfigForm.tsx (Google config form)
   * - frontend/src/components/billing/create-entitlement-mapping-page.tsx
   *   (create-mapping page — consumed by DE-D02)
   *
   * LOUD NOTE — Google form inputs ship canonical testids:
   * `google-package-name-input` and `google-service-account-json-input` are
   * present on the shipped GooglePlayConfigForm. Use them directly; NO
   * label-based semantic fallback is required. If a testid is missing at
   * execution time, fall back to the label→ancestor-field pattern used by
   * the readonly-field helpers in `entitlement-mappings-page.ts`
   * (`page.locator('label', { hasText })
   *   .locator('xpath=ancestor::div[starts-with(@class,"space-y-1")][1]')
   *   .locator('input')`) and record the gap in the DE-D01 Handoff.
   *
   * User stories: US-IAP-001 (provider config), US-IAP-002 (create mapping —
   * DE-D02 consumes the create-mapping page selectors below).
   * @see docs/user-stories/billing/support-iap.md
   */
  iap: {
    // --- Payment providers page shell (payment-providers-page.tsx) -----------
    paymentProvidersPage: '[data-testid="payment-providers-page"]',
    providerList: '[data-testid="provider-list"]',
    appleProviderRow: '[data-testid="apple-provider-row"]',
    googleProviderRow: '[data-testid="google-provider-row"]',
    wechatProviderRow: '[data-testid="wechat-provider-row"]',
    editAppleButton: '[data-testid="edit-apple-button"]',
    editGoogleButton: '[data-testid="edit-google-button"]',
    editWechatButton: '[data-testid="edit-wechat-button"]',
    deleteAppleButton: '[data-testid="delete-apple-button"]',
    deleteGoogleButton: '[data-testid="delete-google-button"]',
    // `add-${type}-button` is rendered per unconfigured provider type.
    addButton: (type: 'apple' | 'google') => `[data-testid="add-${type}-button"]`,

    // --- Delete confirm dialog (DeleteConfirmDialog.tsx) ---------------------
    // Note: when the provider protects active subscriptions, `delete-confirm-button`
    // is NOT rendered (the dialog surfaces the active-sub count + a Cancel-only
    // footer). Tests asserting the delete path assume an empty subscription set.
    deleteConfirmDialog: '[data-testid="delete-confirm-dialog"]',
    deleteConfirmButton: '[data-testid="delete-confirm-button"]',
    deleteCancelButton: '[data-testid="delete-cancel-button"]',

    // --- Apple config form (AppleIapConfigForm.tsx) --------------------------
    appleConfigPage: '[data-testid="apple-config-form-page"]',
    appleConfigHeading: '[data-testid="apple-config-form-page-heading"]',
    appleConfigForm: '[data-testid="apple-config-page-form"]',
    appleBundleIdInput: '[data-testid="apple-bundle-id-input"]',
    appleIssuerIdInput: '[data-testid="apple-issuer-id-input"]',
    appleKeyIdInput: '[data-testid="apple-key-id-input"]',
    applePrivateKeyP8Input: '[data-testid="apple-private-key-p8-input"]',
    appleEnvironmentSelect: '[data-testid="apple-environment-select"]',
    appleEnvironmentSelectTrigger: '[data-testid="apple-environment-select-trigger"]',
    appleConfigSubmitButton: '[data-testid="apple-config-page-submit-button"]',
    appleConfigCancelButton: '[data-testid="apple-config-page-cancel-button"]',

    // --- Google config form (GooglePlayConfigForm.tsx) -----------------------
    googleConfigPage: '[data-testid="google-config-form-page"]',
    googleConfigHeading: '[data-testid="google-config-form-page-heading"]',
    googleConfigForm: '[data-testid="google-config-page-form"]',
    googlePackageNameInput: '[data-testid="google-package-name-input"]',
    googleServiceAccountJsonInput: '[data-testid="google-service-account-json-input"]',
    googleConfigSubmitButton: '[data-testid="google-config-page-submit-button"]',
    googleConfigCancelButton: '[data-testid="google-config-page-cancel-button"]',

    // --- Create-mapping page (create-entitlement-mapping-page.tsx) ----------
    // The page is reached from the entitlement-mappings list via
    // `create-mapping-button` (navigates to /manage/billing/entitlement-mappings/new).
    // Billing period is conditionally rendered for a
    // recurring mapping; points fields use the shared pointRule selectors.
    createMappingPage: '[data-testid="create-entitlement-mapping-page"]',
    createMappingButton: '[data-testid="create-mapping-button"]',
    createMappingProviderSelect: '[data-testid="create-mapping-provider-select"]',
    createMappingBillingTypeSelect: '[data-testid="create-mapping-billing-type-select"]',
    createMappingBillingPeriodSelect: '[data-testid="create-mapping-billing-period-select"]',
    createMappingExternalProductIdInput:
      '[data-testid="create-mapping-external-product-id-input"]',
    createMappingExternalPriceIdInput: '[data-testid="create-mapping-external-price-id-input"]',
    createMappingEntitlementKeyInput: '[data-testid="create-mapping-entitlement-key-input"]',
    createMappingGrantedRoles: '[data-testid="create-mapping-granted-roles"]',
    createMappingSubmitError: '[data-testid="create-mapping-submit-error"]',
    createMappingSubmitButton: '[data-testid="create-mapping-submit-button"]',
    createMappingCancelButton: '[data-testid="create-mapping-cancel-button"]',
  },
};

/**
 * Selector helper for multiple fallback selectors
 *
 * @example
 * const button = page.locator(getSelector(SELECTORS.common.dialogSubmitButton))
 */
export function getSelector(selector: string | string[]): string {
  if (Array.isArray(selector)) {
    return selector.join(", ");
  }
  return selector;
}
