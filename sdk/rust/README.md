# herald-sdk

Rust SDK for [Herald](https://github.com/timzaak/herald) — a multi-tenant authentication, authorization, billing & points system.

## Features

- **Permission checking** with built-in caching and token-based invalidation
- **Subscription management** — get subscription details with `entitlement_key`
- **Points system** — check balance, consume points with idempotency support
- Async/await native, built on `reqwest`

## Usage

```rust
use herald_sdk::{Client, PermissionCheckRequest, Rule};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let client = Client::new(
        "https://your-herald-instance.com".to_string(),
        "your-api-key".to_string(),
        Some(Duration::from_secs(300)), // cache TTL
    );

    // Check permission
    let resp = client.check_permission(PermissionCheckRequest {
        access_token: "user-token".to_string(),
        rules: vec![Rule {
            resource: "document".to_string(),
            action: "read".to_string(),
        }],
    }).await.unwrap();

    println!("allowed: {}", resp.allowed);
    // On a denial, `resp.error` separates an unusable token (`token_not_found`,
    // `invalid_token`) from a denied rule (no `error`).

    // Get subscription
    let sub = client.get_subscription("realm-id", "client-app-id").await.unwrap();
    println!("subscription status: {}", sub.status);

    // Check points balance
    let balance = client.get_balance("realm-id", "user-id").await.unwrap();
    println!("balance: {}", balance.balance);

    // Get a single points transaction by ID
    let tx = client.get_transaction("realm-id", "transaction-id").await.unwrap();
    println!("type: {}, balanceAfter: {}", tx.transaction_type, tx.balance_after);

    // Consume points. `client_app_id` is the client app's UUID `id` (as
    // returned by `create_client_app` / `list_client_apps`), not its `client_id`.
    let result = client.consume_points(
        "realm-id",
        "user-id",
        "client-app-uuid",
        100,
        Some("Purchase item X".to_string()),
        Some("idempotency-key-123".to_string()),
    ).await.unwrap();
    // One transaction per affected Credit Bucket; length 1 for single-pool.
    let primary = &result.transactions[0];
    println!("correlationId: {}", result.correlation_id);
    println!("remaining balance: {}", primary.balance_after);
}
```

## License

Apache-2.0
