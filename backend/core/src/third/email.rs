use std::future::Future;

use herald_domain::telemetry::external_http::timed_external_http_span;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// EmailProvider trait
// ---------------------------------------------------------------------------

/// Trait abstracting email delivery backends.
///
/// Implementors must be `Send + Sync` so they can be stored in shared state.
pub trait EmailProvider: Send + Sync {
    fn send(
        &self,
        to: &str,
        subject: &str,
        text: &str,
        html: &str,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
}

#[derive(Debug, Clone, Copy)]
pub enum EmailTemplateKind {
    VerifyEmail,
    ResetPassword,
    ChangeEmail,
}

impl EmailTemplateKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::VerifyEmail => "verify_email",
            Self::ResetPassword => "reset_password",
            Self::ChangeEmail => "change_email",
        }
    }
}

#[derive(Debug, Deserialize)]
struct StoredEmailTemplate {
    subject: String,
    text: String,
    html: String,
}

struct RenderedEmail {
    subject: String,
    text: String,
    html: String,
}

// ---------------------------------------------------------------------------
// ResendClient (HTTP-based, wraps existing Resend API logic)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ResendClient {
    token: String,
    from: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct SendEmailRequest<'a> {
    from: &'a str,
    to: Vec<&'a str>,
    subject: &'a str,
    text: &'a str,
    html: &'a str,
}

impl ResendClient {
    pub fn new(token: String, from: String) -> Self {
        Self {
            token,
            from,
            http: reqwest::Client::new(),
        }
    }
}

