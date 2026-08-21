# @herald/web

Official Herald browser JavaScript SDK for **third-party web integration**.

Framework-agnostic (React, Vue, or plain HTML), **zero runtime dependencies**
(native `fetch` + `WebCrypto` + `localStorage`). Wraps Herald's browser Bearer
authentication lifecycle — register, email verification, password reset, login
(with TOTP / passkey second factors and passwordless email-OTP), silent access
token refresh, logout, and status.

> Package name `@herald/web` is a working name; the final npm scope is TBD.

## Install

```bash
npm install @herald/web
```

The package is **ESM-only** (`import`/`export`). Modern bundlers (Vite, webpack 5+,
Next.js, esbuild, Rollup) and ESM CDNs (esm.sh) consume it directly. It targets
the browser — there is no CommonJS build (Node/server use is a separate
server SDK's job).

### CDN / `<script>` (no build step)

A minified IIFE bundle exposing a `Herald` global is published for third-party
pages that integrate via a script tag:

```html
<script src="https://unpkg.com/@herald/web"></script>
<script>
  const client = Herald.createHeraldClient({
    baseUrl: 'https://auth.example.com',
    realmId: '<your-realm>',
    clientId: '<your-client-app>',
  })
</script>
```

The `unpkg`/`jsdelivr` package fields resolve to `dist/index.global.js`, so the
bare URL above works; `https://cdn.jsdelivr.net/npm/@herald/web` works too.

> Building from source requires regenerating the typed client from the Herald
> backend: `npm run generate-api` (needs `cargo` + the backend), then `npm run build`.


## Quick start

```ts
import { createHeraldClient, HeraldError } from '@herald/web'

const client = createHeraldClient({
  baseUrl: 'https://auth.example.com', // Herald API origin
  realmId: '<your-realm>',
  clientId: '<your-client-app>',
  onSessionChange: (event) => {
    if (event.type === 'session-expired') {
      // redirect to your login page
    }
  },
})

// Register
await client.register({ email: 'user@example.com', password: '••••••••' })

// Email verification / password reset: the backend sends an email with a link
// that 302-redirects to your pre-registered Client App page. The SDK only
// triggers the send:
await client.triggerVerifyEmail({ email: 'user@example.com' })

// Login — multi-branch result (discriminate on `kind`)
const result = await client.login({ email: 'user@example.com', password: '••••••••' })
switch (result.kind) {
  case 'success':
    // logged in; result.session
    break
  case 'requires-second-factor': {
    // result.secondFactors ⊆ ['totp', 'passkey']; result.tempToken
    const final = await client.verifyTotp({ tempToken: result.tempToken, code: '123456' })
    break
  }
  case 'consent-required':
    // render result.agreements, then re-call login({ ..., agreements: result.agreements })
    break
  case 'oauth-redirect':
    window.location.href = result.redirectTo
    break
}

// Authenticated requests automatically inject Bearer + silently refresh on 401.
```

## Origin pre-registration (CORS)

Herald's CORS policy matches the request origin against the Client App's
`allowed_origins` **exactly**. Before the SDK can call the API from your page,
add your page origin (scheme + host + port, e.g. `https://app.example.com`) to
the Client App's `allowed_origins` in the Herald console.

A non-registered origin surfaces as a `HeraldError { kind: 'network' }` (the
browser cannot distinguish a CORS rejection from a generic network failure).

## Turnstile

If the realm enforces Cloudflare Turnstile, pass the Turnstile token via the
`turnstileToken` field of each method payload.

## Token storage model

- **Access token** — held **only in memory** (never persisted). A page reload
  clears it; the SDK silently refreshes it on the next request.
- **Refresh token** — stored via a pluggable `TokenStorage` interface; the
  **default is `localStorage`**. The backend rotates it on every refresh and
  revokes the whole family on reuse detection.

Tradeoff: a refresh token in `localStorage` is readable by XSS. This matches the
Herald own-frontend risk posture and is mitigated by server-side rotation +
reuse detection + absolute TTL + short-lived access tokens. For higher security,
inject `memoryStorage()` (no persistence across reloads) or a custom adapter:

```ts
import { createHeraldClient, memoryStorage } from '@herald/web'

const client = createHeraldClient({
  baseUrl: 'https://auth.example.com',
  realmId: '<realm>',
  clientId: '<client-app>',
  storage: memoryStorage(),
})
```

## SSR / Node

`createHeraldClient` throws `HeraldError { kind: 'ssr-no-storage' }` when no
`storage` adapter is injected and `localStorage` is unavailable (e.g. SSR/Node).
Inject an explicit adapter in non-browser environments.

## Passkey login

Passkey login is two steps with a browser WebAuthn assertion in between:

```ts
import { performPasskeyAssertion } from '@herald/web'

// 1FA passkey login
const begin = await client.passkey.loginBegin({})
const assertion = await performPasskeyAssertion(begin.options)
const result = await client.passkey.loginFinish({ authToken: begin.authToken, assertion })

// 2FA passkey login (after a `requires-second-factor` result):
// const begin = await client.passkey.loginBegin({ tempToken })
```

Passkey RP isolation requires your page origin to match the Client App's
pre-registered origin.

## Passwordless email-OTP login

Email-OTP is an independent **passwordless first factor** (not a second factor).
`send` resolves a discriminated result: the two 409 control-flow outcomes —
`consent_required` (auto-register consent gate; render `agreements` and re-send
with them) and `email_not_registered` (auto-register off) — arrive as
`{ kind: 'conflict' }` instead of throwing:

```ts
const sent = await client.loginWithEmailOtp.send({ email: 'user@example.com' })
if (sent.kind === 'conflict' && sent.code === 'consent_required') {
  // render sent.agreements (each entry carries the raw backend summary on
  // `.raw` for display), then re-send with the accepted pairs:
  await client.loginWithEmailOtp.send({
    email: 'user@example.com',
    agreements: sent.agreements.map(({ agreementType, versionId }) => ({ agreementType, versionId })),
  })
}

// Verify applies the issued token set on success (same as login).
const result = await client.loginWithEmailOtp.verify({ email: 'user@example.com', code: '123456' })
```

## Error handling

Every method rejects with a `HeraldError`. Branch on the stable `kind`:

```ts
try {
  await client.login({ email, password })
} catch (e) {
  if (e instanceof HeraldError) {
    switch (e.kind) {
      case 'unauthorized': // bad credentials
      case 'rate-limited': // 429
      case 'validation':   // 400
      case 'network':      // fetch failed / CORS
      // ...
    }
  }
}
```

## Out of scope

This SDK wraps only the `/login` direct-signed `CustomUserUi` credential class
(no PKCE → FirstParty), and the authentication lifecycle. Server-side resource
management, high-risk operations (password change, authenticator management,
account deletion), and framework-specific adapters are separate concerns.

## First-party consumption (Herald's own frontend)

Herald's own frontend also consumes this SDK as its token engine
(DEC-js-sdk-013). On top of the third-party surface above it uses the additive
first-party bridge:

- `client.tokens.getAccessToken()` / `setTokens({ accessToken, refreshToken,
  clientId? })` / `clear()` / `bindClientId(clientId)` — inspect / inject the
  token family owned by the host app (e.g. after its own PKCE exchange or
  switch-client, which stay in the host per the scope decision above);
- `client.refresh()` — the public single-flight refresh (shares its in-flight
  promise with the 401 interceptor);
- `login` / `passkey.loginBegin` accept an optional OAuth context
  (`oauthClientId`/`redirectUri`/`state`, or the `oauth` object) which is
  passed through untouched; the backend answers with `redirectTo` and the
  caller completes the exchange itself;
- `consent-required` results carry the raw agreement summary on each entry's
  `raw` field for host apps that render the consent list.

## Regenerating the client

The typed HTTP client is generated from the Herald backend OpenAPI spec:

```bash
npm run generate-api   # cargo export-openapi + @hey-api/openapi-ts
npm run build
```
