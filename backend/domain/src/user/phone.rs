/// Normalize to E.164 using the realm's default country code; any failure
/// collapses to `None` so callers emit one uniform `invalid_phone`
/// (anti-enumeration). Length is checked before parsing: the raw value
/// feeds Redis rate-limit keys.
pub fn normalize_phone(raw: &str, default_region: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() > crate::security_constants::PHONE_INPUT_MAX_LEN {
        return None;
    }
    let region: phonenumber::country::Id = default_region.parse().ok()?;
    let parsed = phonenumber::parse(Some(region), trimmed).ok()?;
    if !phonenumber::is_valid(&parsed) {
        return None;
    }
    Some(
        phonenumber::format(&parsed)
            .mode(phonenumber::Mode::E164)
            .to_string(),
    )
}

/// Masked display form (`138****8000`); NSNs shorter than 10 digits degrade
/// to `****` + last-2 so at least 3 digits stay hidden. `None` for
/// non-parsable input. Masking is backend-only; the frontend never sees the
/// raw form.
pub fn mask_phone(e164: &str) -> Option<String> {
    let parsed = phonenumber::parse(None, e164).ok()?;
    let nsn = parsed.national().to_string();
    if nsn.len() >= 10 {
        let bytes = nsn.as_bytes();
        Some(format!(
            "{}****{}",
            std::str::from_utf8(&bytes[..3]).ok()?,
            std::str::from_utf8(&bytes[nsn.len() - 4..]).ok()?
        ))
    } else if nsn.len() >= 2 {
        Some(format!("****{}", &nsn[nsn.len() - 2..]))
    } else {
        Some("****".to_string())
    }
}

/// Whether `code` is a phonenumber-metadata-resolvable ISO 3166-1 alpha-2
/// region (input must already be uppercase, e.g. `"CN"`). Rejects syntactic
/// alpha-2 values that carry no phone metadata.
pub fn is_supported_country_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    bytes.len() == 2
        && bytes[0].is_ascii_uppercase()
        && bytes[1].is_ascii_uppercase()
        && code.parse::<phonenumber::country::Id>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: PRD §5.2 — every writing form of one number must collapse to one
    /// E.164 account key.
    #[test]
    fn normalize_phone_collapses_writing_forms_to_one_e164() {
        let canonical = normalize_phone("13800138000", "CN");
        assert_eq!(canonical.as_deref(), Some("+8613800138000"));
        assert_eq!(normalize_phone("+8613800138000", "CN"), canonical);
        assert_eq!(normalize_phone("0086 138 0013 8000", "CN"), canonical);
        assert_eq!(normalize_phone("+86 138-0013-8000", "CN"), canonical);
        assert_eq!(normalize_phone(" 13800138000 ", "CN"), canonical);
        assert_eq!(
            normalize_phone("(212) 555-1234", "US").as_deref(),
            Some("+12125551234")
        );
        assert_eq!(
            normalize_phone("+12125551234", "CN").as_deref(),
            Some("+12125551234")
        );
    }

    /// WHY: PRD §4.1 anti-enumeration — every invalid input must collapse to
    /// the same `None` failure.
    #[test]
    fn normalize_phone_rejects_uniformly_without_leaking_reason() {
        // 123... is not an allocated CN mobile range.
        assert_eq!(normalize_phone("12345678901", "CN"), None);
        assert_eq!(normalize_phone("12345", "CN"), None);
        assert_eq!(normalize_phone("not-a-phone", "CN"), None);
        assert_eq!(normalize_phone("", "CN"), None);
        assert_eq!(
            normalize_phone("1380013800000000000000000000000000", "CN"),
            None
        );
        assert_eq!(normalize_phone("13800138000", "ZZ"), None);
    }

    /// WHY: PRD §4.1 — the mask display is backend-derived only, and short
    /// NSNs must degrade without panicking on slicing.
    #[test]
    fn mask_phone_hides_the_middle_of_the_national_number() {
        assert_eq!(mask_phone("+8613800138000").as_deref(), Some("138****8000"));
        assert_eq!(mask_phone("+12125551234").as_deref(), Some("212****1234"));
        assert_eq!(mask_phone("+8521234567").as_deref(), Some("****67"));
        // 8-digit NSN markets (HK/MO/SG) stay tail-only: first-3+last-4
        // would leave only 1 digit hidden.
        assert_eq!(mask_phone("+85291234567").as_deref(), Some("****67"));
        assert_eq!(mask_phone("garbage"), None);
    }

    /// WHY: an unresolvable default region would dead-end every normalize
    /// call in the realm.
    #[test]
    fn country_code_check_requires_resolvable_alpha2() {
        assert!(is_supported_country_code("CN"));
        assert!(is_supported_country_code("US"));
        assert!(!is_supported_country_code("cn"), "lowercase is rejected");
        assert!(!is_supported_country_code("CHN"), "alpha-3 is rejected");
        assert!(!is_supported_country_code("1A"));
        assert!(!is_supported_country_code(""));
    }
}