/// Sends an HTML email via the Resend API.
///
/// # Errors
///
/// Returns an error if the API request fails or returns a non-success status.
impl EmailProvider for ResendClient {
    async fn send(&self, to: &str, subject: &str, text: &str, html: &str) -> anyhow::Result<()> {
        let body = SendEmailRequest {
            from: &self.from,
            to: vec![to],
            subject,
            text,
            html,
        };

        // external.http span + duration histogram. Host-only
        // (no path, no bearer token, no email HTML body) per governance.
        const RESEND_BASE: &str = "https://api.resend.com";
        let timing = timed_external_http_span(RESEND_BASE, "POST");
        let _span_enter = timing.span().enter();

        let resp = self
            .http
            .post("https://api.resend.com/emails")
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("resend send failed: {status} {text}");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SmtpEmailProvider (uses lettre crate)
// ---------------------------------------------------------------------------

/// TLS mode for SMTP connections.
pub enum SmtpEncryption {
    /// Upgrade the connection with STARTTLS after connecting (port 587)
    StartTls,
    /// Use implicit TLS from the start (port 465)
    Ssl,
}

pub struct SmtpEmailProvider {
    host: String,
    port: u16,
    username: String,
    password: String,
    encryption: SmtpEncryption,
    from_address: String,
}

impl SmtpEmailProvider {
    pub fn new(
        host: String,
        port: u16,
        username: String,
        password: String,
        encryption: SmtpEncryption,
        from_address: String,
    ) -> Self {
        Self {
            host,
            port,
            username,
            password,
            encryption,
            from_address,
        }
    }
}

/// Sends an HTML email via SMTP using the `lettre` crate.
///
/// # Errors
///
/// Returns an error if the message cannot be built or the SMTP delivery fails.
impl EmailProvider for SmtpEmailProvider {
    async fn send(&self, to: &str, subject: &str, text: &str, html: &str) -> anyhow::Result<()> {
        use lettre::message::{Mailbox, MultiPart};
        use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

        let from_mailbox: Mailbox = self.from_address.parse()?;
        let to_mailbox: Mailbox = to.parse()?;

        let email = Message::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(subject)
            .multipart(MultiPart::alternative_plain_html(
                text.to_string(),
                html.to_string(),
            ))?;

        let creds = lettre::transport::smtp::authentication::Credentials::new(
            self.username.clone(),
            self.password.clone(),
        );

        let mailer: AsyncSmtpTransport<Tokio1Executor> = match self.encryption {
            SmtpEncryption::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.host)?
                    .port(self.port)
                    .credentials(creds)
                    .build()
            }
            SmtpEncryption::Ssl => AsyncSmtpTransport::<Tokio1Executor>::relay(&self.host)?
                .port(self.port)
                .credentials(creds)
                .build(),
        };

        mailer.send(email).await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// EmailProviderKind (enum dispatch to avoid dyn)
// ---------------------------------------------------------------------------

/// Enum-dispatched email provider, avoiding the need for `Box<dyn EmailProvider>`.
pub enum EmailProviderKind {
    Resend(ResendClient),
    Smtp(SmtpEmailProvider),
}

impl EmailProvider for EmailProviderKind {
    async fn send(&self, to: &str, subject: &str, text: &str, html: &str) -> anyhow::Result<()> {
        match self {
            Self::Resend(p) => p.send(to, subject, text, html).await,
            Self::Smtp(p) => p.send(to, subject, text, html).await,
        }
    }
}

// ---------------------------------------------------------------------------
// EmailConfig — parsed realm email configuration
// ---------------------------------------------------------------------------

/// Parsed email configuration extracted from realm_config key-value entries.
pub struct EmailConfig {
    /// Provider type: "resend" or "smtp"
    pub provider: String,
    /// Default sender address
    pub from_address: String,
    /// Resend API key (only set when provider == "resend")
    pub resend_api_key: Option<String>,
    /// SMTP host (only set when provider == "smtp")
    pub smtp_host: Option<String>,
    /// SMTP port (only set when provider == "smtp")
    pub smtp_port: Option<u16>,
    /// SMTP username (only set when provider == "smtp")
    pub smtp_username: Option<String>,
    /// SMTP password (only set when provider == "smtp")
    pub smtp_password: Option<String>,
    /// SMTP encryption mode: "starttls" or "ssl" (only set when provider == "smtp")
    pub smtp_encryption: Option<String>,
}

impl EmailConfig {
    /// Constructs the appropriate `EmailProviderKind` from this configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields for the chosen provider are missing.
    pub fn build_provider(&self) -> anyhow::Result<EmailProviderKind> {
        match self.provider.as_str() {
            "resend" => {
                let api_key = self.resend_api_key.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("resend_api_key is required for Resend provider")
                })?;
                Ok(EmailProviderKind::Resend(ResendClient::new(
                    api_key.to_string(),
                    self.from_address.clone(),
                )))
            }
            "smtp" => {
                let host = self
                    .smtp_host
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("smtp_host is required for SMTP provider"))?;
                let port = self
                    .smtp_port
                    .ok_or_else(|| anyhow::anyhow!("smtp_port is required for SMTP provider"))?;
                let username = self.smtp_username.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("smtp_username is required for SMTP provider")
                })?;
                let password = self.smtp_password.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("smtp_password is required for SMTP provider")
                })?;
                let encryption = match self.smtp_encryption.as_deref().unwrap_or("starttls") {
                    "ssl" => SmtpEncryption::Ssl,
                    _ => SmtpEncryption::StartTls,
                };
                Ok(EmailProviderKind::Smtp(SmtpEmailProvider::new(
                    host.to_string(),
                    port,
                    username.to_string(),
                    password.to_string(),
                    encryption,
                    self.from_address.clone(),
                )))
            }
            other => anyhow::bail!("unknown email provider: {other}"),
        }
    }
}

// ---------------------------------------------------------------------------
// EmailConfigStatus — result of checking email configuration completeness
// ---------------------------------------------------------------------------

/// Status of email configuration for a realm.
#[derive(Debug, Clone, Serialize)]
pub struct EmailConfigStatus {
    /// Whether the email configuration is complete and usable.
    pub configured: bool,
    /// Provider type if set (e.g., "resend" or "smtp").
    pub provider: Option<String>,
    /// Sender address if set.
    pub from_address: Option<String>,
    /// List of config keys that are missing or empty.
    pub missing_fields: Vec<String>,
}

// ---------------------------------------------------------------------------
// EmailService — reads per-realm email config from realm_config, sends email
// ---------------------------------------------------------------------------

/// Stateless service for reading email configuration from `realm_config` and
/// sending emails through the appropriate provider.
pub struct EmailService;

impl EmailService {
    pub async fn send_templated_email(
        pool: &PgPool,
        realm_id: &str,
        to: &str,
        kind: EmailTemplateKind,
        action_url: &str,
        locale: Option<&str>,
    ) -> anyhow::Result<()> {
        let message =
            Self::render_email(pool, realm_id, kind, action_url, locale.unwrap_or("en")).await?;
        Self::send_email(
            pool,
            realm_id,
            to,
            &message.subject,
            &message.text,
            &message.html,
        )
        .await
    }

