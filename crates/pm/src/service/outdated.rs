use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use futures::stream::{self, StreamExt};
use utoo_ruborist::graph::{DependencyGraph, EdgeType};
use utoo_ruborist::lock::{PackageLock, resolve_lock_dependency};
use utoo_ruborist::manifest::{PackageJson, VersionsRef};
use utoo_ruborist::registry::resolve_target_version;
use utoo_ruborist::service::{ManifestFullData, ManifestJob, ManifestJobDone, ManifestProvider};
use utoo_ruborist::spec::{PackageSpec, Protocol, resolve_catalog_spec, resolve_workspace_spec};

use crate::helper::ruborist_context::Context as FsContext;
use crate::service::workspace::WorkspaceFilter;
use crate::util::cache::matches_pattern;
use crate::util::config_file::Config;
use crate::util::json::{load_package_lock_json_from_path, read_json_file};
use crate::util::user_config::get_manifests_concurrency_limit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutdatedInfo {
    pub package: String,
    pub registry_package: String,
    pub protocol: Option<Protocol>,
    pub dependency_type: EdgeType,
    pub dependent: String,
    pub declared: String,
    pub resolved_spec: String,
    pub current: Option<String>,
    pub wanted: String,
    pub latest: String,
    pub location: Option<String>,
}

#[derive(Debug)]
struct Importer {
    name: String,
    lock_path: String,
    package_json: PackageJson,
}

#[derive(Debug, Clone)]
struct Dependency {
    package: String,
    registry_package: String,
    protocol: Option<Protocol>,
    dependency_type: EdgeType,
    dependent: String,
    declared: String,
    resolved_spec: String,
    current: Option<String>,
    location: Option<String>,
}

#[derive(Debug)]
struct PackageVersions {
    versions: Vec<String>,
    dist_tags: HashMap<String, String>,
}

pub async fn find_outdated(
    root_path: &Path,
    current_project: &Path,
    workspace_filter: WorkspaceFilter,
    patterns: &[String],
) -> Result<Vec<OutdatedInfo>> {
    let registry = FsContext::registry().await;
    find_outdated_with_registry(
        root_path,
        current_project,
        workspace_filter,
        patterns,
        registry,
    )
    .await
}

async fn find_outdated_with_registry(
    root_path: &Path,
    current_project: &Path,
    workspace_filter: WorkspaceFilter,
    patterns: &[String],
    registry: crate::helper::ruborist_context::Registry,
) -> Result<Vec<OutdatedInfo>> {
    let lock = load_package_lock_json_from_path(root_path)
        .await
        .with_context(|| {
            format!(
                "package-lock.json is required; run `ut install` in {} first",
                root_path.display()
            )
        })?;
    let root_package: PackageJson = read_json_file(&root_path.join("package.json")).await?;
    let override_graph = DependencyGraph::from_package_json(root_path.to_path_buf(), root_package);
    let importers = select_importers(root_path, current_project, workspace_filter).await?;
    let catalogs = Config::load_from_path(&root_path.join(".utoo.toml"))
        .await
        .unwrap_or_else(|error| {
            tracing::warn!("failed to load catalog config: {error}");
            Config::default()
        })
        .catalogs();
    let workspace_versions: HashMap<String, String> = FsContext::discovery()
        .find_workspaces(root_path)
        .await?
        .into_iter()
        .map(|workspace| (workspace.name, workspace.package_json.version))
        .collect();

    let mut dependencies = Vec::new();
    for importer in importers {
        collect_dependencies(
            &importer,
            patterns,
            &catalogs,
            &workspace_versions,
            &lock,
            &override_graph,
            &mut dependencies,
        )?;
    }

    let package_names: BTreeSet<String> = dependencies
        .iter()
        .map(|dependency| dependency.registry_package.clone())
        .collect();
    let manifests = fetch_package_versions(package_names, registry).await?;

    let mut result = Vec::new();
    for dependency in dependencies {
        let versions = manifests
            .get(&dependency.registry_package)
            .with_context(|| {
                format!(
                    "manifest for {} was not fetched",
                    dependency.registry_package
                )
            })?;
        let wanted = resolve_target_version(
            VersionsRef {
                versions: &versions.versions,
                dist_tags: &versions.dist_tags,
            },
            &dependency.resolved_spec,
        )
        .with_context(|| {
            format!(
                "no wanted version for {}@{}",
                dependency.registry_package, dependency.resolved_spec
            )
        })?;
        let Some(latest) = versions.dist_tags.get("latest").cloned() else {
            tracing::warn!("{} has no latest dist-tag", dependency.registry_package);
            continue;
        };
        if dependency.current.is_none() && dependency.dependency_type != EdgeType::Prod {
            continue;
        }

        if dependency.current.as_deref() == Some(wanted.as_str()) && wanted == latest {
            continue;
        }

        result.push(OutdatedInfo {
            package: dependency.package,
            registry_package: dependency.registry_package,
            protocol: dependency.protocol,
            dependency_type: dependency.dependency_type,
            dependent: dependency.dependent,
            declared: dependency.declared,
            resolved_spec: dependency.resolved_spec,
            current: dependency.current,
            wanted,
            latest,
            location: dependency.location,
        });
    }

    result.sort_by(|a, b| {
        a.package
            .cmp(&b.package)
            .then_with(|| a.dependent.cmp(&b.dependent))
    });
    Ok(result)
}

