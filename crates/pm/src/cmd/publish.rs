use anyhow::{Context, Result};
use colored::Colorize;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use utoo_ruborist::manifest::PackageJson;
use utoo_ruborist::spec::Protocol;

use crate::helper::workspace::{init_project_root, update_cwd_to_project};
use crate::model::RunMode;
use crate::model::package::{PackageInfo, PublishMeta};
use crate::service::publish::{self as publish_service, PublishOptions, PublishOutcome, WebAuth};
use crate::service::script::ScriptOutput;
use crate::service::workspace::{ResolvedWorkspaces, WorkspaceFilter, WorkspaceService};
use crate::util::cli_enum::{ProvenancePolicy, PublishAccess};
use crate::util::invocation;
use crate::util::presenter::emit;
use crate::util::user_config::{get_or_load_package_json, get_registry};
use crate::{error::CliError, error::classify};

/// Publish one or more packages.
///
/// Without `--filter` the current package (closest `package.json` to the cwd)
/// is published. With `--filter` the matching workspace member(s) are resolved
/// from the workspace root and published in topological order, so a member can
/// be published from the repo root without `cd`-ing into its directory.
pub async fn publish(
    tag: Option<&str>,
    mode: RunMode,
    otp: Option<&str>,
    access: Option<PublishAccess>,
    filter: WorkspaceFilter,
    provenance: ProvenancePolicy,
) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let roots = resolve_publish_roots(&cwd, filter).await?;
    let mut packages = Vec::with_capacity(roots.len());
    let script_output = if invocation::json() {
        ScriptOutput::Machine
    } else {
        ScriptOutput::Verbose
    };
    let web_auth = if !invocation::json() && invocation::interactive() {
        WebAuth::Allow
    } else {
        WebAuth::Deny
    };
    let options = PublishCommandOptions {
        tag,
        mode,
        otp,
        access,
        provenance,
        script_output,
        web_auth,
    };
    for root in roots {
        match publish_one(&root, &options).await {
            Ok(outcome) => {
                if !invocation::json() {
                    print_publish_result(&outcome.package, mode)?;
                }
                packages.push(outcome.package);
                if let Some(error) = outcome.lifecycle_error {
                    return Err(partial_publish_error(error, mode, packages));
                }
            }
            Err(error) if !packages.is_empty() => {
                return Err(partial_publish_error(error, mode, packages));
            }
            Err(error) => return Err(error),
        }
    }
    let output = PublishOutput {
        dry_run: mode == RunMode::DryRun,
        packages,
    };
    emit("publish", &output, || Ok(()))
}

/// Resolve the set of package directories to publish, preserving workspace
/// topological order so dependencies publish before dependents.
async fn resolve_publish_roots(cwd: &Path, filter: WorkspaceFilter) -> Result<Vec<PathBuf>> {
    match filter {
        WorkspaceFilter::Current => Ok(vec![update_cwd_to_project(cwd).await?]),
        WorkspaceFilter::Selected(_) | WorkspaceFilter::All => {
            // `workspace:`/`catalog:` rewriting resolves against the workspace
            // root, so anchor resolution there regardless of the cwd.
            let root = init_project_root(cwd).await?;
            match WorkspaceService::resolve_layers(&root, filter).await? {
                // No workspaces (standalone project): publish the root package.
                ResolvedWorkspaces::Current => Ok(vec![root]),
                ResolvedWorkspaces::Layers { layers, paths } => {
                    let roots: Vec<PathBuf> = layers
                        .into_iter()
                        .flatten()
                        .filter_map(|name| paths.get(&name).cloned())
                        .collect();
                    if roots.is_empty() {
                        anyhow::bail!("No workspace packages matched the given --filter");
                    }
                    Ok(roots)
                }
            }
        }
    }
}

/// Validate, pack, and publish a single package located at `package_root`.
async fn publish_one(
    package_root: &Path,
    options: &PublishCommandOptions<'_>,
) -> Result<PublishOneOutcome> {
    // Run each publish from inside its own package directory, matching the
    // single-package flow (`update_cwd_to_project`). The filtered path anchors
    // resolution at the workspace root, so without this every member would
    // otherwise publish with the cwd left at the root — breaking cwd-relative
    // config and `INIT_CWD` for lifecycle scripts.
    std::env::set_current_dir(package_root)
        .with_context(|| format!("Failed to change directory to {}", package_root.display()))?;
    let pkg = get_or_load_package_json(package_root).await?;

    let meta = PublishMeta::from_package_json(&pkg);
    meta.validate()?;

    let tag = meta.resolve_tag(options.tag)?;
    let access = meta.resolve_access(options.access)?;
    let access_name: &'static str = access.into();
    // CLI `--provenance` OR `publishConfig.provenance` OR `NPM_CONFIG_PROVENANCE`.
    let provenance = meta.resolve_provenance(options.provenance);
    let registry = meta
        .publish_config
        .registry
        .as_deref()
        .map(String::from)
        .unwrap_or_else(get_registry);
    let package_info = PackageInfo::from_package_json(package_root, &pkg)?;
    let result = publish_service::publish(&PublishOptions {
        package_info: &package_info,
        registry: &registry,
        tag: &tag,
        mode: options.mode,
        otp: options.otp,
        access,
        provenance,
        script_output: options.script_output,
        web_auth: options.web_auth,
    })
    .await?;
    let (result, lifecycle_error) = match result {
        PublishOutcome::Completed(result) => (result, None),
        PublishOutcome::Committed {
            result,
            lifecycle_error,
        } => (result, Some(lifecycle_error)),
    };

    Ok(PublishOneOutcome {
        package: PublishedPackage {
            name: result.pack.name,
            version: result.pack.version,
            registry: result.registry,
            tag: result.tag,
            access: access_name,
            provenance: provenance.is_enabled(),
            files: result.pack.files.len(),
            packed_size: result.pack.packed_size,
            integrity: result.pack.integrity,
            resolved_dependencies: resolved_protocol_deps(&pkg, &result.pack.manifest),
        },
        lifecycle_error,
    })
}