    async fn render_email(
        pool: &PgPool,
        realm_id: &str,
        kind: EmailTemplateKind,
        action_url: &str,
        locale: &str,
    ) -> anyhow::Result<RenderedEmail> {
        let brand_name = resolve_realm_brand(pool, realm_id).await?;

        let localized_type = format!("{}:{locale}", kind.as_str());
        let stored = sqlx::query_scalar::<_, String>(
            "SELECT content FROM email_templates
             WHERE (realm_id = $1 OR realm_id IS NULL) AND type IN ($2, $3)
             ORDER BY COALESCE(realm_id = $1, false) DESC,
                      (type = $2) DESC,
                      updated_at DESC
             LIMIT 1",
        )
        .bind(realm_id)
        .bind(&localized_type)
        .bind(kind.as_str())
        .fetch_optional(pool)
        .await?;

        let template = match stored {
            Some(content) => {
                serde_json::from_str::<StoredEmailTemplate>(&content).map_err(|error| {
                    anyhow::anyhow!("invalid {} email template: {error}", kind.as_str())
                })?
            }
            None => default_email_template(kind),
        };
        validate_template_variables(&template.subject)?;
        validate_template_variables(&template.text)?;
        validate_template_variables(&template.html)?;

        let subject_brand = brand_name.replace(['\r', '\n'], " ");
        let subject = render_template(&template.subject, &subject_brand, action_url);
        let text = render_template(&template.text, &brand_name, action_url);
        let html = render_template(
            &template.html,
            &escape_html(&brand_name),
            &escape_html(action_url),
        );
        Ok(RenderedEmail {
            subject,
            text,
            html,
        })
    }

    /// Read email configuration key-value pairs from `realm_config` for a realm.
    ///
    /// Returns `Ok(None)` if no rows with `config_type = 'email'` exist.
    async fn read_email_config(
        pool: &PgPool,
        realm_id: &str,
    ) -> anyhow::Result<Option<EmailConfig>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT config_key, config_value FROM realm_config
             WHERE realm_id = $1 AND config_type = 'email'",
        )
        .bind(realm_id)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            return Ok(None);
        }

        let mut cfg = EmailConfig {
            provider: String::new(),
            from_address: String::new(),
            resend_api_key: None,
            smtp_host: None,
            smtp_port: None,
            smtp_username: None,
            smtp_password: None,
            smtp_encryption: None,
        };

        for (key, value) in rows {
            match key.as_str() {
                "provider" => cfg.provider = value,
                "from_address" => cfg.from_address = value,
                "resend_api_key" => cfg.resend_api_key = Some(value),
                "smtp_host" => cfg.smtp_host = Some(value),
                "smtp_port" => {
                    cfg.smtp_port = value.parse::<u16>().ok();
                }
                "smtp_username" => cfg.smtp_username = Some(value),
                "smtp_password" => cfg.smtp_password = Some(value),
                "smtp_encryption" => cfg.smtp_encryption = Some(value),
                _ => {} // ignore unknown keys
            }
        }

        Ok(Some(cfg))
    }

    /// Check whether email is fully configured for a realm.
    ///
    /// Returns a detailed status including which required fields are missing.
    pub async fn is_email_configured(
        pool: &PgPool,
        realm_id: &str,
    ) -> anyhow::Result<EmailConfigStatus> {
        let cfg = Self::read_email_config(pool, realm_id).await?;

        let Some(cfg) = cfg else {
            return Ok(EmailConfigStatus {
                configured: false,
                provider: None,
                from_address: None,
                missing_fields: vec!["provider".to_string(), "from_address".to_string()],
            });
        };

        let provider_str = if cfg.provider.is_empty() {
            None
        } else {
            Some(cfg.provider.clone())
        };

        let from_address_str = if cfg.from_address.is_empty() {
            None
        } else {
            Some(cfg.from_address.clone())
        };

        let mut missing = Vec::new();

        // Common required fields
        if cfg.provider.is_empty() {
            missing.push("provider".to_string());
        }
        if cfg.from_address.is_empty() {
            missing.push("from_address".to_string());
        }

        // Provider-specific required fields
        match cfg.provider.as_str() {
            "resend" if cfg.resend_api_key.as_deref().unwrap_or("").is_empty() => {
                missing.push("resend_api_key".to_string());
            }
            "resend" => {}
            "smtp" => {
                if cfg.smtp_host.as_deref().unwrap_or("").is_empty() {
                    missing.push("smtp_host".to_string());
                }
                if cfg.smtp_port.is_none() {
                    missing.push("smtp_port".to_string());
                }
                if cfg.smtp_username.as_deref().unwrap_or("").is_empty() {
                    missing.push("smtp_username".to_string());
                }
                if cfg.smtp_password.as_deref().unwrap_or("").is_empty() {
                    missing.push("smtp_password".to_string());
                }
            }
            _ => {
                // Unknown provider values can never send mail; without this
                // a typo'd provider would report the realm as configured.
                missing.push("provider".to_string());
            }
        }

        let configured = missing.is_empty();

        Ok(EmailConfigStatus {
            configured,
            provider: provider_str,
            from_address: from_address_str,
            missing_fields: missing,
        })
    }

    /// Send a multipart plain-text and HTML email for a realm.
    ///
    /// Reads the realm's email configuration, builds the appropriate provider,
    /// and sends the email. Returns `Ok(())` silently when email is not
    /// configured for the realm (callers can ignore the result).
    /// Returns `Err` on send failure so callers can decide propagation.
    pub async fn send_email(
        pool: &PgPool,
        realm_id: &str,
        to: &str,
        subject: &str,
        text: &str,
        html: &str,
    ) -> anyhow::Result<()> {
        let cfg = Self::read_email_config(pool, realm_id).await?;

        let Some(cfg) = cfg else {
            // Not configured — silently skip.
            return Ok(());
        };

        // Basic sanity: need at least provider and from_address to attempt sending.
        if cfg.provider.is_empty() || cfg.from_address.is_empty() {
            return Ok(());
        }

        let provider = cfg.build_provider()?;
        provider.send(to, subject, text, html).await?;

        Ok(())
    }
}

