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
//!    - Refuse the browser flow for JSON or non-interactive invocations.
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

use crate::error::{CliError, ErrorKind};
use crate::model::RunMode;
use crate::model::package::LifecycleHook;
use crate::model::package::PackageInfo;
use crate::model::publish_payload::{PublishPayload, PublishPayloadInput};
use crate::service::auth;
use crate::service::oidc;
use crate::service::pm_pack;
use crate::service::provenance;
use crate::service::script::{ScriptOutput, ScriptService};
use crate::util::cli_enum::PublishAccess;
use crate::util::format_print::print_pack_details;
use crate::util::integrity::compute_shasum;

/// Options for publishing a package, resolved by the cmd layer.
pub struct PublishOptions<'a> {
    pub package_info: &'a PackageInfo,
    pub registry: &'a str,
    pub tag: &'a str,
    pub mode: RunMode,
    pub otp: Option<&'a str>,
    /// Registry visibility (`public`/`restricted`) for the published package.
    pub access: PublishAccess,
    /// Whether to generate and attach a signed provenance attestation.
    pub provenance: bool,
    pub script_output: ScriptOutput,
    pub web_auth: WebAuth,
}

/// Result returned to the cmd layer after a successful publish.
pub struct PublishResult {
    pub pack: pm_pack::PackResult,
    pub tag: String,
    pub registry: String,
}

pub enum PublishOutcome {
    Completed(PublishResult),
    Committed {
        result: PublishResult,
        lifecycle_error: anyhow::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebAuth {
    Allow,
    Deny,
}

pub async fn publish(opts: &PublishOptions<'_>) -> Result<PublishOutcome> {
    // Run prepublishOnly lifecycle script
    ScriptService::execute_script(
        opts.package_info,
        LifecycleHook::PrepublishOnly,
        opts.script_output,
        None,
    )
    .await?;

    // Always pack in memory — dry-run only skips the registry PUT.
    let pack_result = pm_pack::pack(&opts.package_info.path, opts.script_output).await?;

    let tarball_data = &pack_result.tarball_data;
    let shasum = compute_shasum(tarball_data);

    if opts.script_output != ScriptOutput::Machine {
        print_pack_details(&mut std::io::stdout().lock(), &pack_result, Some(&shasum))?;
    }

    if opts.mode == RunMode::DryRun {
        return Ok(PublishOutcome::Completed(PublishResult {
            pack: pack_result,
            tag: opts.tag.to_string(),
            registry: opts.registry.to_string(),
        }));
    }

    // Generate a signed provenance attestation when requested. Live publishes
    // only: dry-run returns above so it never writes to the public Rekor log.
    let provenance_bundle = if opts.provenance {
        Some(
            provenance::generate(&pack_result.name, &pack_result.version, tarball_data)
                .await
                .context("failed to generate provenance attestation")?,
        )
    } else {
        None
    };

    // Prefer OIDC trusted publishing when running in a supported CI (no
    // long-lived token needed); fall back to a configured token otherwise.
    let token = match oidc::try_mint_publish_token(opts.registry, &pack_result.name).await {
        Some(token) => token,
        None => auth::require_token(opts.registry).await?,
    };

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
        access: Some(opts.access.into()),
        provenance: provenance_bundle.as_ref(),
    });

    tracing::info!("Publishing to {} with tag {}", opts.registry, opts.tag);

    let escaped_name = auth::escaped_package_name(&pack_result.name);
    let url = format!("{}/{}", opts.registry.trim_end_matches('/'), escaped_name);

    let response =
        send_with_web_auth_retry(&url, &token, &payload, opts.otp, opts.web_auth).await?;

    let status = response.status().as_u16();
    if !matches!(status, 200 | 201) {
        let body = response.text().await.unwrap_or_default();
        return Err(
            publish_status_error(status, &body, &pack_result.name, &pack_result.version).into(),
        );
    }

    let result = PublishResult {
        pack: pack_result,
        tag: opts.tag.to_string(),
        registry: opts.registry.to_string(),
    };
    let lifecycle_result = async {
        ScriptService::execute_script(
            opts.package_info,
            LifecycleHook::Publish,
            opts.script_output,
            None,
        )
        .await?;
        ScriptService::execute_script(
            opts.package_info,
            LifecycleHook::Postpublish,
            opts.script_output,
            None,
        )
        .await
    }
    .await;

    match lifecycle_result {
        Ok(()) => Ok(PublishOutcome::Completed(result)),
        Err(lifecycle_error) => Ok(PublishOutcome::Committed {
            result,
            lifecycle_error,
        }),
    }
}

fn publish_status_error(status: u16, body: &str, name: &str, version: &str) -> CliError {
    let message = match status {
        401 => {
            format!("Authentication failed. Check your credentials or run `utoo login`.\n{body}")
        }
        403 => format!("Registry forbids publishing {name}@{version}.\n{body}"),
        409 => format!("{name}@{version} already exists. Use a different version."),
        _ => format!("Publish failed (HTTP {status}): {body}"),
    };
    CliError::new(ErrorKind::from_http_status(status), message)
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
    web_auth: WebAuth,
) -> Result<reqwest::Response> {
    let response = build_publish_request(url, token, payload, otp)?
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
        return Err(CliError::new(
            ErrorKind::Auth,
            format!("Authentication failed. Check your credentials or run `utoo login`.\n{body}"),
        )
        .into());
    };

    ensure_web_auth_allowed(web_auth)?;

    tracing::info!("Authenticate your account at:\n{auth_url}");
    if let Err(e) = open::that(auth_url) {
        tracing::warn!("Failed to open browser: {e}");
    }

    tracing::info!("Waiting for authentication...");
    let web_otp = auth::poll_done_url(done_url).await?;
    tracing::info!("Authentication successful, retrying publish...");

    build_publish_request(url, token, payload, Some(&web_otp))?
        .send()
        .await
        .context("Failed to send publish request (retry after web auth)")
}

fn ensure_web_auth_allowed(web_auth: WebAuth) -> Result<()> {
    if web_auth == WebAuth::Deny {
        return Err(CliError::new(
            ErrorKind::Auth,
            "interactive authentication is required to publish",
        )
        .with_suggestion("re-run in an interactive terminal or provide `--otp`")
        .into());
    }
    Ok(())
}

/// Build a PUT request to the registry, optionally including an OTP header.
fn build_publish_request(
    url: &str,
    token: &str,
    payload: &PublishPayload,
    otp: Option<&str>,
) -> Result<RequestBuilder> {
    let mut req = crate::util::http::client()?
        .put(url)
        .header("content-type", "application/json")
        .header("npm-auth-type", "web")
        .header("npm-command", "publish")
        .bearer_auth(token)
        .json(payload);
    if let Some(otp) = otp {
        req = req.header("npm-otp", otp);
    }
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::classify;

    #[test]
    fn web_auth_requires_an_interactive_human_invocation() {
        assert!(ensure_web_auth_allowed(WebAuth::Allow).is_ok());
        let error = ensure_web_auth_allowed(WebAuth::Deny).unwrap_err();
        assert_eq!(classify(&error), ErrorKind::Auth);
    }

    #[test]
    fn forbidden_publish_is_an_auth_error() {
        let error = anyhow::Error::from(publish_status_error(403, "forbidden", "fixture", "1.0.0"));
        assert_eq!(classify(&error), ErrorKind::Auth);
        assert_eq!(classify(&error).exit_code(), 3);
    }
}
