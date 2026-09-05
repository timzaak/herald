-- ====================================
-- Herald Legal Consent Schema (baseline)
-- ====================================
-- Versioned legal agreements (Terms of Service / Privacy Policy) with a
-- platform-default English template seed, and per-user current consent state.
--
-- Pre-launch squash: the platform-default templates are seeded directly in
-- English (the obsolete zh-CN default + later UPDATE-to-English are folded
-- into a single English INSERT). No ALTER/DROP.
--
-- Notes:
-- - legal_agreement_version is append-only: `realm_id IS NULL` = platform default
--   template; non-NULL = per-realm custom override. `version_no` is monotonic
--   within (scope, agreement_type).
-- - The expression unique index folds NULL realm_id into '' via COALESCE so that
--   platform-default rows participate in uniqueness and concurrent publish is
--   guarded (BE-D03).
-- - user_agreement_consent holds current consent state only (history is in
--   audit_events). consented_version_id has a hard FK; user_id -> account(id) is
--   a soft reference (no FK) so deletion keeps the account skeleton (no cascade).
-- - No updated_at on either table (append-only / current-state).
-- - account.deleted_original_email_hash is created with the account table in
--   0001_core.sql, not here.

-- (a) legal_agreement_version: append-only version + history for legal agreements.
-- Pre-launch squash: the link-mode columns (mode/external_url) and their CHECK
-- constraints (former 0009_legal_agreement_link_mode) are inlined. No ALTER/DROP.
CREATE TABLE legal_agreement_version (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    realm_id TEXT,
    agreement_type TEXT NOT NULL,
    version_no INTEGER NOT NULL,
    version_label TEXT,
    content JSONB NOT NULL,
    mode TEXT NOT NULL DEFAULT 'full_text',
    external_url TEXT,
    source TEXT NOT NULL DEFAULT 'custom',
    published_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    published_by TEXT,
    CONSTRAINT legal_agreement_version_mode_chk CHECK (mode IN ('full_text', 'link')),
    CONSTRAINT legal_agreement_version_mode_url_chk CHECK (mode = 'full_text' OR external_url IS NOT NULL)
);

-- Expression unique index: folds NULL realm_id into '' so the platform-default
-- template rows (realm_id IS NULL) also participate in (scope, type, version_no)
-- uniqueness. Guards concurrent publish (BE-D03) by enforcing one version_no per
-- (scope, agreement_type).
CREATE UNIQUE INDEX legal_agreement_version_scope_type_version_unique
    ON legal_agreement_version ((COALESCE(realm_id, '')), agreement_type, version_no);

-- Effective-resolution / history read path for a specific realm.
CREATE INDEX legal_agreement_version_realm_type_version_idx
    ON legal_agreement_version (realm_id, agreement_type, version_no DESC);

-- Platform-default template resolution (realm_id IS NULL rows).
CREATE INDEX legal_agreement_version_default_type_version_idx
    ON legal_agreement_version ((realm_id IS NULL), agreement_type, version_no DESC);

COMMENT ON TABLE legal_agreement_version IS 'Append-only versioned legal agreements (terms_of_service / privacy_policy); realm_id IS NULL = platform default template';
COMMENT ON COLUMN legal_agreement_version.realm_id IS 'NULL = platform default template; non-NULL = per-realm custom override';
COMMENT ON COLUMN legal_agreement_version.agreement_type IS 'terms_of_service | privacy_policy';
COMMENT ON COLUMN legal_agreement_version.version_no IS 'Monotonic within (scope, agreement_type); used as effective-resolution tiebreaker';
COMMENT ON COLUMN legal_agreement_version.content IS 'JSONB { [locale]: body } — at least the default locale body';
COMMENT ON COLUMN legal_agreement_version.mode IS 'Agreement content mode: full_text or link';
COMMENT ON COLUMN legal_agreement_version.external_url IS 'External agreement URL when mode is link';
COMMENT ON COLUMN legal_agreement_version.source IS 'default | custom (a realm-scoped default row is a live-default follow marker)';
COMMENT ON COLUMN legal_agreement_version.published_by IS 'Publishing user_id; platform default seed = system';

