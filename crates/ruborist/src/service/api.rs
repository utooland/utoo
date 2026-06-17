//! High-level API for dependency resolution.
//!
//! This module provides a simple, unified API for resolving dependencies
//! that works across different platforms (native CLI, WASM browser).
//!
//! # Example
//!
//! ```ignore
//! use utoo_ruborist::service::{build_deps, BuildDepsOptions};
//! use utoo_ruborist::progress::NoopReceiver;
//!
//! let output = build_deps(BuildDepsOptions::new(
//!     PathBuf::from("."),
//!     my_glob,
//!     NoopReceiver,
//! )).await?;
//!
//! // Serialize to JSON
//! let json = serde_json::to_string_pretty(&output.lock)?;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use super::cache::ProjectCacheData;
use super::fs::Glob;
use super::registry::UnifiedRegistry;
use crate::model::graph::DependencyGraph;
use crate::model::package_json::PackageJson;
use crate::model::package_lock::PackageLock;
use crate::resolver::builder::{
    BuildDepsConfig, DevDeps, EdgeContext, PeerDeps, add_edges_from, add_workspace_member,
    build_deps_with_config_output, resolve_workspace_member_edges,
};
use crate::resolver::runtime::install_runtime_from_map;
use crate::resolver::workspace::WorkspaceDiscovery;
use crate::spec::Catalogs;
use crate::traits::progress::EventReceiver;

/// Options for dependency resolution.
pub struct BuildDepsOptions<G, R> {
    /// Current working directory (contains package.json)
    pub cwd: PathBuf,
    /// Pre-built registry client. The host (pm) constructs this with the
    /// registry URL, manifest store, semver capability, and any private-registry
    /// auth token — keeping all registry configuration (and credential
    /// resolution) on the host side, out of ruborist.
    pub registry: UnifiedRegistry,
    /// Tarball cache directory passed through to non-registry resolvers
    /// (http/tarball, native-git). Unrelated to manifest disk cache.
    pub cache_dir: Option<PathBuf>,
    /// Project-level warm cache pre-loaded by the host. Seeds the demand
    /// resolver's manifest cache so a warm install skips re-fetching.
    pub project_cache: Option<ProjectCacheData>,
    /// Maximum concurrent network requests
    pub concurrency: usize,
    /// How to handle peer dependencies.
    pub peer_deps: PeerDeps,
    /// Glob implementation for workspace discovery
    pub glob: G,
    /// Progress event receiver
    pub receiver: R,
    /// Catalog definitions for the `catalog:` dependency protocol.
    /// Key `""` = default catalog, other keys = named catalogs.
    pub catalogs: Catalogs,
    /// An existing `package-lock.json` to reuse as the resolved-tree baseline.
    /// When present, the graph is seeded with this tree (see
    /// `model::lock_codec::lock_to_graph`) so the resolver only does work for the delta —
    /// added/changed/removed direct deps — and the prior tree's layout is
    /// preserved verbatim. `None` falls back to a full cold resolve. The
    /// lockfile supersedes `project_cache` on this path: it already pins every
    /// resolved manifest, so `project_cache` is ignored when this is `Some`.
    pub baseline: Option<PackageLock>,
}

impl<G, R> BuildDepsOptions<G, R> {
    /// Create options with default values.
    pub fn new(cwd: PathBuf, glob: G, receiver: R) -> Self
    where
        G: Default,
        R: Default,
    {
        Self {
            cwd,
            registry: UnifiedRegistry::builder()
                .registry("https://registry.npmmirror.com")
                .build(),
            cache_dir: None,
            project_cache: None,
            concurrency: 20,
            peer_deps: PeerDeps::Skip,
            glob,
            receiver,
            catalogs: HashMap::new(),
            baseline: None,
        }
    }
}

/// Output of [`build_deps`].
///
/// `project_cache` carries the manifests resolved during this run; the host
/// decides whether and where to persist it (typically to
/// `node_modules/.utoo-manifest.json`). It is empty when no resolutions
/// happened.
pub struct BuildDepsOutput {
    pub lock: PackageLock,
    pub project_cache: ProjectCacheData,
}

