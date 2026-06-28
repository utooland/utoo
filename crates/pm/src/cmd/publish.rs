use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use utoo_ruborist::manifest::PackageJson;
use utoo_ruborist::spec::Protocol;

use crate::helper::workspace::{init_project_root, update_cwd_to_project};
use crate::model::RunMode;
use crate::model::package::{PackageInfo, PublishMeta};
use crate::service::publish::{self as publish_service, PublishOptions};
use crate::service::workspace::{ResolvedWorkspaces, WorkspaceFilter, WorkspaceService};
use crate::util::user_config::{get_or_load_package_json, get_registry};

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
    filter: WorkspaceFilter,
) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let roots = resolve_publish_roots(&cwd, filter).await?;
    for root in roots {
        publish_one(&root, tag, mode, otp).await?;
    }
    Ok(())
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
    tag: Option<&str>,
    mode: RunMode,
    otp: Option<&str>,
) -> Result<()> {
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

    let tag = meta.resolve_tag(tag)?;
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
        mode,
        otp,
    })
    .await?;

    let mut stdout = io::stdout().lock();
    if mode == RunMode::DryRun {
        // Surface `workspace:`/`catalog:` specifiers rewritten to concrete
        // versions so the user can confirm resolution before publishing.
        print_resolved_protocol_deps(&mut stdout, &pkg, &result.pack.manifest)?;
        writeln!(
            stdout,
            "{}",
            format!(
                "(dry run) Would publish {}@{} to {} with tag '{}'",
                result.pack.name, result.pack.version, result.registry, result.tag
            )
            .yellow()
        )?;
    } else {
        writeln!(
            stdout,
            "{}",
            format!("+ {}@{}", result.pack.name, result.pack.version).green()
        )?;
    }

    Ok(())
}

/// Print dependencies whose `workspace:`/`catalog:` specifier was rewritten to a
/// concrete version in the packed manifest, as `name: <orig> -> <resolved>`.
///
/// Only the deps that actually used a protocol are shown; if none did (a
/// standalone package), nothing is printed. Entries are sorted for stable output.
fn print_resolved_protocol_deps(
    w: &mut impl Write,
    original: &PackageJson,
    resolved: &PackageJson,
) -> io::Result<()> {
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

    let mut rewritten: Vec<(&str, &str, &str, &str)> = Vec::new();
    for (label, orig, res) in maps {
        let (Some(orig), Some(res)) = (orig, res) else {
            continue;
        };
        for (name, spec) in orig {
            if Protocol::strip_prefix(spec).is_some()
                && let Some(resolved_spec) = res.get(name)
            {
                rewritten.push((label, name, spec, resolved_spec));
            }
        }
    }

    if rewritten.is_empty() {
        return Ok(());
    }

    rewritten.sort_unstable_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    writeln!(w, "{}", "Resolved workspace/catalog dependencies:".dimmed())?;
    for (label, name, orig_spec, resolved_spec) in rewritten {
        writeln!(
            w,
            "  {} {name}: {} {} {}",
            label.dimmed(),
            orig_spec,
            "->".dimmed(),
            resolved_spec.green()
        )?;
    }
    writeln!(w)?;
    Ok(())
}