-- (b) user_agreement_consent: per-user current consent state (one row per user/type).
CREATE TABLE user_agreement_consent (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    user_id UUID NOT NULL,
    realm_id TEXT NOT NULL,
    agreement_type TEXT NOT NULL,
    consented_version_id UUID NOT NULL REFERENCES legal_agreement_version(id) ON DELETE RESTRICT,
    consented_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- One current-consent row per (user, agreement_type); upsert refreshes on re-consent.
CREATE UNIQUE INDEX user_agreement_consent_user_type_unique
    ON user_agreement_consent (user_id, agreement_type);

-- Consent-gate read path.
CREATE INDEX user_agreement_consent_user_idx
    ON user_agreement_consent (user_id);

COMMENT ON TABLE user_agreement_consent IS 'Per-user current consent state per agreement type (history lives in audit_events)';
COMMENT ON COLUMN user_agreement_consent.user_id IS 'account.id — soft reference (no FK); account skeleton retained after deletion';
COMMENT ON COLUMN user_agreement_consent.consented_version_id IS 'legal_agreement_version.id the user consented to; gate compares against current effective version id';

-- ====================================
-- Seed: platform default templates (English)
-- ====================================
-- Seeds the platform default (realm_id IS NULL) version_no=1 rows for both
-- agreement types (draft, pending legal review). source='default',
-- published_by='system'. Body text is dollar-quoted ($tos$ / $pp$) to avoid
-- single-quote / backslash escaping issues; the JSONB content is built as
-- { "en": <body> }. Idempotent guard: only insert if no platform-default row
-- exists yet.

INSERT INTO legal_agreement_version (realm_id, agreement_type, version_no, version_label, content, source, published_by)
SELECT
    NULL,
    'terms_of_service',
    1,
    NULL,
    jsonb_build_object('en', $tos$# Herald Terms of Service

> **Draft date**: 2026-06-29
>
> **Status**: Platform default template (draft, pending legal review)

---

## Important notice

Please read these Terms of Service ("Terms") and the accompanying Privacy Policy carefully before using Herald. By checking "I agree", clicking "Register", or otherwise using Herald, you acknowledge that you have read, understood, and agree to be bound by these Terms.

If you do not agree to any part of these Terms, please stop registering or using Herald. These Terms also apply when an updated version requires renewed consent.

---

## Table of contents

1. Definitions
2. Service description
3. Account registration and security
4. Acceptable use
5. Fees and payments
6. Refunds and termination
7. Account deletion
8. Intellectual property
9. Disclaimers and liability
10. Changes, suspension, and termination of service
11. Confidentiality
12. Governing law and dispute resolution
13. Changes to these Terms
14. Contact information

---

## 1. Definitions

1.1 **Herald / Platform**: The multi-tenant authentication and authorization SaaS platform operated by **[Operator name]** ("we", "us", or "Operator").

1.2 **Operator**: The legal entity that provides Herald services and acts as the data controller. Contact details are in Section 14.

1.3 **Realm**: An isolated tenant unit in Herald. A Realm usually corresponds to a customer organization and contains its own users, roles, client applications, and configuration.

1.4 **User / You**: Any natural person who registers for and uses Herald, including regular users and realm administrators.

1.5 **Third-party application**: A client application or system that connects to Herald via SDK, OAuth/OIDC, or API key.

1.6 **Points**: Virtual entitlement units issued by Herald. Points can only be consumed within the platform or authorized third-party applications and have no cash value.

1.7 **Subscription**: A recurring service entitlement purchased through a payment provider.

1.8 **One-time purchase**: A non-recurring entitlement purchased through a payment provider, such as a points package.

1.9 **Payment provider**: A third-party service that processes payments, such as Stripe or Creem.

1.10 **Personal information**: Information that can identify a natural person, such as email, nickname, password, or OAuth identifiers.

---

## 2. Service description

2.1 **Scope**. Herald provides identity and access management capabilities, including email/password login, OAuth login, TOTP, user/role/permission management, subscription and entitlement management, points, invoices, audit logs, and multi-language UI.

2.2 **Multi-tenancy**. Herald is organized by Realm. The specific services, pricing, points rules, and invoice policies available to you are determined by your Realm administrator. If your Realm's rules conflict with these Terms, your Realm's explicit rules apply for that Realm; matters not covered by your Realm's rules remain governed by these Terms.

2.3 **Delivery**. We provide services through the web interface, admin console, and public APIs. These Terms do not grant you access to internal APIs, source code, or infrastructure.

2.4 **Improvements**. We may update, enhance, or adjust features and interfaces. Material changes will be notified as described in Section 13.

---

## 3. Account registration and security

3.1 **Eligibility**. You must be a natural person with legal capacity to contract. If you are below the age of majority in your jurisdiction, you may use Herald only with the involvement and consent of a parent or guardian.

3.2 **Accuracy**. You must provide accurate, complete, and current registration information and keep it up to date. Registered email addresses cannot be changed.

3.3 **Account security**. You are responsible for maintaining the confidentiality of your credentials and for all activities under your account. You should use a strong password, enable TOTP where available, and not share credentials.

3.4 **Security incidents**. If you suspect unauthorized access, notify your Realm administrator or us immediately. We may take protective steps such as freezing the account or terminating sessions.

3.5 **Account status**. Accounts may have different statuses (e.g., pending verification, active, disabled). Disabled accounts cannot log in.

---

## 4. Acceptable use

When using Herald, you agree not to:

- Harm minors or violate anyone's rights.
- Impersonate any person or provide false information.
- Infringe intellectual property, privacy, or other legal rights.
- Probe, scan, stress-test, or attempt to circumvent Herald's security.
- Reverse engineer, decompile, or disassemble any part of the service.
- Upload or transmit unlawful, harmful, abusive, defamatory, obscene, or violent content.
- Send unsolicited commercial messages, phish, commit fraud, or launder money.
- Abuse points, payments, or refunds, including exploiting bugs or cashing out.
- Interfere with service operation or harm other users or third-party applications.
- Violate applicable laws.

---

## 5. Fees and payments

5.1 **Fees**. Herald may include free and paid services. Specific products, prices, billing cycles, and points rules are configured by your Realm and displayed at purchase.

5.2 **Payment providers**. Payments are processed by third-party payment providers. We do not store full payment card or bank account details locally.

5.3 **Subscriptions**. Subscriptions renew automatically through the payment provider. Failed renewals may suspend entitlements. Upgrades and downgrades follow the rules shown at purchase.

5.4 **One-time purchases**. One-time purchases are granted immediately and do not create a subscription or automatic renewal.

5.5 **Taxes**. Prices may be inclusive or exclusive of taxes as indicated at purchase. When a payment provider acts as merchant of record, its tax rules apply.

5.6 **Invoices**. Invoice eligibility and issuance are governed by your Realm's configuration.

---

## 6. Refunds and termination

6.1 **One-time purchases**. One-time purchases, including points packages, are non-refundable unless required by law.

6.2 **Subscription refunds**. Subscription refunds are handled by the payment provider according to its policies. We may reverse entitlements based on refund results.

6.3 **Account deletion**. When you delete your account, active subscriptions are canceled, normally at the end of the current billing period. Refund treatment depends on the payment provider and applicable law.

6.4 **Post-termination data**. After account deletion or service termination, we process your personal information as described in the Privacy Policy and Section 7.

---

## 7. Account deletion

7.1 **How to delete**. You may request account deletion through the self-service interface. Deletion is irreversible.

7.2 **Soft delete**. To balance the right to be forgotten with compliance and data integrity, Herald uses soft deletion:

- The account enters a deleted state and cannot log in.
- Personal information is anonymized or irreversibly masked.
- Active sessions are terminated and OAuth bindings are removed.
- TOTP is cleared.
- An account skeleton and minimal compliance data may be retained.
- Active subscriptions are canceled as described in Section 6.3.

7.3 **Legal retention**. Some data may be retained as required by applicable law for purposes such as tax, anti-money-laundering, and cybersecurity.

---

## 8. Intellectual property

8.1 **Our rights**. Herald and its components are owned by the Operator or its licensors. No rights are granted except as expressly stated.

8.2 **License**. Subject to these Terms, we grant you a limited, personal, non-exclusive, non-transferable, revocable license to use Herald.

8.3 **Your content**. You retain rights to content you submit. You grant us a limited license to process that content to provide the service.

8.4 **Restrictions**. You may not copy, modify, reverse engineer, or create derivative works of Herald without written consent.

---

## 9. Disclaimers and liability

9.1 **As-is**. To the extent permitted by law, Herald is provided "as is" and "as available" without warranties of merchantability, fitness for purpose, or non-infringement.

9.2 **No liability for third parties**. We are not liable for third-party services such as payment providers, OAuth providers, email services, or cloud infrastructure.

9.3 **Liability cap**. To the extent permitted by law, our aggregate liability is limited to the amount you paid us in the twelve months before the event giving rise to liability, or zero if you are a free user. We are not liable for indirect, incidental, special, consequential, or punitive damages, except where prohibited by law.

9.4 **Not professional advice**. Herald does not provide legal, tax, financial, investment, or medical advice.

---

## 10. Changes, suspension, and termination

10.1 **Changes**. We may change, add, or discontinue features. Material changes will be notified under Section 13.

10.2 **Your termination**. You may terminate by deleting your account.

10.3 **Our termination**. We or your Realm administrator may restrict, suspend, or terminate access for violations of these Terms, applicable law, or harmful conduct.

10.4 **Survival**. Provisions that by nature survive termination continue to apply.

---

## 11. Confidentiality

You and we agree to keep confidential any non-public business or technical information disclosed in connection with these Terms, except as needed to provide the service or as required by law.

---

## 12. Governing law and dispute resolution

12.1 **Governing law**. These Terms are governed by **[jurisdiction]** law, excluding conflict-of-law rules.

12.2 **Dispute resolution**. Disputes will first be addressed through good-faith negotiation. If unresolved, either party may bring suit or arbitration in **[forum]**.

12.3 **Consumer rights**. Nothing in these Terms overrides mandatory consumer protection rights in your jurisdiction.

---

## 13. Changes to these Terms

13.1 **Updates**. We may revise these Terms. Material changes affecting fees, refunds, account deletion, data handling, or dispute resolution will be notified in advance.

13.2 **Renewed consent**. For material changes, you may need to provide explicit consent before continuing to use Herald.

13.3 **Notices**. We may notify you by email or through the service.

---

## 14. Contact information

**Operator**: **[Operator name]**  
**Address**: **[Address]**  
**Email**: **[Email]**  
**Support**: **[Support contact]**  
**Data protection officer**: **[DPO contact, if applicable]**

---

*These Terms are a platform default template (draft) published on 2026-06-29 and are subject to legal review before formal use.*
$tos$),
    'default',
    'system'
WHERE NOT EXISTS (
    SELECT 1 FROM legal_agreement_version
    WHERE realm_id IS NULL AND agreement_type = 'terms_of_service'
);

INSERT INTO legal_agreement_version (realm_id, agreement_type, version_no, version_label, content, source, published_by)
SELECT
    NULL,
    'privacy_policy',
    1,
    NULL,
    jsonb_build_object('en', $pp$# Herald Privacy Policy

> **Draft date**: 2026-06-29
>
> **Status**: Platform default template (draft, pending legal review)

---

## Important notice

This Privacy Policy explains how **[Operator name]** ("we", "us", or "Operator") collects, uses, stores, shares, transfers, and protects your personal information when you use Herald.

Please read this policy carefully. By agreeing to the Terms of Service and using Herald, you acknowledge that you have read and understood this Privacy Policy.

This Privacy Policy is part of the Terms of Service. In case of conflict, this Privacy Policy governs matters related to personal information.

---

## Table of contents

1. Who we are
2. Information we collect
3. Purposes and legal bases
4. Data retention
5. Data sharing and processors
6. International transfers
7. Data security
8. Cookies and similar technologies
9. Children's privacy
10. Your rights
11. Policy changes
12. Contact information

---

## 1. Who we are

1.1 **Data controller**: **[Operator name]** is the data controller for personal information processed through Herald.

1.2 **Multi-tenancy**: Herald uses a Realm architecture. Your Realm operator may be an independent data controller or joint controller for your Realm-specific data. This policy describes the platform operator's processing; your Realm may provide additional notices.

1.3 **Contact details**: See Section 12.

---

## 2. Information we collect

We collect only the personal information necessary to provide Herald.

### 2.1 Information you provide

- Registration information: email address, nickname, password.
- OAuth information: when you log in via Google, GitHub, Facebook, Apple, or similar, we receive the information you authorize (typically email, display name, and provider identifier).
- TOTP information: if you enable two-factor authentication, the TOTP secret and binding.
- Invoice information: billing name, address, email, phone, tax ID.
- Other content you voluntarily provide, such as notes.

### 2.2 Information collected automatically

- Login and session information: time, result, session token, IP address, device and browser characteristics.
- Usage logs and audit logs of key operations, scoped to your Realm and accessible to your Realm administrator and us.

### 2.3 Billing and points information

- Transaction records, subscription status, payment attempts, points balance and history, invoices.
- We do not store full payment card or bank account details. Payments are processed by payment providers.

### 2.4 Aggregated data

We may generate aggregated and anonymized statistics that do not identify you.

---

## 3. Purposes and legal bases

| Purpose | Legal basis |
|---|---|
| Providing registration, login, and authentication | Contract performance |
| Processing subscriptions, purchases, points, and invoices | Contract performance |
| Customer support | Contract performance and legitimate interests |
| Legal and compliance obligations | Legal obligation |
| Security, fraud prevention, and audit logs | Legitimate interests and legal obligation |
| Service updates and policy changes | Contract performance and legitimate interests |
| Service improvement using aggregated data | Legitimate interests and, where required, consent |
| OAuth linking and unlinking | Consent |
| Marketing or personalization, if offered | Consent |

You may withdraw consent at any time. Withdrawal does not affect processing already performed.

---

## 4. Data retention

We retain personal information only as long as necessary for the purposes described.

| Data category | Retention |
|---|---|
| Core account information | For the life of the account; anonymized or deleted after deletion |
| Password hash | For the life of the account; anonymized after deletion |
| OAuth bindings | For the life of the account; removed after deletion |
| TOTP secrets | While enabled; removed when disabled or account deleted |
| Login/session logs | [To be configured, e.g., 12 months] |
| Audit logs | [To be configured, e.g., 24 months] or as required by law |
| Billing/transaction records | As required by tax and accounting law |
| Invoices | As required by tax and accounting law |
| Support correspondence | [To be configured, e.g., 24 months] from last interaction |

After retention periods expire, data is deleted or irreversibly anonymized. Where legal retention applies, data is used only for compliance.

---

## 5. Data sharing and processors

5.1 **Sharing**. We may share personal information with:

- Processors necessary to provide the service (listed below).
- Third parties with your consent or at your request.
- Authorities when required by law.
- Parties involved in a merger, acquisition, or asset transfer, under confidentiality obligations.

We do not sell your personal information.

5.2 **Processors**. We use categories of processors such as OAuth providers, payment providers, email services, cloud hosting, and audit/log services. These processors process data only for the purposes we define.

5.3 **Realm administrators**. Your Realm administrator may access your Realm data as needed to administer the Realm.

5.4 **Affiliates**. We may share necessary information with affiliates under equivalent protection obligations.

---

## 6. International transfers

Personal information may be transferred outside your jurisdiction. Where required, we use safeguards such as adequacy decisions, standard contractual clauses, binding corporate rules, or supplementary measures.

---

## 7. Data security

We implement technical and organizational measures to protect personal information, including encryption, access controls, audit logging, and secure credential management. No electronic storage is completely secure.

---

## 8. Cookies and similar technologies

We and our processors may use cookies, local storage, and session tokens to maintain sessions, remember preferences, provide security, and collect anonymized usage statistics. You can manage cookies through your browser.

---

## 9. Children's privacy

Herald is not directed at children below the age of digital consent in their jurisdiction. We do not knowingly collect personal information from children without appropriate consent.

---

## 10. Your rights

Depending on applicable law, you may have the right to:

- Be informed about processing.
- Access your personal information.
- Correct inaccurate information.
- Request deletion (right to be forgotten).
- Restrict processing.
- Receive your data in a portable format.
- Object to processing based on legitimate interests or consent.
- Not be subject to solely automated decisions with legal effect.
- Withdraw consent.
- Complain to a supervisory authority.

To exercise your rights, contact us as described in Section 12.

### 10.1 Account deletion

You may delete your account through Herald's self-service interface. Deletion uses soft deletion as described in the Terms of Service. Legally required data may be retained for compliance only.

---

## 11. Policy changes

We may update this Privacy Policy. Material changes will be notified in advance. If required, we will obtain renewed consent before continuing processing.

---

## 12. Contact information

**Operator**: **[Operator name]**  
**Address**: **[Address]**  
**Email**: **[Email]**  
**Support**: **[Support contact]**  
**Data protection officer**: **[DPO contact, if applicable]**

---

*This Privacy Policy is a platform default template (draft) published on 2026-06-29 and is subject to legal review before formal use.*
$pp$),
    'default',
    'system'
WHERE NOT EXISTS (
    SELECT 1 FROM legal_agreement_version
    WHERE realm_id IS NULL AND agreement_type = 'privacy_policy'
);
