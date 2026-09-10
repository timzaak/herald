use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use herald_domain::billing::{ProviderApiPort, ProviderPrice, ProviderProduct};
use herald_domain::common::entities::app_errors::CoreError;
use serde_json::Value;
use sqlx::PgPool;

/// Coerce a provider `metadata` JSON value into a strict string→string map.
///
/// Both Stripe and Creem model metadata as `HashMap<String, String>`, and
/// Stripe coerces every value to a string server-side. This mirrors that: a
/// non-object / empty value → `None`; string values pass through; numbers /
/// bools / null become their string form; nested objects / arrays become a
/// compact JSON string (lossless). The result is what flows into the
/// `provider_product_info` JSONB, so the JSONB is guaranteed string→string.
fn coerce_metadata_to_string_map(raw: &Value) -> Option<HashMap<String, String>> {
    let obj = raw.as_object()?;
    if obj.is_empty() {
        return None;
    }
    let mut out = HashMap::with_capacity(obj.len());
    for (k, v) in obj {
        out.insert(k.clone(), coerce_value_to_string(v));
    }
    Some(out)
}

/// Render a single metadata value as a display string (see
/// [`coerce_metadata_to_string_map`]).
fn coerce_value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        // serde_json renders numbers/bools bare and containers as compact JSON.
        other => other.to_string(),
    }
}

#[derive(Clone)]
pub struct ConfiguredProviderProductApi {
    pool: PgPool,
    http: reqwest::Client,
}

