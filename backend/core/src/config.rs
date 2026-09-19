use std::fs;

#[derive(serde::Deserialize)]
pub struct AppConfig {
    pub postgresql_uri: String,
    pub redis_uri: String,
    pub turnstile: TurnstileConfig,
    #[serde(default)]
    pub public_base_url: String,
    #[serde(default)]
    pub permission: PermissionConfig,
}

#[derive(serde::Deserialize, Default, Clone)]
pub struct PermissionConfig {
    #[serde(default)]
    pub allowed_ips: Vec<String>,
}

#[derive(serde::Deserialize)]
pub struct TurnstileConfig {
    pub secret: String,
}

/// Production-environment predicate shared by every production-only security
/// gate (secret validation, first-boot admin password, payment-provider
/// base_url rejection, OAuth redirect HTTPS enforcement, rate limiting).
/// "prod" is recognized case-insensitively as an alias — the same alias set
/// the Turnstile test-secret gate uses — so a deployment spelling the value
/// "prod" cannot silently skip those gates.
pub fn is_production(app_env: &str) -> bool {
    app_env.eq_ignore_ascii_case("production") || app_env.eq_ignore_ascii_case("prod")
}

impl AppConfig {
    /// Loads application configuration from a file
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the configuration file (TOML format)
    ///
    /// # Returns
    ///
    /// Returns `Ok(AppConfig)` if the configuration was loaded successfully
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The configuration file does not exist
    /// - The file is not valid TOML format
    /// - Required fields are missing
    /// - Environment variable expansion fails
    pub fn load(path: &str) -> anyhow::Result<AppConfig> {
        let config = fs::read_to_string(path)?;
        let mut cfg: AppConfig = toml::from_str(&config)?;
        if cfg.public_base_url.is_empty() {
            cfg.public_base_url = "http://localhost:8080".to_string();
        }
        Ok(cfg)
    }
}