fn default_email_template(kind: EmailTemplateKind) -> StoredEmailTemplate {
    let (subject, action) = match kind {
        EmailTemplateKind::VerifyEmail => ("Verify your email for {{brand_name}}", "Verify email"),
        EmailTemplateKind::ResetPassword => {
            ("Reset your {{brand_name}} password", "Reset password")
        }
        EmailTemplateKind::ChangeEmail => (
            "Confirm your email change for {{brand_name}}",
            "Confirm email change",
        ),
    };
    StoredEmailTemplate {
        subject: subject.to_string(),
        text: format!("{{{{brand_name}}}}\n\n{action}: {{{{action_url}}}}"),
        html: format!(
            "<p>{{{{brand_name}}}}</p><p><a href=\"{{{{action_url}}}}\">{action}</a></p>"
        ),
    }
}

fn validate_template_variables(template: &str) -> anyhow::Result<()> {
    let mut remainder = template;
    while let Some(start) = remainder.find("{{") {
        let after = &remainder[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| anyhow::anyhow!("unclosed email template variable"))?;
        let variable = after[..end].trim();
        if !matches!(variable, "brand_name" | "action_url") {
            anyhow::bail!("unsupported email template variable: {variable}");
        }
        remainder = &after[end + 2..];
    }
    Ok(())
}

/// Resolve the display brand name for a realm: the white-label `brandName`
/// when set and non-empty, else the realm's `name`, else `"Herald"`. Shared by
/// the templated email renderer and the email-OTP sender (see `api-auth`) so
/// the two share one source of truth for the brand.
pub async fn resolve_realm_brand(pool: &PgPool, realm_id: &str) -> anyhow::Result<String> {
    let realm_name = sqlx::query_scalar::<_, String>("SELECT name FROM realm WHERE id = $1")
        .bind(realm_id)
        .fetch_optional(pool)
        .await?
        .unwrap_or_else(|| "Herald".to_string());
    let config = sqlx::query_scalar::<_, String>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'white_label'
           AND config_key = 'settings' AND enabled = true",
    )
    .bind(realm_id)
    .fetch_optional(pool)
    .await?;
    let brand_name = config
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|value| {
            value
                .get("brandName")
                .and_then(|name| name.as_str())
                .map(str::to_owned)
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(realm_name);
    Ok(brand_name)
}

