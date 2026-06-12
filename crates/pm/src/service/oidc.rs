//! OIDC trusted publishing.
//!
//! Mints a short-lived registry token from a CI OIDC id_token so `ut publish`
//! in CI needs no long-lived npm token. Compatible with npm's trusted-publishing
//! flow (npm 11.5.1+):
//!
//! 1. Obtain a CI OIDC id_token whose audience is `npm:<registry-host>`:
//!    - **GitHub Actions**: `GET $ACTIONS_ID_TOKEN_REQUEST_URL&audience=…` with
//!      `Authorization: Bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN` → `{ value }`.
//!    - **GitLab CI / CircleCI**: read the pre-supplied `$NPM_ID_TOKEN`.
//! 2. Exchange it at
//!    `POST {registry}/-/npm/v1/oidc/token/exchange/package/{escapedName}`
//!    (`Authorization: Bearer <id_token>`) → `{ token }`.
//!
//! Best-effort: returns `None` when not in an OIDC-capable CI or when the
//! exchange fails, so the caller falls back to a configured token.

use std::env;

use anyhow::{Context, Result};
use reqwest::Url;
use serde_json::Value;

use crate::service::auth;
use crate::util::http::client;

/// Try to mint a publish token via OIDC trusted publishing.
///
/// `None` means "not available / fell back" — the caller should resolve a
/// configured token instead. Never errors: a misconfigured or unsupported
/// registry simply yields `None`.
pub async fn try_mint_publish_token(registry: &str, package_name: &str) -> Option<String> {
    // Normalize a bare-host registry to a scheme'd URL once, so audience
    // derivation and the exchange endpoint both parse.
    let registry = auth::parse_registry(registry)?;
    let registry = registry.as_str();
    let id_token = ci_id_token(registry).await?;
    match exchange(registry, package_name, &id_token).await {
        Ok(token) => {
            tracing::info!(
                "OIDC trusted publishing: minted a short-lived token for {package_name}"
            );
            Some(token)
        }
        Err(e) => {
            tracing::warn!("OIDC token exchange failed, falling back to configured token: {e:#}");
            None
        }
    }
}

/// The OIDC audience for `registry`, e.g. `npm:registry.npmjs.org`. Matches
/// npm's `npm:${new URL(registry).hostname}` (host only, no port or path).
fn audience(registry: &str) -> Option<String> {
    let host = auth::parse_registry(registry)?.host_str()?.to_owned();
    Some(format!("npm:{host}"))
}

/// Obtain a CI OIDC id_token for the registry audience, or `None` when not in a
/// supported CI with OIDC enabled.
async fn ci_id_token(registry: &str) -> Option<String> {
    let audience = audience(registry)?;

    // GitHub Actions: request an id_token from the runner's token service.
    // Both vars are present only when the workflow grants `id-token: write`.
    if let (Ok(req_url), Ok(req_token)) = (
        env::var("ACTIONS_ID_TOKEN_REQUEST_URL"),
        env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
    ) && !req_url.is_empty()
        && !req_token.is_empty()
    {
        return github_actions_id_token(&req_url, &req_token, &audience).await;
    }

    // GitLab CI / CircleCI supply the id_token directly via `id_tokens`.
    match env::var("NPM_ID_TOKEN") {
        Ok(token) if !token.is_empty() => Some(token),
        _ => None,
    }
}

/// Fetch an id_token from the GitHub Actions runner token service.
async fn github_actions_id_token(req_url: &str, req_token: &str, audience: &str) -> Option<String> {
    let mut url = Url::parse(req_url).ok()?;
    url.query_pairs_mut().append_pair("audience", audience);

    let resp = client()
        .ok()?
        .get(url)
        .header("Accept", "application/json")
        .bearer_auth(req_token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        tracing::warn!(
            "GitHub Actions OIDC token request failed: HTTP {}",
            resp.status()
        );
        return None;
    }
    let body: Value = resp.json().await.ok()?;
    body["value"].as_str().map(str::to_owned)
}

/// Exchange a CI OIDC id_token for a short-lived registry publish token.
async fn exchange(registry: &str, package_name: &str, id_token: &str) -> Result<String> {
    let url = format!(
        "{}/-/npm/v1/oidc/token/exchange/package/{}",
        registry.trim_end_matches('/'),
        escaped_name(package_name),
    );

    let resp = client()?
        .post(&url)
        .bearer_auth(id_token)
        .send()
        .await
        .context("OIDC token exchange request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OIDC token exchange returned HTTP {status}: {body}");
    }

    let body: Value = resp
        .json()
        .await
        .context("Invalid OIDC token exchange response")?;
    body["token"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("OIDC token exchange response missing `token`"))
}

/// npm `escapedName`: only the `/` in a scoped name is percent-encoded, so
/// `@scope/name` → `@scope%2fname` and `pkg` stays `pkg`.
fn escaped_name(package_name: &str) -> String {
    package_name.replace('/', "%2f")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audience() {
        assert_eq!(
            audience("https://registry.npmjs.org").as_deref(),
            Some("npm:registry.npmjs.org")
        );
        // Bare host (no scheme) is normalized before parsing.
        assert_eq!(
            audience("registry.npmjs.org").as_deref(),
            Some("npm:registry.npmjs.org")
        );
        assert_eq!(audience("localhost:4873").as_deref(), Some("npm:localhost"));
        // Path and port are dropped — host only.
        assert_eq!(
            audience("https://packages.example.com:8443/org/npm-registry/").as_deref(),
            Some("npm:packages.example.com")
        );
        assert_eq!(audience("not a url"), None);
    }

    #[test]
    fn test_escaped_name() {
        assert_eq!(escaped_name("lodash"), "lodash");
        assert_eq!(escaped_name("@scope/pkg"), "@scope%2fpkg");
    }

    #[tokio::test]
    async fn test_github_actions_id_token() {
        use mockito::Matcher;
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/token")
            .match_query(Matcher::UrlEncoded(
                "audience".into(),
                "npm:registry.npmjs.org".into(),
            ))
            .match_header("authorization", "Bearer req-token")
            .match_header("accept", "application/json")
            .with_status(200)
            .with_body(r#"{"value":"the.id.token"}"#)
            .create_async()
            .await;

        let req_url = format!("{}/token", server.url());
        let token = github_actions_id_token(&req_url, "req-token", "npm:registry.npmjs.org").await;

        mock.assert_async().await;
        assert_eq!(token.as_deref(), Some("the.id.token"));
    }

    #[tokio::test]
    async fn test_exchange_mints_token() {
        let mut server = mockito::Server::new_async().await;
        // Scoped name escaping: `@scope/pkg` → `@scope%2fpkg` in the path.
        let mock = server
            .mock("POST", "/-/npm/v1/oidc/token/exchange/package/@scope%2fpkg")
            .match_header("authorization", "Bearer the.id.token")
            .with_status(200)
            .with_body(r#"{"token":"npm_minted_publish_token"}"#)
            .create_async()
            .await;

        let token = exchange(&server.url(), "@scope/pkg", "the.id.token")
            .await
            .unwrap();

        mock.assert_async().await;
        assert_eq!(token, "npm_minted_publish_token");
    }

    #[tokio::test]
    async fn test_exchange_non_success_errors() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/-/npm/v1/oidc/token/exchange/package/pkg")
            .with_status(404)
            .with_body("no trusted publisher configured")
            .create_async()
            .await;

        let err = exchange(&server.url(), "pkg", "the.id.token")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("404"));
    }
}
