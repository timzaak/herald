use uuid::Uuid;

use crate::billing::{BillingType, EntitlementMapping};
use crate::common::entities::app_errors::CoreError;

/// Validate an ISO 4217 currency code on write paths.
///
/// Format check only (`^[A-Z]{3}$`) plus rejection of the reserved codes
/// `XXX` (no currency) and `XTS` (testing). No full ISO 4217 dictionary is
/// maintained: a syntactically valid code with no matching price row fails
/// loudly at resolution time, so dictionary-level validation is unnecessary.
pub fn validate_currency_code(code: &str) -> Result<(), CoreError> {
    if code.len() != 3 || !code.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(CoreError::BadRequest(format!(
            "invalid currency code: {code}"
        )));
    }
    if matches!(code, "XXX" | "XTS") {
        return Err(CoreError::BadRequest(format!(
            "reserved currency code: {code}"
        )));
    }
    Ok(())
}

/// Failure modes of programmatic default-price resolution. Both are
/// fail-loud on purpose: no secondary-currency fallback, no silent
/// substitution, so callers never charge an unintended currency or amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrencyResolveError {
    /// No enabled Stripe mapping row matches the requested currency.
    NotFound {
        entitlement_key: String,
        currency: String,
    },
    /// Multiple rows match the currency (+ optional billing filters); the
    /// caller must narrow by billing type/period.
    Ambiguous {
        entitlement_key: String,
        currency: String,
        count: usize,
    },
}

/// A single price row resolved by currency, ready to be used as an explicit
/// `target_id` purchase target by third-party callers.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPriceRow {
    pub mapping_id: Uuid,
    pub entitlement_key: String,
    pub payment_provider: String,
    pub currency: String,
    pub amount: i64,
    pub billing_type: Option<BillingType>,
    pub billing_period: Option<String>,
    pub external_price_id: Option<String>,
}

/// Read a mapping row's stored currency exactly as stored (Stripe sync writes
/// lowercase like `"usd"`; WeChat manual config writes the validated uppercase
/// form). Handlers that mirror the stored value to API consumers use this;
/// resolution code uses [`stored_currency`] for the normalized form.
pub fn mapping_currency(m: &EntitlementMapping) -> Option<&str> {
    m.provider_product_info
        .as_ref()
        .and_then(|info| info.get("currency"))
        .and_then(|v| v.as_str())
}

/// Read a mapping row's stored currency, ASCII-upper-cased.
///
/// Providers store lowercase codes (Stripe: `"usd"`); resolution inputs and
/// outputs use uppercase ISO 4217 codes. Matching is case-insensitive and the
/// reported currency is normalized to the uppercase form.
fn stored_currency(m: &EntitlementMapping) -> Option<String> {
    mapping_currency(m).map(|c| c.to_ascii_uppercase())
}

/// Resolve the unique default price row for an entitlement by currency.
///
/// `mappings` must already be scoped to the realm, the entitlement key, and
/// enabled Stripe rows. Currency is a filter dimension, not a unique key:
/// the same currency can carry multiple billing periods, so optional
/// billing filters narrow the candidates. Exactly one remaining row resolves;
/// zero rows fail `NotFound`, more than one fail `Ambiguous`. Matching against
/// the stored code is ASCII-case-insensitive (Stripe stores `"usd"`).
pub fn resolve_price_row(
    mappings: &[EntitlementMapping],
    entitlement_key: &str,
    currency: &str,
    billing_type: Option<&BillingType>,
    billing_period: Option<&str>,
) -> Result<ResolvedPriceRow, CurrencyResolveError> {
    let wanted = currency.to_ascii_uppercase();
    let candidates: Vec<&EntitlementMapping> = mappings
        .iter()
        .filter(|m| stored_currency(m).as_deref() == Some(wanted.as_str()))
        .filter(|m| billing_type.is_none_or(|bt| m.billing_type.as_ref() == Some(bt)))
        .filter(|m| billing_period.is_none_or(|period| m.billing_period.as_deref() == Some(period)))
        .collect();

    match candidates.len() {
        0 => Err(CurrencyResolveError::NotFound {
            entitlement_key: entitlement_key.to_string(),
            currency: wanted,
        }),
        1 => {
            let m = candidates[0];
            let info = m.provider_product_info.as_ref();
            let amount = info
                .and_then(|i| i.get("price"))
                .and_then(|v| v.as_i64())
                .ok_or_else(|| CurrencyResolveError::NotFound {
                    entitlement_key: entitlement_key.to_string(),
                    currency: wanted.clone(),
                })?;
            Ok(ResolvedPriceRow {
                mapping_id: m.id,
                entitlement_key: m.entitlement_key.clone(),
                payment_provider: m.payment_provider.clone(),
                currency: wanted,
                amount,
                billing_type: m.billing_type,
                billing_period: m.billing_period.clone(),
                external_price_id: m.external_price_id.clone(),
            })
        }
        n => Err(CurrencyResolveError::Ambiguous {
            entitlement_key: entitlement_key.to_string(),
            currency: wanted,
            count: n,
        }),
    }
}

