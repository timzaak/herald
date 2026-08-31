# Herald

[中文](README-zh.md) | English

[![CI Pipeline](https://github.com/timzaak/herald/actions/workflows/ci.yml/badge.svg)](https://github.com/timzaak/herald/actions/workflows/ci.yml)
[![CD Pipeline](https://github.com/timzaak/herald/actions/workflows/cd.yml/badge.svg)](https://github.com/timzaak/herald/actions/workflows/cd.yml)
[![Release](https://img.shields.io/github/v/release/timzaak/herald)](https://github.com/timzaak/herald/releases/latest)

**Open-source, self-hosted infrastructure for AI products.**

Start with multi-tenant authentication, billing, payments, credits, and an admin console already connected. Adapt the open codebase with AI-assisted development, then spend your iterations on the product logic customers actually pay for.

[Website](https://www.fornetcode.com) · [Live Demo](https://auth.fornetcode.com) · [Get Started](https://www.fornetcode.com/en/docs/getting-started) · [Star on GitHub](https://github.com/timzaak/herald)

## Why Herald

AI startup teams need to validate and iterate quickly, but every paid product still needs accounts, tenant isolation, permissions, subscriptions, usage credits, and operational tooling. Building those systems from scratch—or stitching together separate providers—takes time away from the experience that makes the product unique.

Herald is more than an auth provider. It connects identity to payments, entitlements, and product usage in one codebase:

- a purchase can grant access and credits;
- a refund or canceled subscription can revoke them;
- every tenant can have its own users, roles, apps, branding, and billing setup;
- the entire foundation remains self-hosted and customizable.

## What You Get

| Area | Included capabilities |
|------|-----------------------|
| **Multi-tenant identity** | Isolated Realms, email/password, Google, GitHub, Apple, Facebook and WeChat login, passkeys, TOTP 2FA, bot protection |
| **Authorization & apps** | Realm-level RBAC, Client Apps, API keys, OAuth 2.0, device authorization, cross-app SSO |
| **Billing & payments** | Stripe, Creem, and WeChat Pay, App Store / Google Play in-app purchases, subscriptions, one-time purchases, invoices, payment-to-entitlement mapping |
| **Credits & usage** | Prepaid balances, top-ups, refunds, expiry, per-user ledgers, grants, and rolling quota windows |
| **Admin & operations** | Users, roles, billing, credits, apps, tenant settings, audit trails, and account lifecycle management |
| **Product customization** | Custom domains, white-label branding, transactional email, versioned legal agreements, API docs and SDKs |

This foundation is especially useful for AI products with free allowances, paid plans, metered usage, or credit-based pricing.

## Built for AI-Assisted Iteration

Herald gives AI coding tools a complete, working product foundation to modify instead of a blank repository or a collection of disconnected APIs. Use it to adapt workflows, roles, integrations, branding, and business rules while preserving a shared model for identity, billing, and usage.

The project itself is developed with a hybrid AI-assisted workflow using Claude Code, GLM, and Codex. Its development toolkit builds on [web-dev-skills](https://github.com/timzaak/web-dev-skills).

[herald-app-example](https://github.com/timzaak/herald-app-example) is a complete, fully AI-developed Flutter app that integrates Herald authentication — a working reference for this approach.

## Quick Start

You need Python 3.12+ with [uv](https://github.com/astral-sh/uv), Docker, Cargo, and npm.

```bash
git clone https://github.com/timzaak/herald.git
cd herald
uv run scripts/demo-start.py
```

Once running:

- Frontend: http://localhost:3000
- Backend API: http://localhost:8080

See the [Getting Started guide](https://www.fornetcode.com/en/docs/getting-started) for manual setup and next steps.

## Try the Live Demo

Open [auth.fornetcode.com](https://auth.fornetcode.com) and sign in with:

```text
Email:    admin@fornetcode.com
Password: Herald@2026Admin
```

The demo lets you explore the admin experience before running Herald locally.

## For AI Agents

Herald's documentation is authored as MDX under [`docs-web/content/docs/`](docs-web/content/docs/) (English) and [`docs-web/content/docs/zh/`](docs-web/content/docs/zh/) (Chinese), and published at https://www.fornetcode.com. Prefer these structured sources over scraping rendered HTML:

- **Full doc index (for LLMs):** https://www.fornetcode.com/llms.txt
- **Full doc text (for LLMs):** https://www.fornetcode.com/llms-full.txt
- **Single page as Markdown:** append `.md` to any doc URL, e.g. https://www.fornetcode.com/en/docs/auth-passkey.md
- **API reference (OpenAPI):** [`docs-web/openapi.json`](docs-web/openapi.json)

Source MDX files (in-repo): `docs-web/content/docs/{getting-started,architecture,configuration,deployment,billing-overview,billing-stripe-payment,billing-creem-payment,billing-iap,billing-wechat-payment,billing-invoice,billing-credit-note,auth-passkey,auth-email-otp,auth-apple-native,third-party-integration,custom-user-ui,ui-custom,realm-custom-domain,points-quota}.mdx`.

## License

Herald is licensed under [Apache-2.0](LICENSE). You can use, modify, and distribute it, including in commercial products. The open-source project has no per-user license fee.