impl ConfiguredProviderProductApi {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            http: reqwest::Client::new(),
        }
    }

    async fn load_provider_config(
        &self,
        realm_id: &str,
        payment_provider: &str,
    ) -> Result<(String, String), CoreError> {
        let api_key = sqlx::query_scalar::<_, String>(
            "SELECT config_value
             FROM realm_config
             WHERE realm_id = $1 AND config_type = $2 AND config_key = 'api_key' AND enabled = true
             LIMIT 1",
        )
        .bind(realm_id)
        .bind(payment_provider)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?
        .ok_or_else(|| {
            CoreError::BadRequest(format!(
                "{} is not configured for realm {}",
                payment_provider, realm_id
            ))
        })?;

        let base_url = match payment_provider {
            "stripe" => "https://api.stripe.com".to_string(),
            "creem" => {
                if api_key.starts_with("ck_test_") || api_key.starts_with("creem_test_") {
                    "https://test-api.creem.io".to_string()
                } else {
                    "https://api.creem.io".to_string()
                }
            }
            _ => String::new(),
        };

        Ok((api_key, base_url))
    }

    async fn fetch_stripe_products(
        &self,
        realm_id: &str,
    ) -> Result<Vec<ProviderProduct>, CoreError> {
        let (api_key, base_url) = self.load_provider_config(realm_id, "stripe").await?;

        let products_url = format!("{}/v1/products?active=true&limit=100", base_url);
        let response = self
            .http
            .get(products_url)
            .bearer_auth(&api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CoreError::InternalServerError(format!(
                "Stripe product sync failed: {} - {}",
                status.as_u16(),
                text
            )));
        }

        let body: Value = response.json().await?;
        let mut synced = Vec::new();

        for product in body["data"].as_array().into_iter().flatten() {
            let Some(product_id) = product["id"].as_str() else {
                continue;
            };

            // Collect ALL active prices for this product. Stripe
            // exposes one Price per (currency, billing period); each becomes its
            // own ProviderPrice variant. We deliberately do NOT collapse to a
            // "first price" — multi-price products must yield one mapping row per
            // price.
            let prices_url = format!(
                "{}/v1/prices?active=true&limit=100&product={}",
                base_url, product_id
            );
            let prices_response = self.http.get(prices_url).bearer_auth(&api_key).send().await;

            let prices: Vec<ProviderPrice> = match prices_response {
                Ok(res) if res.status().is_success() => {
                    let prices_json = res.json::<Value>().await.unwrap_or(Value::Null);
                    prices_json["data"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|price| {
                            let is_recurring = price["recurring"].is_object();
                            let price_metadata = coerce_metadata_to_string_map(&price["metadata"]);
                            ProviderPrice {
                                external_price_id: price["id"].as_str().map(str::to_string),
                                price: price["unit_amount"].as_i64(),
                                currency: price["currency"].as_str().map(str::to_string),
                                billing_type: Some(
                                    if is_recurring {
                                        "recurring"
                                    } else {
                                        "one_time"
                                    }
                                    .to_string(),
                                ),
                                billing_period: price["recurring"]["interval"]
                                    .as_str()
                                    .map(str::to_string),
                                price_metadata,
                            }
                        })
                        .collect()
                }
                _ => Vec::new(),
            };

            synced.push(ProviderProduct {
                external_product_id: product_id.to_string(),
                name: product["name"].as_str().unwrap_or(product_id).to_string(),
                description: product["description"].as_str().map(str::to_string),
                product_metadata: coerce_metadata_to_string_map(&product["metadata"]),
                prices,
            });
        }

        Ok(synced)
    }

    async fn fetch_creem_products(
        &self,
        realm_id: &str,
    ) -> Result<Vec<ProviderProduct>, CoreError> {
        let (api_key, base_url) = self.load_provider_config(realm_id, "creem").await?;

        let response = self
            .http
            .get(format!(
                "{}/v1/products/search?page_number=1&page_size=100",
                base_url
            ))
            .header("x-api-key", api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CoreError::InternalServerError(format!(
                "Creem product sync failed: {} - {}",
                status.as_u16(),
                text
            )));
        }

        let body: Value = response.json().await?;
        // Creem is product-level only: it has no Stripe-style Price object and no
        // price id, so `external_price_id` is always `None`. When the API exposes
        // real price fields (price/currency/billing_type/billing_period) we emit a
        // single `ProviderPrice` carrying them; when ALL four are absent we emit an
        // empty `prices` vec, and the sync service falls back to a single NULL-price
        // row (`external_price_id = NULL`), which dedups via the NULLS NOT DISTINCT
        // unique constraint. We do NOT synthesize a placeholder price id.
        let products = body
            .get("items")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|product| {
                let id = product["id"].as_str()?;

                let price = product["price"].as_i64();
                let currency = product["currency"].as_str().map(str::to_string);
                // Creem reports native word forms ("onetime"/"subscription");
                // map them onto the canonical BillingType wire strings so the
                // domain parse in the sync loop succeeds. An unrecognized form
                // passes through verbatim — the sync loop then preserves the
                // mapping's existing billing_type instead of storing NULL.
                let billing_type = product["billing_type"].as_str().map(|raw| {
                    herald_domain::billing::normalize_creem_billing_type_word_form(raw)
                        .map(|billing_type| billing_type.as_str().to_string())
                        .unwrap_or_else(|| raw.to_string())
                });
                let billing_period = product["billing_period"].as_str().map(str::to_string);

                // Only emit a real ProviderPrice when at least one of the four
                // price fields is present; otherwise fall back to the empty-prices
                // (NULL_PRICE) path.
                let prices = if price.is_none()
                    && currency.is_none()
                    && billing_type.is_none()
                    && billing_period.is_none()
                {
                    Vec::new()
                } else {
                    vec![ProviderPrice {
                        external_price_id: None,
                        price,
                        currency,
                        billing_type,
                        billing_period,
                        price_metadata: None,
                    }]
                };

                Some(ProviderProduct {
                    external_product_id: id.to_string(),
                    name: product["name"].as_str().unwrap_or(id).to_string(),
                    description: product["description"].as_str().map(str::to_string),
                    product_metadata: None,
                    prices,
                })
            })
            .collect();

        Ok(products)
    }
}

impl ProviderApiPort for ConfiguredProviderProductApi {
    fn fetch_products(
        &self,
        realm_id: &str,
        payment_provider: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ProviderProduct>, CoreError>> + Send + '_>> {
        let realm_id = realm_id.to_string();
        let payment_provider = payment_provider.to_string();
        Box::pin(async move {
            match payment_provider.as_str() {
                "stripe" => self.fetch_stripe_products(&realm_id).await,
                "creem" => self.fetch_creem_products(&realm_id).await,
                // WeChat Pay v3 is order-based and has no hosted product
                // catalogue (DEC-wechat-support-006): skip sync as a harmless
                // no-op so catalogue refresh does not error.
                "wechat" => Ok(Vec::new()),
                other => Err(CoreError::BadRequest(format!(
                    "Provider product sync is not supported for {}",
                    other
                ))),
            }
        })
    }
}
