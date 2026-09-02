// =============================================================================
// User TOTP Entities Tests
// =============================================================================
//
// 测试 User TOTP 相关实体的业务逻辑
//
// **测试目标**：
// 1. 验证 UserTotpConfig 创建和状态转换
// 2. 验证 UserTotpBackupCode 创建和使用状态
// 3. 验证 key_version 字段正确性
// 4. 验证响应类型构造
//
// **测试类型**：单元测试
//
// =============================================================================

use crate::common;
use crate::user_totp::entities::{UserTotpBackupCode, UserTotpConfig};

// ============================================================================
// Unit Tests: UserTotpConfig
// ============================================================================

#[test]
fn test_unit_user_totp_config_new() {
    let user_id = common::generate_uuid_v7();
    let realm_id = "test-realm".to_string();
    let secret_hash = "encrypted_secret".to_string();
    let key_version = 1;

    let config = UserTotpConfig::new(user_id, realm_id.clone(), secret_hash.clone(), key_version);

    assert!(!config.id.to_string().is_empty());
    assert_eq!(config.user_id, user_id);
    assert_eq!(config.realm_id, realm_id);
    assert_eq!(config.secret_hash, secret_hash);
    assert_eq!(config.key_version, key_version);
    assert!(!config.enabled, "New config should be disabled by default");
    assert!(config.verified_at.is_none());
    assert!(config.last_used_at.is_none());
}

#[test]
fn test_unit_user_totp_config_enable() {
    let mut config = UserTotpConfig::new(
        common::generate_uuid_v7(),
        "test-realm".to_string(),
        "encrypted_secret".to_string(),
        1,
    );

    assert!(!config.enabled);

    let before_enable = config.updated_at;
    config.enable();

    assert!(config.enabled);
    assert!(config.verified_at.is_some());
    assert!(
        config.updated_at > before_enable,
        "updated_at should be updated"
    );
}

#[test]
fn test_unit_user_totp_config_disable() {
    let mut config = UserTotpConfig::new(
        common::generate_uuid_v7(),
        "test-realm".to_string(),
        "encrypted_secret".to_string(),
        1,
    );

    config.enable();
    assert!(config.enabled);

    // Add a small delay to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(1));
    config.disable();

    assert!(!config.enabled);
    assert!(
        config.verified_at.is_some(),
        "verified_at should be preserved"
    );
}

#[test]
fn test_unit_user_totp_config_update_last_used() {
    let mut config = UserTotpConfig::new(
        common::generate_uuid_v7(),
        "test-realm".to_string(),
        "encrypted_secret".to_string(),
        1,
    );

    assert!(config.last_used_at.is_none());

    // Add a small delay to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(1));
    config.update_last_used();

    assert!(config.last_used_at.is_some());
}

#[test]
fn test_unit_user_totp_config_regenerate_secret() {
    let mut config = UserTotpConfig::new(
        common::generate_uuid_v7(),
        "test-realm".to_string(),
        "encrypted_secret".to_string(),
        1,
    );

    config.enable();

    let new_secret_hash = "new_encrypted_secret".to_string();
    let before_regenerate = config.updated_at;

    config.regenerate_secret(new_secret_hash.clone());

    assert_eq!(config.secret_hash, new_secret_hash);
    assert!(
        !config.enabled,
        "Config should be disabled after regeneration"
    );
    assert!(config.verified_at.is_none());
    assert!(config.last_used_at.is_none());
    assert!(config.updated_at > before_regenerate);
    assert_eq!(config.key_version, 1, "key_version should remain unchanged");
}

// ============================================================================
// Unit Tests: UserTotpBackupCode
// ============================================================================

#[test]
fn test_unit_user_totp_backup_code_mark_as_used() {
    let config_id = common::generate_uuid_v7();
    let code_hash = "$2b$12$some_bcrypt_hash".to_string();

    let mut backup_code = UserTotpBackupCode::new(config_id, code_hash);

    assert!(!backup_code.used);
    assert!(backup_code.used_at.is_none());

    backup_code.mark_as_used();

    assert!(backup_code.used);
    assert!(backup_code.used_at.is_some());
}

// ============================================================================
// Unit Tests: TotpSetupResponse & TotpStatusResponse
// ============================================================================

// NOTE: Low-value tests removed (test_unit_totp_setup_response, test_unit_totp_status_response)
// These tests only verified struct field assignments without business logic.
// Field assignments are covered by business logic tests below.

// ============================================================================
// Unit Tests: RealmTotpConfig & RealmTotpStatistics
// ============================================================================

// NOTE: Low-value tests removed (test_unit_realm_totp_config_*, test_unit_realm_totp_statistics_*)
// These tests only verified struct field assignments without business logic.
// RealmTotpConfig and RealmTotpStatistics are simple data structures used for API responses,
// field assignments are covered by integration tests.

// ============================================================================
// Unit Tests: UserTotpConfig Equality and Clone
// ============================================================================

// NOTE: Low-value tests removed (test_unit_user_totp_config_clone, test_unit_user_totp_config_equality)
// These tests only verified trait implementations (Clone, PartialEq) without business logic.
// Trait derivations are covered by Rust's type system and compile-time guarantees.
