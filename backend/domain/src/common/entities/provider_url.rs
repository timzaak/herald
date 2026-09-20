// Client-side validation of realm-configured provider base URLs.
//
// The write-time rejection and the boot-time audit of provider `base_url`
// overrides are both gated on the production label; on any other label
// (demo/staging/a typo) a realm admin can point provider traffic at an
// arbitrary host, and the credential-transmitting clients would then send
// the stored raw API keys there. This validation is the client-side,
// label-independent backstop: a provider client never transmits credentials
// to a destination that is not an HTTPS endpoint (loopback HTTP is allowed
// for local test fixtures).

use crate::common::entities::app_errors::CoreError;

/// Validate a provider base URL before a client will transmit credentials to
/// it. Accepts `https://…` anywhere, and `http://…` only for loopback hosts
/// (`localhost`, `127.0.0.0/8`, `[::1]`) used by local test fixtures.
pub fn validate_provider_base_url(raw: &str) -> Result<(), CoreError> {
    let parsed = url::Url::parse(raw).map_err(|_| invalid(raw))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => {
            let host = parsed
                .host_str()
                .ok_or_else(|| invalid(raw))?
                .trim_start_matches('[')
                .trim_end_matches(']');
            // The loopback class is decided by parsing, not by a "127."
            // prefix: a DNS name like 127.0.0.1.attacker.example would
            // otherwise ride the prefix past this guard and receive the
            // transmitted credentials.
            let is_loopback = host == "localhost"
                || host
                    .parse::<std::net::Ipv4Addr>()
                    .is_ok_and(|ip| ip.is_loopback())
                || host
                    .parse::<std::net::Ipv6Addr>()
                    .is_ok_and(|ip| ip.is_loopback());
            if is_loopback {
                Ok(())
            } else {
                Err(invalid(raw))
            }
        }
        _ => Err(invalid(raw)),
    }
}

fn invalid(raw: &str) -> CoreError {
    CoreError::BadRequest(format!(
        "provider base_url must be an https endpoint (http only on loopback): {raw}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_is_accepted_anywhere() {
        assert!(validate_provider_base_url("https://api.creem.io").is_ok());
        assert!(validate_provider_base_url("https://internal.example:8443/base").is_ok());
    }

    #[test]
    fn plain_http_is_accepted_only_on_loopback() {
        assert!(validate_provider_base_url("http://127.0.0.1:8080").is_ok());
        assert!(validate_provider_base_url("http://localhost:8080").is_ok());
        assert!(validate_provider_base_url("http://[::1]:8080").is_ok());
        // 回归（审计 run-2：unvalidated-provider-base-url-credential-transmission）：
        // 任意明文主机拒绝 —— 客户端不得向其传输原始凭证。
        assert!(validate_provider_base_url("http://attacker.example").is_err());
        assert!(validate_provider_base_url("ftp://api.creem.io").is_err());
        assert!(validate_provider_base_url("not a url").is_err());
    }

    #[test]
    fn loopback_is_decided_by_parsing_not_by_a_127_prefix() {
        // 回归（review 20260920）："127." 前缀曾把任意以 127. 开头的 DNS 名
        // 当作 127.0.0.0/8 放行 —— loopback 语义必须由地址解析判定。
        assert!(validate_provider_base_url("http://127.0.0.1.attacker.example").is_err());
        assert!(validate_provider_base_url("http://127.foo.example").is_err());
        // 127.0.0.0/8 的每个真实地址仍然放行（本地测试夹具）。WHATWG URL
        // 解析会把 IPv4 简写归一化成完整地址，因此 127.1 ≡ 127.0.0.1 也放行。
        assert!(validate_provider_base_url("http://127.0.0.1").is_ok());
        assert!(validate_provider_base_url("http://127.255.255.254:9443").is_ok());
        assert!(validate_provider_base_url("http://127.1").is_ok());
    }
}
