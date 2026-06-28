//! SLSA build provenance: detect the CI provider, assemble the SLSA v1
//! provenance predicate, and obtain a Sigstore-audience OIDC id_token used to
//! prove the build identity to Fulcio.
//!
//! Mirrors the predicate npm's CLI generates (`buildType`
//! `https://github.com/npm/cli/gha/v2` for GitHub Actions) so the resulting
//! attestation verifies on registries that consume npm provenance.

use std::env;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::service::oidc;

/// OIDC audience required by Fulcio for keyless signing.
pub const SIGSTORE_AUDIENCE: &str = "sigstore";

const SLSA_PREDICATE_TYPE: &str = "https://slsa.dev/provenance/v1";
const IN_TOTO_STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";
const GHA_BUILD_TYPE: &str = "https://github.com/npm/cli/gha/v2";
const GITLAB_BUILD_TYPE: &str = "https://github.com/npm/cli/gitlab/v0";

/// The in-toto subject a provenance statement attests to: the published
/// tarball, identified by its package purl and SHA-512 digest.
pub struct Subject {
    /// Package URL, e.g. `pkg:npm/%40scope/pkg@1.0.0`.
    pub name: String,
    /// Lowercase hex SHA-512 of the tarball bytes.
    pub sha512_hex: String,
}

impl Subject {
    /// Build a subject from the package coordinates and tarball digest.
    ///
    /// The purl encodes the name per the package-url spec so scoped names
    /// (`@scope/pkg`) round-trip through registry verification.
    pub fn new(name: &str, version: &str, sha512_hex: String) -> Self {
        Self {
            name: format!("pkg:npm/{}@{version}", purl_encode_name(name)),
            sha512_hex,
        }
    }
}

/// Build context resolved from CI env vars: the SLSA predicate plus the OIDC
/// id_token (audience `sigstore`) identifying the workflow.
pub struct BuildContext {
    pub predicate: Value,
    pub id_token: String,
}

/// Assemble the in-toto Statement wrapping `subject` and `predicate`.
pub fn build_statement(subject: &Subject, predicate: &Value) -> Value {
    json!({
        "_type": IN_TOTO_STATEMENT_TYPE,
        "subject": [{
            "name": subject.name,
            "digest": { "sha512": subject.sha512_hex },
        }],
        "predicateType": SLSA_PREDICATE_TYPE,
        "predicate": predicate,
    })
}

/// Detect the CI provider, build its SLSA predicate, and fetch the signing
/// OIDC token. Bails with actionable guidance when no supported, OIDC-capable
/// CI is detected.
pub async fn resolve_build_context() -> Result<BuildContext> {
    if env::var_os("GITHUB_ACTIONS").is_some_and(|v| v == "true") {
        let id_token = oidc::github_oidc_token(SIGSTORE_AUDIENCE).await.context(
            "GitHub Actions OIDC token unavailable: grant `permissions: id-token: write` \
             to the job",
        )?;
        Ok(BuildContext {
            predicate: github_actions_predicate(),
            id_token,
        })
    } else if env::var_os("GITLAB_CI").is_some_and(|v| v == "true") {
        // GitLab provides the token directly via an `id_tokens` job entry; npm
        // reads `SIGSTORE_ID_TOKEN`.
        let id_token = env::var("SIGSTORE_ID_TOKEN")
            .ok()
            .filter(|t| !t.is_empty())
            .context(
                "GitLab CI provenance requires a `SIGSTORE_ID_TOKEN` id_token with `aud: sigstore`",
            )?;
        Ok(BuildContext {
            predicate: gitlab_ci_predicate(),
            id_token,
        })
    } else {
        bail!(
            "--provenance requires a supported CI with OIDC. Detected neither \
             GitHub Actions (set `permissions: id-token: write`) nor GitLab CI \
             (configure a `SIGSTORE_ID_TOKEN` with `aud: sigstore`)."
        )
    }
}

/// SLSA v1 predicate populated from GitHub Actions `GITHUB_*` env vars.
fn github_actions_predicate() -> Value {
    let var = |k: &str| env::var(k).unwrap_or_default();
    let server_url = var("GITHUB_SERVER_URL");
    let repository = var("GITHUB_REPOSITORY");
    let git_ref = var("GITHUB_REF");
    let sha = var("GITHUB_SHA");
    let workflow_ref = var("GITHUB_WORKFLOW_REF");
    let run_id = var("GITHUB_RUN_ID");
    let run_attempt = var("GITHUB_RUN_ATTEMPT");

    // `GITHUB_WORKFLOW_REF` is `owner/repo/path/to/workflow.yml@ref`; the
    // workflow path is what remains after stripping the `owner/repo/` prefix
    // and the `@ref` suffix.
    let workflow_path = workflow_ref
        .strip_prefix(&format!("{repository}/"))
        .and_then(|rest| rest.split('@').next())
        .unwrap_or_default()
        .to_string();

    json!({
        "buildDefinition": {
            "buildType": GHA_BUILD_TYPE,
            "externalParameters": {
                "workflow": {
                    "ref": git_ref,
                    "repository": format!("{server_url}/{repository}"),
                    "path": workflow_path,
                },
            },
            "internalParameters": {
                "github": {
                    "event_name": var("GITHUB_EVENT_NAME"),
                    "repository_id": var("GITHUB_REPOSITORY_ID"),
                    "repository_owner_id": var("GITHUB_REPOSITORY_OWNER_ID"),
                },
            },
            "resolvedDependencies": [{
                "uri": format!("git+{server_url}/{repository}@{git_ref}"),
                "digest": { "gitCommit": sha },
            }],
        },
        "runDetails": {
            "builder": { "id": format!("{server_url}/{workflow_ref}") },
            "metadata": {
                "invocationId": format!(
                    "{server_url}/{repository}/actions/runs/{run_id}/attempts/{run_attempt}"
                ),
            },
        },
    })
}

