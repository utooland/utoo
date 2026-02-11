use anyhow::{Context, Result, bail};

use crate::util::config_file::Config;

fn registry_api(registry: &str, path: &str) -> String {
    format!("{}{}", registry.trim_end_matches('/'), path)
}

/// Resolve auth token from env vars or ~/.utoo/config.toml.
pub async fn resolve_token() -> Option<String> {
    for var in ["NPM_TOKEN", "NODE_AUTH_TOKEN"] {
        if let Ok(token) = std::env::var(var) {
            if !token.is_empty() {
                return Some(token);
            }
        }
    }

    let config = Config::load(true).await.ok()?;
    let token = config.get("auth-token").ok().flatten()?;
    if token.is_empty() { None } else { Some(token) }
}

/// Resolve token or bail with a login hint.
pub async fn require_token() -> Result<String> {
    resolve_token()
        .await
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run `utoo login` to authenticate."))
}

/// Login to the registry via web flow. Returns the token on success.
///
/// `on_login_url` is called with the URL the user must visit to authenticate.
pub async fn web_login(
    registry: &str,
    on_login_url: impl FnOnce(&str),
) -> Result<String> {
    let client = reqwest::Client::new();

    let response = client
        .post(registry_api(registry, "/-/v1/login"))
        .json(&serde_json::json!({}))
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

    let timeout = std::time::Duration::from_secs(300);
    let start = std::time::Instant::now();
    let mut interval = std::time::Duration::from_secs(5);

    loop {
        if start.elapsed() > timeout {
            bail!("Login timed out after 5 minutes");
        }

        tokio::time::sleep(interval).await;
        let poll_resp = client.get(done_url).send().await?;

        if let Some(retry_after) = poll_resp.headers().get("retry-after") {
            if let Ok(secs) = retry_after.to_str().unwrap_or("5").parse::<u64>() {
                interval = std::time::Duration::from_secs(secs);
            }
        }

        match poll_resp.status().as_u16() {
            200 => {
                let body: serde_json::Value = poll_resp.json().await?;
                let token = body["token"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing token in login response"))?;
                return Ok(token.to_string());
            }
            202 => continue,
            status => {
                let body = poll_resp.text().await.unwrap_or_default();
                bail!("Login polling failed (HTTP {}): {}", status, body);
            }
        }
    }
}

/// Query current identity from the registry.
pub async fn whoami(registry: &str, token: &str) -> Result<String> {
    let response = reqwest::Client::new()
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
    let response = reqwest::Client::new()
        .delete(registry_api(registry, &format!("/-/user/token/{}", token)))
        .bearer_auth(token)
        .send()
        .await
        .context("Failed to send logout request")?;

    let status = response.status();
    if !status.is_success() && status.as_u16() != 404 {
        let body = response.text().await.unwrap_or_default();
        tracing::warn!("Server-side token invalidation failed (HTTP {}): {}", status, body);
    }

    let mut config = Config::load(true).await?;
    config.delete("auth-token", true)?;
    Ok(())
}

/// Save token to ~/.utoo/config.toml.
pub async fn save_token(token: String) -> Result<()> {
    let mut config = Config::load(true).await?;
    config.set("auth-token", token, true)?;
    Ok(())
}
