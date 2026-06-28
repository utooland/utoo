//! Build provenance generation for `ut publish --provenance`.
//!
//! Produces a Sigstore-signed SLSA provenance attestation for a package
//! tarball, following npm's provenance flow so the resulting bundle verifies on
//! attestation-aware registries (npmjs.org, GitHub Packages):
//!
//! 1. [`slsa`] detects the CI provider (GitHub Actions / GitLab CI), builds the
//!    SLSA v1 predicate, and fetches a `sigstore`-audience OIDC id_token.
//! 2. The predicate + subject (tarball purl & SHA-512) form an in-toto Statement.
//! 3. [`sigstore`] signs the Statement keylessly (ephemeral key → Fulcio cert →
//!    Rekor tlog) and assembles a Sigstore bundle v0.3.
//!
//! The bundle is attached to the publish request as a second `_attachments`
//! entry named `<name>-<version>.sigstore` (see [`crate::model::publish_payload`]).
//!
//! Provenance only succeeds in a supported CI; outside one, [`generate`] bails
//! with guidance rather than producing an unsigned/invalid attestation.

mod sigstore;
mod slsa;

use anyhow::{Context, Result};
use sha2::{Digest, Sha512};

use slsa::Subject;

/// A signed provenance attestation ready to attach to a publish request.
pub struct ProvenanceBundle {
    /// Bundle media type, e.g. `application/vnd.dev.sigstore.bundle.v0.3+json`.
    pub media_type: String,
    /// Serialized bundle JSON.
    pub data: String,
}

/// Generate a signed provenance bundle for the packed tarball.
///
/// `name`/`version` identify the package; `tarball` is the exact gzip bytes
/// being published (its SHA-512 becomes the attestation subject digest).
pub async fn generate(name: &str, version: &str, tarball: &[u8]) -> Result<ProvenanceBundle> {
    let sha512_hex = hex::encode(Sha512::digest(tarball));
    let subject = Subject::new(name, version, sha512_hex);

    let ctx = slsa::resolve_build_context()
        .await
        .context("failed to resolve CI build context for provenance")?;

    let statement = slsa::build_statement(&subject, &ctx.predicate);

    let (data, media_type) = sigstore::attest(&statement, &ctx.id_token)
        .await
        .context("failed to sign provenance attestation")?;

    Ok(ProvenanceBundle { media_type, data })
}
