use anyhow::{Context, Result, bail};
use reqwest::Url;
use tokio::sync::OnceCell;

use crate::util::cli_enum::ConfigScope;
use crate::util::config_file::Config;
use crate::util::http::client;

fn registry_api(registry: &str, path: &str) -> String {
    format!("{}{}", registry.trim_end_matches('/'), path)
}

/// Extract host from registry URL for use as config key suffix.
///
/// "https://registry.npmjs.org/" -> "registry.npmjs.org"
fn registry_host(registry: &str) -> &str {
    let s = registry
        .strip_prefix("https://")
        .or_else(|| registry.strip_prefix("http://"))
        .unwrap_or(registry);
    s.trim_end_matches('/')
}

/// Config key for storing auth token per registry.
fn token_key(registry: &str) -> String {
    format!("auth-token:{}", registry_host(registry))
}

/// Resolve auth token for a specific registry.
pub async fn resolve_token(registry: &str) -> Option<String> {
    for var in ["NPM_TOKEN", "NODE_AUTH_TOKEN"] {
        if let Ok(token) = std::env::var(var)
            && !token.is_empty()
        {
            return Some(token);
        }
    }

    let config = Config::load(ConfigScope::Global).await.ok()?;
    let token = config.get(&token_key(registry)).ok().flatten()?;
    if token.is_empty() { None } else { Some(token) }
}

/// Token for the configured registry, resolved once and cached for the process.
///
/// Install decides per request whether to attach auth; resolving env/config on
/// every fetch would be wasteful, so the lookup runs at most once (npm/bun both
/// load credentials once at startup, then do in-memory lookups).
pub async fn cached_token() -> Option<String> {
    static CACHED_TOKEN: OnceCell<Option<String>> = OnceCell::const_new();
    CACHED_TOKEN
        .get_or_init(|| async { resolve_token(&crate::util::user_config::get_registry()).await })
        .await
        .clone()
}

/// Whether `url` targets the same host as `registry`. The leak guard for
/// attaching a token to a tarball download: credentials go to the registry
/// host only, never to a third-party CDN a manifest's `dist.tarball` points at.
fn same_host(url: &str, registry: &str) -> bool {
    fn host(s: &str) -> Option<String> {
        Url::parse(s).ok()?.host_str().map(str::to_ascii_lowercase)
    }
    matches!((host(url), host(registry)), (Some(a), Some(b)) if a == b)
}

/// The Bearer token to attach when fetching `url`, or `None` when there is no
/// configured token or `url` does not target the registry host (leak guard).
/// Use for downloads whose host is not guaranteed to be the registry (e.g. a
/// tarball URL that a manifest may point at a CDN).
pub async fn token_for_url(url: &str) -> Option<String> {
    if same_host(url, &crate::util::user_config::get_registry()) {
        cached_token().await
    } else {
        None
    }
}

/// Resolve token or bail with a login hint.
pub async fn require_token(registry: &str) -> Result<String> {
    resolve_token(registry).await.ok_or_else(|| {
        anyhow::anyhow!(
            "Not logged in to {}. Run `utoo login` to authenticate.",
            registry
        )
    })
}

