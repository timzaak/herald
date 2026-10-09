# Herald

[中文](README-zh.md) | English

[![CI Pipeline](https://github.com/timzaak/herald/actions/workflows/ci.yml/badge.svg)](https://github.com/timzaak/herald/actions/workflows/ci.yml)
[![CD Pipeline](https://github.com/timzaak/herald/actions/workflows/cd.yml/badge.svg)](https://github.com/timzaak/herald/actions/workflows/cd.yml)
[![Release](https://img.shields.io/github/v/release/timzaak/herald)](https://github.com/timzaak/herald/releases/latest)

**Open-source, self-hosted infrastructure for AI products.**

Start with multi-tenant authentication, billing, payments, paywall-ready entitlements, credits, and an admin console already connected. Adapt the open codebase with AI-assisted development, then spend your iterations on the product logic customers actually pay for.

[Website](https://www.fornetcode.com) · [Live Demo](https://auth.fornetcode.com) · [Get Started](https://www.fornetcode.com/en/docs/getting-started) · [Star on GitHub](https://github.com/timzaak/herald)

## Why Herald

AI startup teams need to validate and iterate quickly, but every paid product still needs accounts, tenant isolation, permissions, subscriptions, usage credits, and operational tooling. Building those systems from scratch—or stitching together separate providers—takes time away from the experience that makes the product unique.

Herald is more than an auth provider. It connects identity to payments, entitlements, and product usage in one codebase — a production-ready paywall out of the box:

- a purchase can grant access and credits;
- a refund or canceled subscription can revoke them;
- every tenant can have its own users, roles, apps, branding, and billing setup;
- the entire foundation remains self-hosted and customizable.

## What You Get

| Area | Included capabilities |
|------|-----------------------|
| **Multi-tenant identity** | Isolated Realms, email/password plus one-time-code login, Google, GitHub, Apple, Facebook, WeChat, Discord and LDAP login, passkeys, TOTP 2FA, an OpenID Connect identity layer, bot protection |
| **Authorization & apps** | Realm-level RBAC, Client Apps, API keys, OAuth 2.0, device authorization, cross-app SSO |
| **Billing & payments** | Stripe, Creem, and WeChat Pay, App Store / Google Play in-app purchases, subscriptions, one-time purchases, multi-currency pricing, invoices, payment-to-entitlement mapping, payment and credit-consumption statistics — an instant paywall |
| **Credits & usage** | Prepaid balances, top-ups, refunds, expiry, per-user ledgers, grants, and rolling quota windows |
| **Admin & operations** | Users, roles, billing, credits, apps, tenant settings, audit trails, and account lifecycle management |
| **Product customization** | Custom domains, white-label branding, transactional email, versioned legal agreements, API docs and SDKs |

This foundation is especially useful for AI products with a paywall, free allowances, paid plans, metered usage, or credit-based pricing.

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

Stuck on setup, or evaluating Herald for your product? Email [zsy.evan@gmail.com](mailto:zsy.evan@gmail.com) — happy to help.

## Try the Live Demo

Open [auth.fornetcode.com](https://auth.fornetcode.com) and click **Create Your Realm** on the sign-in page to spin up your own realm. You become its admin, with a fully isolated tenant to explore — no shared account, no one else's data.

## For AI Agents

- **Docs for LLMs:** [llms.txt](https://www.fornetcode.com/llms.txt) (index) · [llms-full.txt](https://www.fornetcode.com/llms-full.txt) (full text) · append `.md` to any docs URL for Markdown
- **API reference (OpenAPI):** [`docs-web/openapi.json`](docs-web/openapi.json)
- **MCP server:** OAuth browser sign-in — no API key, no secret:

  ```bash
  claude mcp add --transport http herald https://your-herald-host/mcp/your-realm-id
  ```

  Setup and tool reference: [MCP Integration for AI Agents](https://www.fornetcode.com/en/docs/integration/mcp)

## Talk to Us

Building a product on Herald? We'd love to hear what you're making.

- 🐛 [Open an issue](https://github.com/timzaak/herald/issues) for bugs and feature requests
- ✉️ [zsy.evan@gmail.com](mailto:zsy.evan@gmail.com) for deployment help, feedback, or just to tell us what you're building

## License

Herald is licensed under [Apache-2.0](LICENSE). You can use, modify, and distribute it, including in commercial products. The open-source project has no per-user license fee.
