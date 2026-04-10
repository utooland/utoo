use anyhow::{Context, Result, bail};

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