/// Login to the registry via web flow. Returns the token on success.
///
/// `on_login_url` is called with the URL the user must visit to authenticate.
pub async fn web_login(registry: &str, on_login_url: impl FnOnce(&str)) -> Result<String> {
    let client = client();

    let response = client
        .post(registry_api(registry, "/-/v1/login"))
        .header("npm-auth-type", "web")
        .json(&serde_json::json!({ "hostname": "utoo" }))
        .send()
        .await
        .context("Failed to initiate web login")?;

    let status = response.status();
    if matches!(status.as_u16(), 404 | 501) {
        bail!("Registry does not support web login (HTTP {})", status);
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("Web login initiation failed (HTTP {}): {}", status, body);
    }

    let body: serde_json::Value = response.json().await?;
    let login_url = body["loginUrl"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing loginUrl in response"))?;
    let done_url = body["doneUrl"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing doneUrl in response"))?;

    on_login_url(login_url);

    poll_done_url(done_url).await
}

/// Poll a registry `doneUrl` until it returns a token.
///
/// Used by both the login flow and the publish OTP web-auth flow.
/// Returns **202** while pending, **200** with a `token` field on success.
/// Respects the `retry-after` header; times out after 5 minutes.
pub async fn poll_done_url(done_url: &str) -> Result<String> {
    let client = client();
    let timeout = std::time::Duration::from_secs(300);
    let start = std::time::Instant::now();
    let mut interval = std::time::Duration::from_secs(5);

    loop {
        if start.elapsed() > timeout {
            bail!("Authentication timed out after 5 minutes");
        }

        tokio::time::sleep(interval).await;
        let resp = client.get(done_url).send().await?;

        if let Some(retry_after) = resp.headers().get("retry-after")
            && let Ok(secs) = retry_after.to_str().unwrap_or("5").parse::<u64>()
        {
            interval = std::time::Duration::from_secs(secs);
        }

        match resp.status().as_u16() {
            200 => {
                let body: serde_json::Value = resp.json().await?;
                let token = body["token"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing token in response"))?;
                return Ok(token.to_string());
            }
            202 => continue,
            status => {
                let body = resp.text().await.unwrap_or_default();
                bail!("Authentication polling failed (HTTP {status}): {body}");
            }
        }
    }
}

/// Query current identity from the registry.
pub async fn whoami(registry: &str, token: &str) -> Result<String> {
    let response = client()
        .get(registry_api(registry, "/-/whoami"))
        .bearer_auth(token)
        .send()
        .await
        .context("Failed to send whoami request")?;

    let status = response.status();
    if status.as_u16() == 401 {
        bail!("Authentication failed. Your token may be invalid or expired.");
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("Whoami failed (HTTP {}): {}", status, body);
    }

    let body: serde_json::Value = response.json().await?;
    body["username"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("Unexpected response: missing username"))
}

/// Logout: invalidate the token on the server and remove from local config.
pub async fn logout(registry: &str, token: &str) -> Result<()> {
    let response = client()
        .delete(registry_api(registry, &format!("/-/user/token/{}", token)))
        .bearer_auth(token)
        .send()
        .await
        .context("Failed to send logout request")?;

    let status = response.status();
    if !status.is_success() && status.as_u16() != 404 {
        let body = response.text().await.unwrap_or_default();
        tracing::warn!(
            "Server-side token invalidation failed (HTTP {}): {}",
            status,
            body
        );
    }

    let mut config = Config::load(ConfigScope::Global).await?;
    config.delete(&token_key(registry), ConfigScope::Global)?;
    Ok(())
}

/// Save token to ~/.utoo/config.toml, keyed by registry.
pub async fn save_token(registry: &str, token: String) -> Result<()> {
    let mut config = Config::load(ConfigScope::Global).await?;
    config.set(&token_key(registry), token, ConfigScope::Global)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_api_trailing_slash() {
        assert_eq!(
            registry_api("https://registry.npmjs.org/", "/-/whoami"),
            "https://registry.npmjs.org/-/whoami"
        );
    }

    #[test]
    fn test_registry_api_no_trailing_slash() {
        assert_eq!(
            registry_api("https://registry.npmjs.org", "/-/v1/login"),
            "https://registry.npmjs.org/-/v1/login"
        );
    }

    #[test]
    fn test_registry_host_https() {
        assert_eq!(
            registry_host("https://registry.npmjs.org/"),
            "registry.npmjs.org"
        );
    }

    #[test]
    fn test_same_host_leak_guard() {
        let registry = "https://registry.npmjs.org";
        // A tarball served by the registry itself → attach token.
        assert!(same_host(
            "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz",
            registry
        ));
        // Scheme differs but host matches → still the registry.
        assert!(same_host(
            "http://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz",
            registry
        ));
        // Host case is normalized.
        assert!(same_host("https://REGISTRY.npmjs.org/pkg.tgz", registry));
        // A third-party CDN → never leak the token.
        assert!(!same_host(
            "https://cdn.jsdelivr.net/npm/pkg/-/pkg-1.0.0.tgz",
            registry
        ));
        // A sibling subdomain is still a different host.
        assert!(!same_host("https://cdn.npmjs.org/pkg.tgz", registry));
        // Unparsable URLs are treated as non-matching (fail closed).
        assert!(!same_host("not a url", registry));
    }

    #[test]
    fn test_registry_host_http() {
        assert_eq!(registry_host("http://localhost:4873/"), "localhost:4873");
    }

    #[test]
    fn test_registry_host_bare() {
        assert_eq!(registry_host("registry.npmjs.org"), "registry.npmjs.org");
    }

    #[test]
    fn test_token_key_default_registry() {
        assert_eq!(
            token_key("https://registry.npmjs.org/"),
            "auth-token:registry.npmjs.org"
        );
    }

    #[test]
    fn test_token_key_custom_registry() {
        assert_eq!(
            token_key("http://localhost:4873"),
            "auth-token:localhost:4873"
        );
    }
}
