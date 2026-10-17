# Herald

[中文](README-zh.md) | English

[![CI Pipeline](https://github.com/timzaak/herald/actions/workflows/ci.yml/badge.svg)](https://github.com/timzaak/herald/actions/workflows/ci.yml)
[![CD Pipeline](https://github.com/timzaak/herald/actions/workflows/cd.yml/badge.svg)](https://github.com/timzaak/herald/actions/workflows/cd.yml)
[![Release](https://img.shields.io/github/v/release/timzaak/herald)](https://github.com/timzaak/herald/releases/latest)

**Auth, billing, credits, and an admin console for AI products, in one open-source codebase.**

Run it yourself, and every identity, payment, and credit record sits in your own Postgres. Hand the repo to your AI coding tool, and it starts from a working product instead of a blank project. Apache-2.0, no per-user license fee.

[Website](https://www.fornetcode.com) · [Live demo](https://auth.fornetcode.com) · [Get started](https://www.fornetcode.com/en/docs/getting-started) · [Star on GitHub](https://github.com/timzaak/herald)

## Why Herald

AI startup teams keep hitting the same wall. AI tools build the product itself in days, and then the parts users never see take weeks: accounts, tenant isolation, roles, subscriptions, usage credits, an admin console. You either write all of it from scratch, or stitch together separate auth, payment, and billing services and hope the wiring holds.

The wiring is what breaks. Each provider keeps its own user IDs, its own webhooks, its own retry rules. A subscription renews, the webhook gets dropped, and the customer has paid for access they no longer have. You find out from a support email.

Herald keeps identity, payments, entitlements, and usage in one codebase, so those connections are code you can read and change:

- a purchase grants access and credits;
- a refund or canceled subscription revokes them;
- every tenant can have its own users, roles, apps, branding, and billing setup.

If all you need is login plus card payments, a hosted service is less work. Herald pays off when you want tenant-level control, credit-based pricing, or user data in your own database.

## What you get

| Area | Included capabilities |
|------|-----------------------|
| **Multi-tenant identity** | Isolated realms, email/password and one-time-code login, Google, GitHub, Apple, Facebook, WeChat, Discord, and LDAP login, passkeys, TOTP 2FA, an OpenID Connect layer, Cloudflare Turnstile bot protection |
| **Authorization and apps** | Realm-level RBAC, organizations with their own roles and membership, Client Apps, API keys, OAuth 2.0, device authorization, cross-app SSO |
| **Billing and payments** | Stripe, Creem, and WeChat Pay, App Store / Google Play in-app purchases, subscriptions, one-time purchases, multi-currency pricing, invoices, payment-to-entitlement mapping, payment and credit-consumption statistics |
| **Credits and usage** | Prepaid balances, top-ups, refunds, expiry, per-user ledgers, grants, rolling quota windows |
| **Admin and operations** | Users, roles, billing, credits, apps, tenant settings, audit trails, account lifecycle management |
| **Product customization** | Custom domains, white-label branding, transactional email, versioned legal agreements, API docs, SDKs |

This foundation is built for AI products with a paywall, free allowances, paid plans, metered usage, or credit-based pricing.

## What integration looks like

`herald-auth-web` handles register, login, second factors, and silent token refresh in the browser:

```ts
import { createHeraldClient } from 'herald-auth-web'

const client = createHeraldClient({
  baseUrl: 'https://auth.example.com',
  realmId: '<your-realm>',
  clientId: '<your-client-app>',
})

const result = await client.login({ email: 'user@example.com', password: '••••••••' })
// result.kind: 'success' | 'requires-second-factor' | 'consent-required' | 'oauth-redirect'
```

On the server, `herald-sdk` (Node and Rust) checks permissions and credit balances against the same records:

```ts
import { HeraldClient } from 'herald-sdk'

const herald = new HeraldClient('https://your-herald-instance.com', '<your-api-key>')

const check = await herald.checkPermission({
  accessToken: 'user-browser-token',
  rules: [{ resource: 'document', action: 'read' }],
})

const balance = await herald.getBalance('realm-id', 'user-id')
```

Before the browser SDK can call the API from your page, add the page origin to the Client App's `allowed_origins` in the Herald console.

## Try it

Online, zero install: open [auth.fornetcode.com](https://auth.fornetcode.com) and click **Create Your Realm**. You become the admin of an isolated tenant with its own users and billing setup, so you can experiment without touching anyone else's data.

To run locally you need Python 3.12+ with [uv](https://github.com/astral-sh/uv), Docker, Cargo, and npm:

```bash
git clone https://github.com/timzaak/herald.git
cd herald
uv run scripts/demo-start.py
```

The frontend comes up on http://localhost:3000 and the backend API on http://localhost:8080. For manual setup and next steps, see the [getting started guide](https://www.fornetcode.com/en/docs/getting-started).

Stuck on setup, or evaluating Herald for your product? Email [zsy.evan@gmail.com](mailto:zsy.evan@gmail.com); happy to help.

## Built for AI-assisted iteration

The repo is meant to be modified by AI coding tools:

- Docs for LLMs: [llms.txt](https://www.fornetcode.com/llms.txt) (index) · [llms-full.txt](https://www.fornetcode.com/llms-full.txt) (full text) · append `.md` to any docs URL for Markdown
- API reference (OpenAPI): [`docs-web/openapi.json`](docs-web/openapi.json)
- MCP server with OAuth browser sign-in, no API key, no secret:

  ```bash
  claude mcp add --transport http herald https://your-herald-host/mcp/your-realm-id
  ```

Setup and tool reference: [MCP integration for AI agents](https://www.fornetcode.com/en/docs/integration/mcp).

[herald-app-example](https://github.com/timzaak/herald-app-example) is a complete Flutter app that integrates Herald authentication and was developed entirely with AI; it works as a reference for this approach. Herald itself is developed with a hybrid Claude Code, GLM, and Codex workflow, built on [web-dev-skills](https://github.com/timzaak/web-dev-skills).

## Talk to us

Building a product on Herald? We'd love to hear what you're making.

- 🐛 [Open an issue](https://github.com/timzaak/herald/issues) for bugs and feature requests
- ✉️ [zsy.evan@gmail.com](mailto:zsy.evan@gmail.com) for deployment help, feedback, or just to tell us what you're building

## License

Herald is licensed under [Apache-2.0](LICENSE). You can use, modify, and distribute it, including in commercial products, and the open-source project has no per-user license fee.