fn partial_publish_error(
    error: anyhow::Error,
    mode: RunMode,
    completed_packages: Vec<PublishedPackage>,
) -> anyhow::Error {
    let category = classify(&error);
    CliError::new(category, format!("{error:#}"))
        .with_details(serde_json::json!({
            "dryRun": mode == RunMode::DryRun,
            "completedPackages": completed_packages,
        }))
        .into()
}

fn print_publish_result(package: &PublishedPackage, mode: RunMode) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    match mode {
        RunMode::DryRun => {
            print_resolved_protocol_deps(&mut stdout, &package.resolved_dependencies)?;
            writeln!(
                stdout,
                "{}",
                format!(
                    "(dry run) Would publish {}@{} to {} with tag '{}'",
                    package.name, package.version, package.registry, package.tag
                )
                .yellow()
            )
        }
        RunMode::Live => writeln!(
            stdout,
            "{}",
            format!("+ {}@{}", package.name, package.version).green()
        ),
    }?;
    stdout.flush()
}

/// Print dependencies whose `workspace:`/`catalog:` specifier was rewritten to a
/// concrete version in the packed manifest, as `name: <orig> -> <resolved>`.
///
/// Only the deps that actually used a protocol are shown; if none did (a
/// standalone package), nothing is printed. Entries are sorted for stable output.
fn print_resolved_protocol_deps(
    w: &mut impl Write,
    rewritten: &[ResolvedDependency],
) -> io::Result<()> {
    if rewritten.is_empty() {
        return Ok(());
    }

    writeln!(w, "{}", "Resolved workspace/catalog dependencies:".dimmed())?;
    for dep in rewritten {
        writeln!(
            w,
            "  {} {}: {} {} {}",
            dep.dependency_type.dimmed(),
            dep.name,
            dep.from,
            "->".dimmed(),
            dep.to.green()
        )?;
    }
    writeln!(w)?;
    Ok(())
}

fn resolved_protocol_deps(
    original: &PackageJson,
    resolved: &PackageJson,
) -> Vec<ResolvedDependency> {
    type DepMap = Option<HashMap<String, String>>;
    let maps: [(&str, &DepMap, &DepMap); 4] = [
        (
            "dependencies",
            &original.dependencies,
            &resolved.dependencies,
        ),
        (
            "devDependencies",
            &original.dev_dependencies,
            &resolved.dev_dependencies,
        ),
        (
            "peerDependencies",
            &original.peer_dependencies,
            &resolved.peer_dependencies,
        ),
        (
            "optionalDependencies",
            &original.optional_dependencies,
            &resolved.optional_dependencies,
        ),
    ];

    let mut rewritten = Vec::new();
    for (label, orig, res) in maps {
        let (Some(orig), Some(res)) = (orig, res) else {
            continue;
        };
        for (name, spec) in orig {
            if Protocol::strip_prefix(spec).is_some()
                && let Some(resolved_spec) = res.get(name)
            {
                rewritten.push(ResolvedDependency {
                    dependency_type: label.to_string(),
                    name: name.clone(),
                    from: spec.clone(),
                    to: resolved_spec.clone(),
                });
            }
        }
    }

    rewritten
        .sort_unstable_by(|a, b| (&a.dependency_type, &a.name).cmp(&(&b.dependency_type, &b.name)));
    rewritten
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishOutput {
    dry_run: bool,
    packages: Vec<PublishedPackage>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishedPackage {
    name: String,
    version: String,
    registry: String,
    tag: String,
    access: &'static str,
    provenance: bool,
    files: usize,
    packed_size: u64,
    integrity: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resolved_dependencies: Vec<ResolvedDependency>,
}

struct PublishOneOutcome {
    package: PublishedPackage,
    lifecycle_error: Option<anyhow::Error>,
}

struct PublishCommandOptions<'a> {
    tag: Option<&'a str>,
    mode: RunMode,
    otp: Option<&'a str>,
    access: Option<PublishAccess>,
    provenance: ProvenancePolicy,
    script_output: ScriptOutput,
    web_auth: WebAuth,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedDependency {
    dependency_type: String,
    name: String,
    from: String,
    to: String,
}
