# Live Demo Tests

`demo/e2e/live` is only for demo tests that depend on real external services,
real credentials, manual authorization, QR scanning, or public webhook callbacks.
Regular user-story demos that run fully inside the seeded demo environment belong
in the role or domain directories under `demo/e2e/`:
`billing-admin/`, `realm-admin/`, `regular-user/`, `super-admin/` group demos by
the acting role, and `live/` groups them by external dependency.

The 8 legacy demo files directly under `demo/e2e/` (oauth-pkce-*, device-code-*,
i18n-*, authentication-redirect) predate this grouping convention; new demos must
not be added at the root. Shared infrastructure (fixtures/, helpers/, pages/,
selectors.ts) is intentionally root-level.

## Index

| File | Related US | Coverage status | External dependency | Manual |
| --- | --- | --- | --- | --- |
| `auth/oauth/us-ru-003-github-oauth-live.e2e.ts` | US-RU-003 | Partial: GitHub success only | GitHub OAuth | Yes |
| `billing/payment-attempt/us-pa-001-creem-checkout-live.e2e.ts` | US-PA-001, US-PA-002, US-PA-003 | Partial: Creem checkout smoke only | Creem | Maybe |
| `billing/payment-attempt/us-pa-001-stripe-checkout-live.e2e.ts` | US-PA-001, US-PA-002, US-PA-003, US-PV-001, US-IF-004, US-IF-007, US-IF-008 | Partial: Stripe checkout smoke + invoice field/provider-filter coverage + credit note refund sync | Stripe | No |
| `billing/multiple-price-purchase/us-em-009-multiple-price-checkout-live.e2e.ts` | US-EM-008 S1, US-EM-009 S1/S2 | Partial: real annual-price checkout references the real Stripe price; price-level grant assertion only with seeded webhook secret + public endpoint | Stripe | Maybe |
| `billing/one-time-mapping-purchase/us-pu-006-one-time-purchase-live.e2e.ts` | US-PU-006 S1, S2 | Partial: Stripe / Creem redirect initiation only | Stripe / Creem | No |
| `billing/one-time-mapping-purchase/us-pu-006-stripe-one-time-invoice-live.e2e.ts` | US-PU-006, US-IF-004 | Partial: one-time Stripe invoice field/provider-filter verification | Stripe | No |
| `billing/one-time-mapping-purchase/us-pu-006-creem-one-time-invoice-live.e2e.ts` | US-PU-006 | Partial: one-time Creem invoice verification only | Creem | No |
| `billing/payment-invoice-mapping/us-pm-002-creem-renewal-tran-stability-live.e2e.ts` | US-PM-001, US-PM-002 | Partial: Creem renewal tran_ existence + renewal attempt provider_reference smoke only | Creem | Yes |
| `billing/one-time-mapping-purchase/us-pw-003-stripe-paywall-grant-live.e2e.ts` | US-PW-002, US-PW-003 (场景1), US-PW-006 (场景1) | Partial: real Stripe one-time checkout → role grant (source=payment) + third-party RBAC gate | Stripe | No |
| `core/us-ra-013-qq-smtp-live.e2e.ts` | US-RA-013 (场景2), US-RA-014 (场景1) | Partial: QQ SMTP config saved + test email sent via API | QQ Mail SMTP | No |

## Rules

- Live tests must declare `Related User Stories`, `Coverage`, `Not Covered`,
  `Live Dependency`, `Manual Step`, `Run Command`, and `Skip/Fail Policy` in the
  file header.
- Live tests are integration smoke tests by default. Do not count them as full
  user-story coverage unless the file explicitly says `Coverage: complete`.
- Live tests may seed and clean up real third-party credentials through API
  helpers when that setup is part of external integration validation.
- Live tests are not part of the default demo regression set unless a command
  explicitly targets them.
- If a test does not require a real external service, real credential, manual
  authorization, QR scan, or public callback URL, do not place it in this
  directory.