/// Build dependency tree from an **in-memory root manifest** `pkg` and return
/// [`BuildDepsOutput`]. `options.cwd` is the resolution root, already resolved by
/// the caller — a normal install reads it via [`read_root_manifest`]; a global
/// install (`utoo install -g`, `utoo x`) synthesizes a private root
/// `{ dependencies: { <tool>: <spec> } }` so the tool resolves as a *dependency*
/// (no `prepare`/`prepublish`, no `devDependencies`). It:
/// 1. Injects runtime dependencies (node-bin packages from engines.install-node)
/// 2. Builds the graph and adds root + workspace edges
/// 3. Resolves all dependencies using the registry
/// 4. Returns a [`BuildDepsOutput`] with the package lock and the new project cache
pub async fn build_deps<G, R>(
    options: BuildDepsOptions<G, R>,
    mut pkg: PackageJson,
) -> Result<BuildDepsOutput>
where
    G: Glob + Clone,
    R: EventReceiver,
{
    let BuildDepsOptions {
        cwd: root_path,
        registry,
        cache_dir,
        project_cache,
        concurrency,
        peer_deps,
        glob,
        receiver,
        catalogs,
        baseline,
    } = options;

    // The lockfile baseline pins every resolved manifest itself, so the
    // separate manifest project cache is redundant — and seeding both would
    // duplicate the warm state. Drop it on the reuse path.
    let project_cache = if baseline.is_some() {
        None
    } else {
        project_cache
    };

    // 1. Inject runtime dependencies (node-bin packages)
    if let Some(engines) = &pkg.engines {
        let runtime_deps = install_runtime_from_map(engines);
        if !runtime_deps.is_empty() {
            tracing::debug!("Injecting {} runtime dependencies", runtime_deps.len());
            for (name, version) in runtime_deps {
                pkg.optional_dependencies
                    .get_or_insert_with(HashMap::new)
                    .entry(name)
                    .or_insert(version);
            }
        }
    }

    // 2. Initialize dependency graph + root dependency edges (catalog: specs
    //    resolved at edge creation).
    let mut graph = DependencyGraph::from_package_json(root_path.clone(), pkg.clone());
    let root_index = graph.root_index;
    let edge_ctx = EdgeContext::new(peer_deps, DevDeps::Include).with_catalogs(&catalogs);
    add_edges_from(&mut graph, root_index, &pkg, &edge_ctx);

    // 3. Discover and add workspace packages. A synthetic global-install root has
    //    no `workspaces` field, so this returns immediately without touching disk.
    let discovery = WorkspaceDiscovery::new(glob);
    let workspaces = discovery.find_workspaces_from_pkg(&root_path, &pkg).await?;
    for workspace in workspaces {
        tracing::debug!(
            "Added workspace: {} at {:?}",
            workspace.name,
            workspace.path
        );
        add_workspace_member(
            &mut graph,
            root_index,
            &workspace.name,
            workspace.path,
            &workspace.package_json,
            &edge_ctx,
        );
    }
    // Settle all importer-declared workspace: edges now that every member is
    // attached — the BFS never sees a workspace: spec on the happy path.
    resolve_workspace_member_edges(&mut graph);

    // 3b. Seed the resolved tree from the existing lockfile (if any). Importer
    //     edges stay unresolved (live, from the current manifests) so the BFS
    //     resolves only the delta against the seeded nodes; everything pinned
    //     by the lock is reused with no network I/O.
    let reuse_baseline = baseline.is_some();
    if let Some(lock) = baseline {
        crate::model::lock_codec::lock_to_graph(&mut graph, &lock, &root_path);
    }

    // 4. The host supplies the stateless registry client (URL, store, semver,
    //    auth) pre-built. The warm `project_cache` seeds the demand resolver's
    //    manifest cache directly via `BuildDepsConfig::project_cache` below.
    tracing::debug!(
        "Using registry: {} (semver: {})",
        registry.registry_url(),
        registry.supports_semver(),
    );

    let mut config = BuildDepsConfig::default()
        .with_peer_deps(peer_deps)
        .with_concurrency(concurrency)
        .with_catalogs(catalogs)
        .with_project_cache(project_cache);
    if let Some(dir) = cache_dir {
        config = config.with_cache_dir(dir);
    }

    // Preserve the typed error via `Error::new` + `.context(...)` so CLI
    // renderers (e.g. pm's format_print) can downcast and pretty-print the
    // dependency chain carried by `ResolveError::WithChain`.
    let manifest_cache = build_deps_with_config_output(&mut graph, &registry, config, &receiver)
        .await
        .map_err(|e| anyhow::Error::new(e).context("Dependency resolution failed"))?;

    // On the reuse path, prune nodes the BFS left orphaned (removed deps,
    // versions shadowed by a re-resolved bump) before emitting the lock.
    let (packages, _total) = if reuse_baseline {
        graph.serialize_to_packages_pruned(&root_path)
    } else {
        graph.serialize_to_packages(&root_path)
    };

    // Export the manifests resolved this run for the host to persist.
    let project_cache = ProjectCacheData::from_resolved(manifest_cache.entries);

    Ok(BuildDepsOutput {
        lock: PackageLock::new(&pkg.name, &pkg.version, packages),
        project_cache,
    })
}