/// SLSA v1 predicate populated from GitLab CI `CI_*` env vars.
fn gitlab_ci_predicate() -> Value {
    let var = |k: &str| env::var(k).unwrap_or_default();
    let server_url = var("CI_SERVER_URL");
    let project_path = var("CI_PROJECT_PATH");
    // `CI_COMMIT_REF_NAME` is the bare branch/tag name; resolve the fully
    // qualified ref (`refs/tags/…` on a tag pipeline, `refs/heads/…` otherwise)
    // so the provenance ref/URI match what verifiers expect.
    let git_ref = gitlab_full_ref(&var("CI_COMMIT_TAG"), &var("CI_COMMIT_REF_NAME"));
    let sha = var("CI_COMMIT_SHA");
    let pipeline_id = var("CI_PIPELINE_ID");
    let job_url = var("CI_JOB_URL");

    json!({
        "buildDefinition": {
            "buildType": GITLAB_BUILD_TYPE,
            "externalParameters": {
                "workflow": {
                    "ref": git_ref,
                    "repository": format!("{server_url}/{project_path}"),
                    "path": var("CI_CONFIG_PATH"),
                },
            },
            "internalParameters": {
                "gitlab": {
                    "pipeline_id": pipeline_id,
                    "project_id": var("CI_PROJECT_ID"),
                },
            },
            "resolvedDependencies": [{
                "uri": format!("git+{server_url}/{project_path}@{git_ref}"),
                "digest": { "gitCommit": sha },
            }],
        },
        "runDetails": {
            "builder": { "id": format!("{server_url}/{project_path}/-/runners") },
            "metadata": { "invocationId": job_url },
        },
    })
}

/// Resolve a GitLab pipeline's fully qualified git ref. `CI_COMMIT_TAG` is set
/// only on tag pipelines (→ `refs/tags/<tag>`); otherwise the ref is the branch
/// `CI_COMMIT_REF_NAME` (→ `refs/heads/<branch>`). An empty ref name yields an
/// empty string rather than a dangling `refs/heads/`.
fn gitlab_full_ref(commit_tag: &str, ref_name: &str) -> String {
    if !commit_tag.is_empty() {
        format!("refs/tags/{commit_tag}")
    } else if !ref_name.is_empty() {
        format!("refs/heads/{ref_name}")
    } else {
        String::new()
    }
}

/// Percent-encode a package name for a purl. Per the package-url spec (and
/// npm-package-arg's `toPurl`), the leading `@` of a scoped name becomes `%40`
/// while the `/` scope separator stays literal, so `@scope/pkg` becomes
/// `%40scope/pkg` — the canonical form (`pkg:npm/%40scope/pkg@…`) registries
/// verify against.
fn purl_encode_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purl_encodes_scoped_name() {
        assert_eq!(purl_encode_name("lodash"), "lodash");
        assert_eq!(purl_encode_name("@scope/pkg"), "%40scope/pkg");
        assert_eq!(purl_encode_name("@a-b/c.d_e"), "%40a-b/c.d_e");
    }

    #[test]
    fn subject_builds_purl() {
        let s = Subject::new("@scope/pkg", "1.2.3", "abc".into());
        assert_eq!(s.name, "pkg:npm/%40scope/pkg@1.2.3");
        assert_eq!(s.sha512_hex, "abc");
    }

    #[test]
    fn gitlab_full_ref_resolves_tag_and_branch() {
        // Tag pipeline: CI_COMMIT_TAG takes precedence.
        assert_eq!(gitlab_full_ref("v1.0.0", "v1.0.0"), "refs/tags/v1.0.0");
        // Branch pipeline: no tag, fall back to the branch ref name.
        assert_eq!(gitlab_full_ref("", "main"), "refs/heads/main");
        // Nothing set: empty rather than a dangling `refs/heads/`.
        assert_eq!(gitlab_full_ref("", ""), "");
    }

    #[test]
    fn statement_has_required_shape() {
        let subject = Subject::new("pkg", "1.0.0", "deadbeef".into());
        let stmt = build_statement(&subject, &json!({ "buildDefinition": {} }));
        assert_eq!(stmt["_type"], IN_TOTO_STATEMENT_TYPE);
        assert_eq!(stmt["predicateType"], SLSA_PREDICATE_TYPE);
        assert_eq!(stmt["subject"][0]["name"], "pkg:npm/pkg@1.0.0");
        assert_eq!(stmt["subject"][0]["digest"]["sha512"], "deadbeef");
    }

    #[test]
    fn github_predicate_parses_workflow_path() {
        // Exercised indirectly: ensure prefix/suffix stripping is correct.
        let repository = "owner/repo";
        let workflow_ref = "owner/repo/.github/workflows/publish.yml@refs/heads/main";
        let path = workflow_ref
            .strip_prefix(&format!("{repository}/"))
            .and_then(|rest| rest.split('@').next())
            .unwrap_or_default();
        assert_eq!(path, ".github/workflows/publish.yml");
    }
}
