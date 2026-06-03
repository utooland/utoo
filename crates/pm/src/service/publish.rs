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

use crate::model::RunMode;
use crate::model::package::LifecycleHook;
use crate::model::package::PackageInfo;
use crate::model::publish_payload::{PublishPayload, PublishPayloadInput};
use crate::service::auth;
use crate::service::pm_pack;
use crate::service::script::{ScriptOutput, ScriptService};
use crate::util::format_print::print_pack_details;
use crate::util::integrity::compute_shasum;

/// Options for publishing a package, resolved by the cmd layer.
pub struct PublishOptions<'a> {
    pub package_info: &'a PackageInfo,
    pub registry: &'a str,
    pub tag: &'a str,
    pub mode: RunMode,
    pub otp: Option<&'a str>,
}

/// Result returned to the cmd layer after a successful publish.
pub struct PublishResult {
    pub pack: pm_pack::PackResult,
    pub tag: String,
    pub registry: String,
}

pub async fn publish(opts: &PublishOptions<'_>) -> Result<PublishResult> {
    // Run prepublishOnly lifecycle script
    ScriptService::execute_script(
        opts.package_info,
        LifecycleHook::PrepublishOnly,
        ScriptOutput::Verbose,
    )
    .await?;

    // Always pack in memory — dry-run only skips the registry PUT.
    let pack_result = pm_pack::pack(&opts.package_info.path).await?;

    let tarball_data = &pack_result.tarball_data;
    let shasum = compute_shasum(tarball_data);

    print_pack_details(&mut std::io::stdout().lock(), &pack_result, Some(&shasum))?;

    if opts.mode == RunMode::DryRun {
        return Ok(PublishResult {
            pack: pack_result,
            tag: opts.tag.to_string(),
            registry: opts.registry.to_string(),
        });
    }

    let token = auth::require_token(opts.registry).await?;

    // Reuse the manifest packed into the tarball (with `workspace:`/`catalog:`
    // already rewritten) so the registry metadata matches the tarball contents.
    let payload = PublishPayload::new(&PublishPayloadInput {
        package_json: &pack_result.manifest,
        name: &pack_result.name,
        version: &pack_result.version,
        tag: opts.tag,
        shasum: &shasum,
        integrity: &pack_result.integrity,
        tarball_data,
        registry: opts.registry,
        access: Some("public"),
    });

    tracing::info!("Publishing to {} with tag {}", opts.registry, opts.tag);

    // Scoped packages: encode `/` as `%2f` so the registry sees a single path
    // segment (npm does the same via `npa.resolve().escapedName`).
    let escaped_name = pack_result.name.replace('/', "%2f");
    let url = format!("{}/{}", opts.registry.trim_end_matches('/'), escaped_name);

    let response = send_with_web_auth_retry(&url, &token, &payload, opts.otp).await?;

    match response.status().as_u16() {
        200 | 201 => {}
        401 => {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "Authentication failed. Check your credentials or run `utoo login`.\n{body}"
            );
        }
        403 => {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Forbidden: {}", body);
        }
        409 => {
            anyhow::bail!(
                "{}@{} already exists. Use a different version.",
                pack_result.name,
                pack_result.version,
            );
        }
        other => {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Publish failed (HTTP {}): {}", other, body);
        }
    }

    // Run publish lifecycle scripts
    ScriptService::execute_script(
        opts.package_info,
        LifecycleHook::Publish,
        ScriptOutput::Verbose,
    )
    .await?;
    ScriptService::execute_script(
        opts.package_info,
        LifecycleHook::Postpublish,
        ScriptOutput::Verbose,
    )
    .await?;

    Ok(PublishResult {
        pack: pack_result,
        tag: opts.tag.to_string(),
        registry: opts.registry.to_string(),
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
        anyhow::bail!("Authentication failed. Check your credentials or run `utoo login`.\n{body}");
    };

    tracing::info!("Authenticate your account at:\n{auth_url}");
    if let Err(e) = open::that(auth_url) {
        tracing::warn!("Failed to open browser: {e}");
    }

    tracing::info!("Waiting for authentication...");
    let web_otp = auth::poll_done_url(done_url).await?;
    tracing::info!("Authentication successful, retrying publish...");

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