/// Collect the deduplicated set of currencies covered by enabled Stripe
/// mapping rows, normalized to uppercase ISO 4217 form. Rows whose product
/// info carries no currency are skipped.
pub fn collect_currencies(mappings: &[EntitlementMapping]) -> Vec<String> {
    let mut currencies: Vec<String> = Vec::new();
    for m in mappings {
        if let Some(code) = stored_currency(m)
            && !currencies.contains(&code)
        {
            currencies.push(code);
        }
    }
    currencies
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn mapping(
        id: u16,
        currency: &str,
        price: i64,
        billing_type: Option<BillingType>,
        billing_period: Option<&str>,
    ) -> EntitlementMapping {
        let now = Utc::now();
        EntitlementMapping {
            id: Uuid::now_v7(),
            realm_id: "realm".to_string(),
            payment_provider: "stripe".to_string(),
            external_product_id: "prod".to_string(),
            external_price_id: Some(format!("price_{id}")),
            entitlement_key: "pro".to_string(),
            billing_type,
            billing_period: billing_period.map(|p| p.to_string()),
            service_duration_days: None,
            enabled: true,
            provider_product_info: Some(serde_json::json!({
                "price": price,
                "currency": currency,
            })),
            granted_role_ids: Vec::new(),
            synced_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    // A row that matches the currency but carries no readable price must not
    // resolve: returning it would hand the caller a target that fails at
    // purchase creation anyway. Fail loud here instead.
    #[test]
    fn resolves_unique_currency_row_with_amount() {
        let mappings = vec![
            mapping(1, "USD", 1000, Some(BillingType::Recurring), Some("month")),
            mapping(2, "EUR", 900, Some(BillingType::Recurring), Some("month")),
        ];
        let row = resolve_price_row(&mappings, "pro", "EUR", None, None).unwrap();
        assert_eq!(row.currency, "EUR");
        assert_eq!(row.amount, 900);
        assert_eq!(row.external_price_id.as_deref(), Some("price_2"));
    }

    // Programmatic default resolution has no secondary-currency fallback:
    // zero matches must be a loud NotFound, never a silent switch to
    // whatever other currencies exist.
    #[test]
    fn no_match_fails_not_found_without_fallback() {
        let mappings = vec![mapping(1, "USD", 1000, Some(BillingType::Recurring), None)];
        let err = resolve_price_row(&mappings, "pro", "EUR", None, None).unwrap_err();
        assert_eq!(
            err,
            CurrencyResolveError::NotFound {
                entitlement_key: "pro".to_string(),
                currency: "EUR".to_string()
            }
        );
    }

    // Currency is a filter, not a unique key: same currency with multiple
    // billing periods must be Ambiguous unless the caller narrows by a
    // billing dimension — silently picking one would charge the wrong period.
    #[test]
    fn same_currency_multiple_periods_is_ambiguous_then_narrowed() {
        let mappings = vec![
            mapping(1, "USD", 1000, Some(BillingType::Recurring), Some("month")),
            mapping(2, "USD", 10000, Some(BillingType::Recurring), Some("year")),
        ];
        let err = resolve_price_row(&mappings, "pro", "USD", None, None).unwrap_err();
        assert!(matches!(
            err,
            CurrencyResolveError::Ambiguous { count: 2, .. }
        ));

        let row = resolve_price_row(&mappings, "pro", "USD", None, Some("year")).unwrap();
        assert_eq!(row.amount, 10000);
    }

    #[test]
    fn billing_type_filter_narrows_candidates() {
        let mappings = vec![
            mapping(1, "USD", 500, Some(BillingType::OneTime), None),
            mapping(2, "USD", 1000, Some(BillingType::Recurring), None),
        ];
        let row =
            resolve_price_row(&mappings, "pro", "USD", Some(&BillingType::OneTime), None).unwrap();
        assert_eq!(row.amount, 500);
    }

    #[test]
    fn row_without_price_info_fails_not_found() {
        let mut m = mapping(1, "USD", 1000, None, None);
        m.provider_product_info = Some(serde_json::json!({"currency": "USD"}));
        let err = resolve_price_row(&[m], "pro", "USD", None, None).unwrap_err();
        assert!(matches!(err, CurrencyResolveError::NotFound { .. }));
    }

    #[test]
    fn collect_currencies_dedupes_and_skips_missing() {
        let mappings = vec![
            mapping(1, "USD", 1000, None, None),
            mapping(2, "USD", 2000, None, None),
            mapping(3, "EUR", 900, None, None),
        ];
        assert_eq!(collect_currencies(&mappings), vec!["USD", "EUR"]);
    }

    // Provider sync stores Stripe's lowercase codes ("usd"); callers pass
    // uppercase ISO codes ("USD"). Resolution must bridge the case gap and
    // report the normalized uppercase form.
    #[test]
    fn matches_lowercase_stored_currency_case_insensitively() {
        let mappings = vec![mapping(1, "usd", 1000, Some(BillingType::Recurring), None)];
        let row = resolve_price_row(&mappings, "pro", "USD", None, None).unwrap();
        assert_eq!(row.currency, "USD");
        assert_eq!(row.amount, 1000);
        assert_eq!(
            collect_currencies(&mappings),
            vec!["USD".to_string()],
            "lowercase stored codes must surface uppercase"
        );
    }

    #[test]
    fn validate_currency_code_accepts_and_rejects() {
        assert!(validate_currency_code("USD").is_ok());
        assert!(validate_currency_code("CNY").is_ok());
        // lowercase / wrong length / non-ascii all rejected
        assert!(validate_currency_code("usd").is_err());
        assert!(validate_currency_code("US").is_err());
        assert!(validate_currency_code("USDD").is_err());
        assert!(validate_currency_code("美元").is_err());
        assert!(validate_currency_code("Us1").is_err());
        // ISO 4217 reserved codes must never be accepted
        assert!(validate_currency_code("XXX").is_err());
        assert!(validate_currency_code("XTS").is_err());
    }
}
