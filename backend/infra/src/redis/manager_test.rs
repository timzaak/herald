// =============================================================================
// Redis Connection Manager Tests - Infrastructure Layer
// =============================================================================
//
// DB isolation between production and test environments
//
// =============================================================================

use super::*;
use redis::AsyncCommands;

/// Test DB isolation between production and test environments
#[tokio::test]
async fn test_db_isolation() {
    // Production environment (DB 0)
    let prod_config = ManagerConfig {
        url: "redis://127.0.0.1:6379/0".to_string(),
        default_db: 0,
        test_mode: false,
        test_db: 1,
    };
    let prod_manager = RedisConnectionManager::new(prod_config).await;

    // Test environment (DB 1)
    let test_config = ManagerConfig {
        url: "redis://127.0.0.1:6379/0".to_string(),
        default_db: 0,
        test_mode: true,
        test_db: 1,
    };
    let test_manager = RedisConnectionManager::new(test_config).await;

    // Skip test if Redis is not available
    if prod_manager.is_err() || test_manager.is_err() {
        println!("Redis not available, skipping test");
        return;
    }

    let prod_manager = prod_manager.unwrap();
    let test_manager = test_manager.unwrap();

    // Write to DB 0
    let mut prod_conn = prod_manager.get().await.unwrap();
    let _: () = prod_conn.set("isolation_key", "prod_value").await.unwrap();

    // Write to DB 1 with same key
    let mut test_conn = test_manager.get().await.unwrap();
    let _: () = test_conn.set("isolation_key", "test_value").await.unwrap();

    // Verify DB 0 data
    let prod_value: String = prod_conn.get("isolation_key").await.unwrap();
    assert_eq!(prod_value, "prod_value");

    // Verify DB 1 data
    let test_value: String = test_conn.get("isolation_key").await.unwrap();
    assert_eq!(test_value, "test_value");

    // Cleanup
    let _: () = prod_conn.del("isolation_key").await.unwrap();
    let _: () = test_conn.del("isolation_key").await.unwrap();
}