fn render_template(template: &str, brand_name: &str, action_url: &str) -> String {
    let mut output = String::with_capacity(template.len());
    let mut remainder = template;
    while let Some(start) = remainder.find("{{") {
        output.push_str(&remainder[..start]);
        let after = &remainder[start + 2..];
        let Some(end) = after.find("}}") else {
            output.push_str(&remainder[start..]);
            return output;
        };
        output.push_str(match after[..end].trim() {
            "brand_name" => brand_name,
            "action_url" => action_url,
            _ => "",
        });
        remainder = &after[end + 2..];
    }
    output.push_str(remainder);
    output
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn render_template_supports_whitespace_without_leaking_template_syntax() {
        let rendered = render_template(
            "{{  brand_name }}: {{ action_url }}",
            "Acme",
            "https://example.test/action",
        );
        assert_eq!(rendered, "Acme: https://example.test/action");
    }

    #[test]
    fn validate_template_rejects_unknown_and_unclosed_variables() {
        assert!(validate_template_variables("{{user_email}}").is_err());
        assert!(validate_template_variables("{{brand_name").is_err());
    }

    #[test]
    fn html_variables_are_escaped_before_rendering() {
        let template = default_email_template(EmailTemplateKind::VerifyEmail);
        let rendered = render_template(
            &template.html,
            &escape_html("<Acme & Co>"),
            &escape_html("https://example.test/?a=1&b=2"),
        );
        assert!(rendered.contains("&lt;Acme &amp; Co&gt;"));
        assert!(rendered.contains("a=1&amp;b=2"));
        assert!(!rendered.contains("<Acme"));
    }

    #[test]
    fn every_builtin_template_uses_brand_and_action_url() {
        for kind in [
            EmailTemplateKind::VerifyEmail,
            EmailTemplateKind::ResetPassword,
            EmailTemplateKind::ChangeEmail,
        ] {
            let template = default_email_template(kind);
            assert!(template.subject.contains("{{brand_name}}"));
            assert!(template.text.contains("{{brand_name}}"));
            assert!(template.text.contains("{{action_url}}"));
            assert!(template.html.contains("{{brand_name}}"));
            assert!(template.html.contains("{{action_url}}"));
            validate_template_variables(&template.subject).unwrap();
            validate_template_variables(&template.text).unwrap();
            validate_template_variables(&template.html).unwrap();
        }
    }

    #[tokio::test]
    async fn database_fallback_prefers_realm_locale_and_never_crosses_realms() {
        let (handle, pool, _sea) = herald_test_db::create_isolated_schema_database(2).await;
        let realm_a = "email-template-realm-a";
        let realm_b = "email-template-realm-b";
        for (id, name) in [(realm_a, "Realm A"), (realm_b, "Realm B")] {
            sqlx::query("INSERT INTO realm (id, name) VALUES ($1, $2)")
                .bind(id)
                .bind(name)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query(
            "INSERT INTO realm_config
             (realm_id, config_type, config_key, config_value, enabled)
             VALUES ($1, 'white_label', 'settings', $2, true)",
        )
        .bind(realm_a)
        .bind(r#"{"brandName":"Brand A"}"#)
        .execute(&pool)
        .await
        .unwrap();

        let content = |subject: &str| {
            serde_json::json!({
                "subject": subject,
                "text": "{{brand_name}} {{action_url}}",
                "html": "<p>{{brand_name}}</p><a href=\"{{action_url}}\">go</a>"
            })
            .to_string()
        };
        for (realm_id, template_type, subject) in [
            (None, "verify_email:zh-CN", "global localized"),
            (Some(realm_a), "verify_email", "realm default"),
            (Some(realm_a), "verify_email:zh-CN", "realm localized"),
        ] {
            sqlx::query(
                "INSERT INTO email_templates (realm_id, type, content) VALUES ($1, $2, $3)",
            )
            .bind(realm_id)
            .bind(template_type)
            .bind(content(subject))
            .execute(&pool)
            .await
            .unwrap();
        }

        let realm_message = EmailService::render_email(
            &pool,
            realm_a,
            EmailTemplateKind::VerifyEmail,
            "https://example.test/a?x=1&y=2",
            "zh-CN",
        )
        .await
        .unwrap();
        assert_eq!(realm_message.subject, "realm localized");
        assert!(realm_message.text.contains("Brand A"));
        assert!(realm_message.html.contains("x=1&amp;y=2"));

        let other_message = EmailService::render_email(
            &pool,
            realm_b,
            EmailTemplateKind::VerifyEmail,
            "https://example.test/b",
            "zh-CN",
        )
        .await
        .unwrap();
        assert_eq!(other_message.subject, "global localized");
        assert!(other_message.text.contains("Realm B"));
        assert!(!other_message.text.contains("Brand A"));

        handle.teardown().await;
    }
}