async fn select_importers(
    root_path: &Path,
    current_project: &Path,
    filter: WorkspaceFilter,
) -> Result<Vec<Importer>> {
    let root_package: PackageJson = read_json_file(&root_path.join("package.json")).await?;
    let workspaces = FsContext::discovery().find_workspaces(root_path).await?;

    let root_importer = || Importer {
        name: root_package.name.clone(),
        lock_path: String::new(),
        package_json: root_package.clone(),
    };
    let to_importer =
        |path: PathBuf, name: String, package_json: PackageJson| -> Result<Importer> {
            let lock_path = path
                .strip_prefix(root_path)
                .with_context(|| format!("workspace {} is outside project root", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            Ok(Importer {
                name,
                lock_path,
                package_json,
            })
        };

    match filter {
        WorkspaceFilter::Current => {
            if current_project == root_path {
                return Ok(vec![root_importer()]);
            }
            let workspace = workspaces
                .into_iter()
                .find(|workspace| workspace.path == current_project)
                .with_context(|| {
                    format!(
                        "current project {} is not a workspace of {}",
                        current_project.display(),
                        root_path.display()
                    )
                })?;
            Ok(vec![to_importer(
                workspace.path,
                workspace.name,
                workspace.package_json,
            )?])
        }
        WorkspaceFilter::All => {
            let mut importers = vec![root_importer()];
            for workspace in workspaces {
                importers.push(to_importer(
                    workspace.path,
                    workspace.name,
                    workspace.package_json,
                )?);
            }
            Ok(importers)
        }
        WorkspaceFilter::Selected(filters) => {
            let mut importers = Vec::new();
            for workspace in workspaces {
                let relative = workspace
                    .path
                    .strip_prefix(root_path)
                    .unwrap_or(&workspace.path)
                    .to_string_lossy()
                    .replace('\\', "/");
                if filters.iter().any(|filter| {
                    matches_pattern(&workspace.name, filter) || matches_pattern(&relative, filter)
                }) {
                    importers.push(to_importer(
                        workspace.path,
                        workspace.name,
                        workspace.package_json,
                    )?);
                }
            }
            if importers.is_empty() {
                bail!("no workspace matched: {}", filters.join(", "));
            }
            Ok(importers)
        }
    }
}

fn collect_dependencies(
    importer: &Importer,
    patterns: &[String],
    catalogs: &utoo_ruborist::spec::Catalogs,
    workspace_versions: &HashMap<String, String>,
    lock: &PackageLock,
    override_graph: &DependencyGraph,
    output: &mut Vec<Dependency>,
) -> Result<()> {
    let mut dependencies: HashMap<String, (String, EdgeType)> = HashMap::new();
    for (map, edge_type) in [
        (&importer.package_json.dependencies, EdgeType::Prod),
        (&importer.package_json.dev_dependencies, EdgeType::Dev),
        (&importer.package_json.peer_dependencies, EdgeType::Peer),
        (
            &importer.package_json.optional_dependencies,
            EdgeType::Optional,
        ),
    ] {
        for (name, spec) in map.iter().flatten() {
            dependencies.insert(name.clone(), (spec.clone(), edge_type));
        }
    }

    for (name, (declared, dependency_type)) in dependencies {
        if !patterns.is_empty()
            && !patterns
                .iter()
                .any(|pattern| matches_pattern(&name, pattern))
        {
            continue;
        }

        let declared_protocol = Protocol::strip_prefix(&declared).map(|(protocol, _)| protocol);
        if declared_protocol == Some(Protocol::Workspace) {
            validate_workspace_dependency(&name, &declared, workspace_versions)?;
            continue;
        }

        let resolved = resolve_lock_dependency(lock, &importer.lock_path, &name);
        let (location, current) = resolved.map_or((None, None), |(path, package)| {
            (Some(path.to_string()), package.version.clone())
        });
        let override_spec =
            override_graph.check_override(override_graph.root_index, &name, current.as_deref());
        let effective_spec = override_spec.as_deref().unwrap_or(&declared);
        let protocol = Protocol::strip_prefix(effective_spec).map(|(protocol, _)| protocol);

        let resolved_spec = if override_spec.is_none() && protocol == Some(Protocol::Catalog) {
            resolve_catalog_spec(&name, &declared, catalogs)
                .with_context(|| format!("cannot resolve {name} from {declared}"))?
                .to_string()
        } else {
            effective_spec.to_string()
        };

        let (registry_package, version_spec) = match PackageSpec::from(resolved_spec.as_str()) {
            PackageSpec::Registry {
                name: alias_name,
                version_spec,
            } if protocol == Some(Protocol::NpmAlias) => (alias_name, version_spec),
            PackageSpec::Registry { .. }
                if protocol.is_none() || protocol == Some(Protocol::Catalog) =>
            {
                (name.clone(), resolved_spec)
            }
            _ => continue,
        };

        output.push(Dependency {
            package: name,
            registry_package,
            protocol,
            dependency_type,
            dependent: importer.name.clone(),
            declared,
            resolved_spec: version_spec,
            current,
            location,
        });
    }
    Ok(())
}

fn validate_workspace_dependency(
    name: &str,
    declared: &str,
    workspace_versions: &HashMap<String, String>,
) -> Result<()> {
    let version = workspace_versions
        .get(name)
        .with_context(|| format!("workspace dependency {name} does not match a project member"))?;
    let published_spec = resolve_workspace_spec(declared, version)
        .with_context(|| format!("invalid workspace dependency {name}@{declared}"))?;
    if !utoo_ruborist::semver::matches(&published_spec, version) {
        bail!("workspace dependency {name}@{declared} does not accept local version {version}");
    }
    Ok(())
}

async fn fetch_package_versions(
    package_names: BTreeSet<String>,
    registry: crate::helper::ruborist_context::Registry,
) -> Result<HashMap<String, PackageVersions>> {
    let concurrency = get_manifests_concurrency_limit().await.max(1);
    let results = stream::iter(package_names)
        .map(|name| {
            let registry = registry.clone();
            async move {
                let result = registry
                    .execute_manifest_job(ManifestJob::Full {
                        name: name.clone(),
                        spec: None,
                    })
                    .await
                    .map_err(anyhow::Error::new)?;
                let versions = match result {
                    ManifestJobDone::Full { data, .. } => match data {
                        ManifestFullData::Full { manifest, .. } => PackageVersions {
                            versions: manifest.versions.clone(),
                            dist_tags: manifest.dist_tags.clone(),
                        },
                        ManifestFullData::Versions(info) => PackageVersions {
                            versions: info.versions.version_list.clone(),
                            dist_tags: info.versions.dist_tags.clone(),
                        },
                    },
                    ManifestJobDone::Version { .. } => {
                        unreachable!("full manifest request returned a version response")
                    }
                };
                Ok::<_, anyhow::Error>((name, versions))
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;

    results.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use utoo_ruborist::service::{NoopStore, UnifiedRegistry};

    #[test]
    fn package_patterns_use_or_semantics() {
        let patterns = ["eslint*".to_string(), "@types/*".to_string()];
        assert!(patterns.iter().any(|p| matches_pattern("eslint", p)));
        assert!(patterns.iter().any(|p| matches_pattern("@types/node", p)));
        assert!(!patterns.iter().any(|p| matches_pattern("react", p)));
    }

    #[test]
    fn validates_workspace_ranges() {
        let versions = HashMap::from([("local".to_string(), "1.2.3".to_string())]);
        assert!(validate_workspace_dependency("local", "workspace:^", &versions).is_ok());
        assert!(validate_workspace_dependency("local", "workspace:^2.0.0", &versions).is_err());
    }

    #[test]
    fn expands_catalog_and_alias_dependencies() {
        let importer = Importer {
            name: "app".to_string(),
            lock_path: String::new(),
            package_json: PackageJson {
                dependencies: Some(HashMap::from([
                    ("react".to_string(), "catalog:".to_string()),
                    ("legacy-react".to_string(), "npm:react@^17.0.0".to_string()),
                ])),
                ..PackageJson::default()
            },
        };
        let catalogs = HashMap::from([(
            String::new(),
            HashMap::from([("react".to_string(), "^18.0.0".to_string())]),
        )]);
        let mut dependencies = Vec::new();
        let lock = PackageLock::new("app", "1.0.0", HashMap::new());
        let override_graph =
            DependencyGraph::from_package_json(PathBuf::new(), importer.package_json.clone());

        collect_dependencies(
            &importer,
            &[],
            &catalogs,
            &HashMap::new(),
            &lock,
            &override_graph,
            &mut dependencies,
        )
        .unwrap();
        dependencies.sort_by(|a, b| a.package.cmp(&b.package));

        assert_eq!(dependencies[0].package, "legacy-react");
        assert_eq!(dependencies[0].registry_package, "react");
        assert_eq!(dependencies[0].resolved_spec, "^17.0.0");
        assert_eq!(dependencies[1].package, "react");
        assert_eq!(dependencies[1].registry_package, "react");
        assert_eq!(dependencies[1].resolved_spec, "^18.0.0");
    }

    #[test]
    fn applies_root_override_to_wanted_spec() {
        let package_json: PackageJson = serde_json::from_str(
            r#"{
              "name":"app",
              "dependencies":{"foo":"^1.0.0"},
              "overrides":{"foo":"1.0.0"}
            }"#,
        )
        .unwrap();
        let importer = Importer {
            name: "app".to_string(),
            lock_path: String::new(),
            package_json: package_json.clone(),
        };
        let lock: PackageLock = serde_json::from_str(
            r#"{
              "name":"app",
              "version":"1.0.0",
              "lockfileVersion":3,
              "packages":{"node_modules/foo":{"name":"foo","version":"1.0.0"}}
            }"#,
        )
        .unwrap();
        let override_graph = DependencyGraph::from_package_json(PathBuf::new(), package_json);
        let mut dependencies = Vec::new();

        collect_dependencies(
            &importer,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &lock,
            &override_graph,
            &mut dependencies,
        )
        .unwrap();

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].declared, "^1.0.0");
        assert_eq!(dependencies[0].resolved_spec, "1.0.0");
        assert_eq!(dependencies[0].current.as_deref(), Some("1.0.0"));
    }

    #[tokio::test]
    async fn finds_outdated_direct_dependency_from_lock_and_registry() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"name":"app","version":"1.0.0","dependencies":{"foo":"^1.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            temp.path().join("package-lock.json"),
            r#"{
              "name":"app",
              "version":"1.0.0",
              "lockfileVersion":3,
              "requires":true,
              "packages":{
                "":{"name":"app","version":"1.0.0","dependencies":{"foo":"^1.0.0"}},
                "node_modules/foo":{"name":"foo","version":"1.0.0"}
              }
            }"#,
        )
        .unwrap();

        let mut server = mockito::Server::new_async().await;
        let manifest = server
            .mock("GET", "/foo")
            .match_header("accept", "application/vnd.npm.install-v1+json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                  "name":"foo",
                  "dist-tags":{"latest":"2.0.0"},
                  "versions":{
                    "1.0.0":{"name":"foo","version":"1.0.0"},
                    "1.1.0":{"name":"foo","version":"1.1.0"},
                    "2.0.0":{"name":"foo","version":"2.0.0"}
                  }
                }"#,
            )
            .expect(1)
            .create_async()
            .await;
        let registry = UnifiedRegistry::builder()
            .registry(server.url())
            .supports_semver(false)
            .store(Arc::new(NoopStore))
            .build();

        let result = find_outdated_with_registry(
            temp.path(),
            temp.path(),
            WorkspaceFilter::Current,
            &[],
            registry,
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].package, "foo");
        assert_eq!(result[0].current.as_deref(), Some("1.0.0"));
        assert_eq!(result[0].wanted, "1.1.0");
        assert_eq!(result[0].latest, "2.0.0");
        assert_eq!(result[0].location.as_deref(), Some("node_modules/foo"));
        manifest.assert_async().await;
    }
}
