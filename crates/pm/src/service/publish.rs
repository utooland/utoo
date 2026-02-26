//! Publish a package tarball to an npm-compatible registry.
//!
//! ## Authentication flow
//!
//! 1. Resolve a bearer token from environment (`NPM_TOKEN` / `NODE_AUTH_TOKEN`)
//!    or local config (`~/.utoo/config.toml`). If none is found, bail with a
//!    login hint.
//!
//! 2. Send the PUT request with headers `npm-auth-type: web` and
//!    `npm-command: publish`. These tell the registry the client supports
//!    web-based OTP approval.
//!
//! 3. If the registry responds **401** and the body contains `authUrl` +
//!    `doneUrl` (indicating 2FA / OTP is required):
//!    - Print the `authUrl` and open it in the default browser.
//!    - Poll `doneUrl` every few seconds (respecting `retry-after`) until the
//!      user approves in the browser. The endpoint returns **202** while
//!      pending, **200** with a `token` field on success.
//!    - The returned token is an **OTP value**, not a replacement bearer token.
//!      Retry the PUT with the original bearer token and the OTP in the
//!      `npm-otp` header.
//!
//! 4. If the 401 body does *not* contain web-auth URLs, report a generic
//!    authentication error.

use anyhow::{Context, Result};
use reqwest::RequestBuilder;

use crate::model::package::PackageInfo;
use crate::model::publish_payload::{PublishPayload, PublishPayloadInput};
use crate::service::auth;
use crate::service::pm_pack;
use crate::service::script::ScriptService;
use crate::util::format_print::print_pack_details;
use crate::util::integrity::compute_shasum;
use crate::util::json::load_package_json_from_path;

/// Result returned to the cmd layer after a successful publish.
pub struct PublishResult {
    pub pack: pm_pack::PackResult,
    pub tag: String,
    pub registry: String,
}

pub async fn publish(
    package_info: &PackageInfo,
    registry: &str,
    tag: &str,
    dry_run: bool,
    otp: Option<&str>,
) -> Result<PublishResult> {
    // Run prepublishOnly lifecycle script
    ScriptService::execute_script(package_info, "prepublishOnly", true).await?;

    // Always pack in memory — dry-run only skips the registry PUT.
    let pack_result = pm_pack::pack(&package_info.path).await?;

    let tarball_data = &pack_result.tarball_data;
    let shasum = compute_shasum(tarball_data);

    print_pack_details(&pack_result, Some(&shasum));

    if dry_run {
        return Ok(PublishResult {
            pack: pack_result,
            tag: tag.to_string(),
            registry: registry.to_string(),
        });
    }

    let token = auth::require_token(registry).await?;

    // Load package.json for version metadata in the publish payload
    let package_json = load_package_json_from_path(&package_info.path).await?;
    let payload = PublishPayload::new(&PublishPayloadInput {
        package_json: &package_json,
        name: &pack_result.name,
        version: &pack_result.version,
        tag,
        shasum: &shasum,
        integrity: &pack_result.integrity,
        tarball_data,
        registry,
        access: Some("public"),
    });

    println!("Publishing to {registry} with tag {tag}");

    // Scoped packages: encode `/` as `%2f` so the registry sees a single path
    // segment (npm does the same via `npa.resolve().escapedName`).
    let escaped_name = pack_result.name.replace('/', "%2f");
    let url = format!("{}/{}", registry.trim_end_matches('/'), escaped_name);

    let response = send_with_web_auth_retry(&url, &token, &payload, otp).await?;

    match response.status().as_u16() {
        200 | 201 => {}
        401 => {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Authentication failed. Check your credentials or run `utoo login`.\n{body}"
            ));
        }
        403 => {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Forbidden: {}", body));
        }
        409 => {
            return Err(anyhow::anyhow!(
                "{}@{} already exists. Use a different version.",
                pack_result.name,
                pack_result.version,
            ));
        }
        other => {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Publish failed (HTTP {}): {}", other, body));
        }
    }

    // Run publish lifecycle scripts
    ScriptService::execute_script(package_info, "publish", true).await?;
    ScriptService::execute_script(package_info, "postpublish", true).await?;

    Ok(PublishResult {
        pack: pack_result,
        tag: tag.to_string(),
        registry: registry.to_string(),
    })
}

/// Send a publish PUT request, handling web-based OTP approval if needed.
///
/// If the first request returns 401 with `authUrl` + `doneUrl` in the body
/// (indicating web-based 2FA), opens the browser, polls for approval, and
/// retries with the OTP. Otherwise returns the initial response as-is.
async fn send_with_web_auth_retry(
    url: &str,
    token: &str,
    payload: &PublishPayload,
    otp: Option<&str>,
) -> Result<reqwest::Response> {
    let response = build_publish_request(url, token, payload, otp)
        .send()
        .await
        .context("Failed to send publish request")?;

    if response.status().as_u16() != 401 || otp.is_some() {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
    let body_json: serde_json::Value =
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);

    let (Some(auth_url), Some(done_url)) =
        (body_json["authUrl"].as_str(), body_json["doneUrl"].as_str())
    else {
        return Err(anyhow::anyhow!(
            "Authentication failed. Check your credentials or run `utoo login`.\n{body}"
        ));
    };

    println!("Authenticate your account at:\n{auth_url}");
    if let Err(e) = open::that(auth_url) {
        tracing::warn!("Failed to open browser: {e}");
    }

    println!("Waiting for authentication...");
    let web_otp = auth::poll_done_url(done_url).await?;
    println!("Authentication successful, retrying publish...");

    build_publish_request(url, token, payload, Some(&web_otp))
        .send()
        .await
        .context("Failed to send publish request (retry after web auth)")
}

/// Build a PUT request to the registry, optionally including an OTP header.
fn build_publish_request(
    url: &str,
    token: &str,
    payload: &PublishPayload,
    otp: Option<&str>,
) -> RequestBuilder {
    let mut req = crate::util::http::client()
        .put(url)
        .header("content-type", "application/json")
        .header("npm-auth-type", "web")
        .header("npm-command", "publish")
        .bearer_auth(token)
        .json(payload);
    if let Some(otp) = otp {
        req = req.header("npm-otp", otp);
    }
    req
}
