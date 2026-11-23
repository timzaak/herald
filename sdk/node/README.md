# herald-sdk

Node.js SDK for [Herald](https://github.com/timzaak/herald) — a multi-tenant
authentication, authorization, billing & points system. This is the
server-side TypeScript counterpart of the Rust
[`herald-sdk`](../rust) crate, with the same API surface and caching
behaviour. Zero runtime dependencies (native `fetch`, Node 18+).

For browser end-user authentication (login flows, token refresh, WebAuthn)
use [`herald-auth-web`](../web) instead.

## Features

- **Permission checking** with built-in caching (per-request TTL cache,
  token-indexed invalidation, 5-minute token staleness heuristic)
- **Organization authorization** — create/delete organizations, manage
  memberships and tiers (including on-behalf-of `actorUserId`), check
  per-organization permissions, and list user organizations
- **Outbound webhook verification** — constant-time HMAC-SHA256 verification,
  replay-window enforcement, multi-secret rotation support, and typed event
  envelope parsing (`verifyWebhookSignature` / `verifyAndParseWebhookEvent`)
- **Subscription management** — get subscription details by `entitlementKey`
- **Points system** — check balance, consume points with idempotency support,
  grant points to explicit Credit Buckets
- **Realm / user / client-app administration** over the external API
- ESM + CommonJS dual build, async/await native, optional `withHttpTimeout`

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

// Get a single points transaction by ID
const tx = await client.getTransaction('realm-id', 'transaction-id')
console.log('type:', tx.transactionType, 'balanceAfter:', tx.balanceAfter)

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

// Organizations (instance-level workspace authorization)
const org = await client.createOrg('realm-id', {
  externalId: 'workspace-42',
  name: 'Acme Workspace',
  ownerId: 'owner-user-id',
})
await client.upsertOrgMember('realm-id', org.id, {
  userId: 'member-user-id',
  role: 'org-admin',
})
const orgCheck = await client.checkOrgPermission('realm-id', org.id, {
  userId: 'member-user-id',
  rules: [{ resource: 'project', action: 'delete' }],
})
console.log('org allowed:', orgCheck.member && orgCheck.allowed)
```

### Outbound Webhooks

Verify Herald's HMAC-SHA256 webhook signature over the **raw** request bytes
and parse the typed event envelope. Pass an array of secrets during key
rotation for zero-downtime transitions:

```ts
import { HeraldWebhookError, verifyAndParseWebhookEvent } from 'herald-sdk'

app.post('/webhooks/herald', express.raw({ type: 'application/json' }), async (req, res) => {
  try {
    const event = verifyAndParseWebhookEvent({
      payload: req.body,
      headers: req.headers,
      secret: [process.env.HERALD_WEBHOOK_SECRET!, process.env.HERALD_WEBHOOK_NEXT_SECRET!],
    })

    if (event.type === 'role.granted') {
      // Paywall unlock in seconds
      console.log('unlock', event.data.userId, event.data.roleName)
    } else if (event.type === 'user.disabled') {
      // Instant session takedown
      console.log('disable', event.data.userId)
    }

    res.status(200).send()
  } catch (error) {
    if (error instanceof HeraldWebhookError) {
      return res.status(400).send(error.code)
    }
    throw error
  }
})
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