/// Resolve the workspace root for `cwd` and read its `package.json`. The
/// disk-side counterpart to [`build_deps`]: a normal install calls this to get
/// the `(root_path, manifest)` pair, then passes the manifest to `build_deps`.
/// (Global installs skip this and synthesize the manifest in memory.)
pub async fn read_root_manifest<G: Glob + Clone>(
    cwd: &Path,
    glob: G,
) -> Result<(PathBuf, PackageJson)> {
    let discovery = WorkspaceDiscovery::new(glob);
    let root_path = discovery.find_root_path(cwd).await?;
    let pkg_path = root_path.join("package.json");
    let pkg: PackageJson = super::fs::read_json(&pkg_path).await.with_context(|| {
        format!(
            "Failed to read/parse package.json at {}",
            pkg_path.display()
        )
    })?;
    Ok((root_path, pkg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::fs::NoopGlob;
    use crate::traits::progress::NoopReceiver;

    #[tokio::test]
    async fn test_build_deps_options_creation() {
        let options: BuildDepsOptions<NoopGlob, NoopReceiver> = BuildDepsOptions {
            cwd: PathBuf::from("."),
            registry: UnifiedRegistry::builder()
                .registry("https://registry.npmmirror.com")
                .build(),
            cache_dir: None,
            project_cache: None,
            concurrency: 20,
            peer_deps: PeerDeps::Skip,
            glob: NoopGlob,
            receiver: NoopReceiver,
            catalogs: HashMap::new(),
            baseline: None,
        };

        assert_eq!(options.concurrency, 20);
        assert_eq!(options.peer_deps, PeerDeps::Skip);
        assert_eq!(
            options.registry.registry_url(),
            "https://registry.npmmirror.com"
        );
    }

    /// `build_deps` resolves an **in-memory** synthetic root (no disk
    /// package.json, no workspace discovery) — the path global installs use. A
    /// root with no dependencies resolves offline to a lock with only the root
    /// entry.
    #[tokio::test]
    async fn test_build_deps_in_memory_root_no_deps() {
        let options: BuildDepsOptions<NoopGlob, NoopReceiver> = BuildDepsOptions {
            cwd: PathBuf::from("/synthetic-root"),
            registry: UnifiedRegistry::builder()
                .registry("https://registry.npmmirror.com")
                .build(),
            cache_dir: None,
            project_cache: None,
            concurrency: 20,
            peer_deps: PeerDeps::Skip,
            glob: NoopGlob,
            receiver: NoopReceiver,
            catalogs: HashMap::new(),
            baseline: None,
        };
        let mut pkg = PackageJson::new("utoo-global", "0.0.0");
        pkg.private = Some(true);

        let output = build_deps(options, pkg)
            .await
            .expect("in-memory synthetic root should resolve offline");
        assert!(output.lock.packages.contains_key(""), "root entry present");
        assert_eq!(output.lock.packages.len(), 1, "no deps → only the root");
    }

    #[test]
    fn test_project_cache_export_scoped_packages() {
        // Test that scoped packages are correctly parsed when exporting project cache
        // This ensures "@babel/core@^7.0.0" is parsed as ("@babel/core", "^7.0.0")
        // not ("", "babel/core@^7.0.0")
        let test_cases = [
            // (input, expected_name, expected_spec)
            ("@babel/core@^7.0.0", "@babel/core", "^7.0.0"),
            ("@types/node@^18.0.0", "@types/node", "^18.0.0"),
            ("@scope/pkg@1.0.0", "@scope/pkg", "1.0.0"),
            ("lodash@^4.17.0", "lodash", "^4.17.0"),
            ("express@4.18.0", "express", "4.18.0"),
        ];

        for (input, expected_name, expected_spec) in test_cases {
            let (name, spec) = crate::model::util::parse_package_spec(input);
            assert_eq!(
                name, expected_name,
                "Failed for input '{}': expected name '{}', got '{}'",
                input, expected_name, name
            );
            assert_eq!(
                spec, expected_spec,
                "Failed for input '{}': expected spec '{}', got '{}'",
                input, expected_spec, spec
            );
        }

        let buggy_result = "@babel/core@^7.0.0".split_once('@');
        assert_eq!(buggy_result, Some(("", "babel/core@^7.0.0")));
    }
}
