# herald-sdk

Node.js SDK for [Herald](https://github.com/timzaak/herald) — a multi-tenant
authentication, authorization, billing & points system. This is the
server-side TypeScript counterpart of the Rust
[`herald-sdk`](../backend/sdk) crate, with the same API surface and caching
behaviour. Zero runtime dependencies (native `fetch`, Node 18+).

For browser end-user authentication (login flows, token refresh, WebAuthn)
use [`herald-auth-web`](../sdk-web) instead.

## Features

- **Permission checking** with built-in caching (per-request TTL cache,
  token-indexed invalidation, 5-minute token staleness heuristic)
- **Subscription management** — get subscription details by `entitlementKey`
- **Points system** — check balance, consume points with idempotency support,
  grant points to explicit Credit Buckets
- **Realm / user / client-app administration** over the external API
- ESM + CommonJS dual build, async/await native

## Install

```bash
npm install herald-sdk
```

## Usage

```ts
import { HeraldClient } from 'herald-sdk'

const client = new HeraldClient(
  'https://your-herald-instance.com', // base URL
  'your-api-key',                     // realm/service API key (X-API-Key)
  300,                                // permission cache TTL seconds (default 300)
)

// Check permission (cached per exact request for the TTL)
const resp = await client.checkPermission({
  accessToken: 'user-browser-token', // issued by /api/auth/{realmId}/login
  clientId: 'your-client-id',
  rules: [{ resource: 'document', action: 'read' }],
})
console.log('allowed:', resp.allowed)

// Force-refresh a user's cached checks after you know they changed
client.invalidateCache('user-browser-token')

// Get subscription
const sub = await client.getSubscription('realm-id', 'client-app-id')
console.log('subscription status:', sub.status)

// Check points balance
const balance = await client.getBalance('realm-id', 'user-id')
console.log('balance:', balance.balance)

// Consume points (idempotent via idempotencyKey)
const result = await client.consumePoints(
  'realm-id',
  'user-id',
  'client-app-id',
  100,
  'Purchase item X',
  'idempotency-key-123',
)
// One transaction per affected Credit Bucket; length 1 for single-pool.
const primary = result.transactions[0]
console.log('correlationId:', result.correlationId)
console.log('remaining balance:', primary.balanceAfter)
```

Errors throw `HeraldSdkError` with a stable `code`
(`unauthorized` | `forbidden` | `not-found` | `internal-server-error` |
`api-error` | `network` | `parse`), plus the HTTP `status` and raw `body`
when a response was received:

```ts
import { HeraldSdkError } from 'herald-sdk'

try {
  await client.getBalance('realm-id', 'user-id')
} catch (error) {
  if (error instanceof HeraldSdkError && error.code === 'forbidden') {
    // cross-realm access or insufficient permission
  }
}
```

## License

Apache-2.0
