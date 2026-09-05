//! Realm validation utilities
//!
//! This module provides validation functions for Realm IDs and related values,
//! ensuring business rules are enforced at the domain layer.

use crate::common::entities::app_errors::CoreError;

/// Reserved words that cannot be used as realm IDs
const RESERVED_WORDS: &[&str] = &["admin", "system", "api", "www"];

/// Validates a realm ID according to business rules
///
/// # Rules
/// - Must be between 3 and 36 characters
/// - Must start with an alphanumeric character
/// - Can only contain letters, numbers, hyphens, and underscores
/// - Must not be a reserved word (admin, system, api, www)
///
/// # Arguments
/// * `realm_id` - The realm ID to validate
///
/// # Returns
/// * `Ok(())` if the realm ID is valid
/// * `Err(CoreError::BadRequest)` if validation fails
pub fn validate_realm_id(realm_id: &str) -> Result<(), CoreError> {
    // Check length: 3-36 characters
    if realm_id.len() < 3 || realm_id.len() > 36 {
        return Err(CoreError::BadRequest(
            "Realm ID must be between 3 and 36 characters".to_string(),
        ));
    }

    // Check first character: must be alphanumeric
    if !realm_id
        .chars()
        .next()
        .map(|c| c.is_ascii_alphanumeric())
        .unwrap_or(false)
    {
        return Err(CoreError::BadRequest(
            "Realm ID must start with an alphanumeric character".to_string(),
        ));
    }

    // Check all characters: only alphanumeric, hyphen, and underscore
    if !realm_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CoreError::BadRequest(
            "Realm ID can only contain letters, numbers, hyphens, and underscores".to_string(),
        ));
    }

    // Check reserved words (case-insensitive)
    if RESERVED_WORDS.contains(&realm_id.to_lowercase().as_str()) {
        return Err(CoreError::BadRequest(format!(
            "'{}' is a reserved word and cannot be used as a realm ID",
            realm_id
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_realm_id_too_short() {
        // Too short (< 3 characters)
        assert!(validate_realm_id("ab").is_err());
        assert!(validate_realm_id("a").is_err());
        assert!(validate_realm_id("").is_err());
    }

    #[test]
    fn test_validate_realm_id_too_long() {
        // Too long (> 36 characters)
        let long_id = "a".repeat(37);
        assert!(validate_realm_id(&long_id).is_err());
    }

    #[test]
    fn test_validate_realm_id_invalid_first_char() {
        // Must start with alphanumeric
        assert!(validate_realm_id("-realm").is_err());
        assert!(validate_realm_id("_realm").is_err());
        assert!(validate_realm_id("1realm").is_ok()); // Number is OK
        assert!(validate_realm_id("arealm").is_ok()); // Letter is OK
    }

    #[test]
    fn test_validate_realm_id_invalid_characters() {
        // Only alphanumeric, hyphen, underscore allowed
        assert!(validate_realm_id("realm.id").is_err());
        assert!(validate_realm_id("realm id").is_err());
        assert!(validate_realm_id("realm@id").is_err());
        assert!(validate_realm_id("realm#id").is_err());
        assert!(validate_realm_id("realm$id").is_err());
        assert!(validate_realm_id("realm%id").is_err());
        assert!(validate_realm_id("realm&id").is_err());
        assert!(validate_realm_id("realm*id").is_err());
        assert!(validate_realm_id("realm+id").is_err());
        assert!(validate_realm_id("réalm").is_err());
    }

    #[test]
    fn test_validate_realm_id_reserved_words() {
        // Reserved words (case-insensitive)
        assert!(validate_realm_id("admin").is_err());
        assert!(validate_realm_id("ADMIN").is_err());
        assert!(validate_realm_id("Admin").is_err());
        assert!(validate_realm_id("system").is_err());
        assert!(validate_realm_id("api").is_err());
        assert!(validate_realm_id("www").is_err());
    }

    #[test]
    fn test_validate_realm_id_max_length_boundary() {
        // Exactly 36 characters should be valid
        let max_id = "a".repeat(36);
        assert!(validate_realm_id(&max_id).is_ok());

        // 37 characters should be invalid
        let too_long = "a".repeat(37);
        assert!(validate_realm_id(&too_long).is_err());
    }

    #[test]
    fn test_validate_realm_id_min_length_boundary() {
        // Exactly 3 characters should be valid
        assert!(validate_realm_id("abc").is_ok());

        // 2 characters should be invalid
        assert!(validate_realm_id("ab").is_err());
    }
}
