use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use futures::stream::{self, StreamExt, TryStreamExt};
use utoo_ruborist::graph::{DependencyGraph, EdgeType};
use utoo_ruborist::lock::LockDependencyIndex;
use utoo_ruborist::manifest::{PackageJson, VersionsRef};
use utoo_ruborist::registry::resolve_target_version;
use utoo_ruborist::service::{
    HttpStatusError, ManifestFullData, ManifestJob, ManifestJobDone, ManifestProvider, VersionsInfo,
};
use utoo_ruborist::spec::{PackageSpec, Protocol, resolve_catalog_spec, resolve_workspace_spec};
use utoo_ruborist::workspace::WorkspacePackage;

use crate::helper::ruborist_context::{Context as FsContext, Registry};
use crate::service::workspace::{WorkspaceFilter, WorkspaceNode, expand_workspace_filters};
use crate::util::cache::matches_pattern;
use crate::util::cli_enum::OmitType;
use crate::util::config_file::Config;
use crate::util::json::{load_package_lock_json_from_path, read_json_file};
use crate::util::user_config::get_manifests_concurrency_limit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutdatedProtocol {
    Catalog,
    NpmAlias,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutdatedInfo {
    pub package: String,
    pub registry_package: String,
    pub protocol: Option<OutdatedProtocol>,
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
    protocol: Option<OutdatedProtocol>,
    dependency_type: EdgeType,
    dependent: String,
    declared: String,
    resolved_spec: String,
    current: Option<String>,
    location: Option<String>,
}

#[derive(Debug)]
enum PackageVersions {
    Fresh {
        versions: Vec<String>,
        dist_tags: HashMap<String, String>,
    },
    Cached(Arc<VersionsInfo>),
}

impl PackageVersions {
    fn as_ref(&self) -> VersionsRef<'_> {
        match self {
            Self::Fresh {
                versions,
                dist_tags,
            } => VersionsRef {
                versions,
                dist_tags,
            },
            Self::Cached(versions) => (&**versions).into(),
        }
    }
}

pub async fn find_outdated(
    root_path: &Path,
    current_project: &Path,
    workspace_filter: WorkspaceFilter,
    patterns: &[String],
    omit: &[OmitType],
) -> Result<Vec<OutdatedInfo>> {
    let registry = FsContext::registry().await;
    find_outdated_with_registry(
        root_path,
        current_project,
        workspace_filter,
        patterns,
        omit,
        registry,
    )
    .await
}

async fn find_outdated_with_registry(
    root_path: &Path,
    current_project: &Path,
    workspace_filter: WorkspaceFilter,
    patterns: &[String],
    omit: &[OmitType],
    registry: Registry,
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
    let discovery = FsContext::discovery();
    let workspaces = discovery
        .find_workspaces_from_pkg(root_path, &root_package)
        .await?;
    let importers = select_importers(
        root_path,
        current_project,
        workspace_filter,
        &root_package,
        &workspaces,
    )?;
    let override_graph = DependencyGraph::from_package_json(root_path.to_path_buf(), root_package);
    let catalogs = Config::load_from_path(&root_path.join(".utoo.toml"))
        .await?
        .catalogs();
    let workspace_versions: HashMap<String, String> = workspaces
        .iter()
        .map(|workspace| {
            (
                workspace.name.clone(),
                workspace.package_json.version.clone(),
            )
        })
        .collect();

    let lock_index = LockDependencyIndex::new(&lock);
    let mut dependencies = Vec::new();
    for importer in importers {
        dependencies.extend(collect_dependencies(
            &importer,
            patterns,
            omit,
            &catalogs,
            &workspace_versions,
            &lock_index,
            &override_graph,
        )?);
    }

    let package_names: BTreeSet<String> = dependencies
        .iter()
        .map(|dependency| dependency.registry_package.clone())
        .collect();
    let manifests = fetch_package_versions(package_names, registry).await?;

    let mut result = Vec::new();
    for dependency in dependencies {
        // A missing manifest here means the registry returned 404 for this
        // package. npm treats that package as not comparable while allowing
        // the remaining direct dependencies to be reported.
        let Some(versions) = manifests.get(&dependency.registry_package) else {
            continue;
        };
        // No matching version is npm's ETARGET-equivalent for this edge. It is
        // likewise local to the package; auth/network/parse failures already
        // escaped from `fetch_package_versions`.
        let Ok(wanted) = resolve_target_version(versions.as_ref(), &dependency.resolved_spec)
        else {
            continue;
        };
        let Some(latest) = versions.as_ref().dist_tags.get("latest").cloned() else {
            continue;
        };
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

fn select_importers(
    root_path: &Path,
    current_project: &Path,
    filter: WorkspaceFilter,
    root_package: &PackageJson,
    workspaces: &[WorkspacePackage],
) -> Result<Vec<Importer>> {
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
                .iter()
                .find(|workspace| workspace.path == current_project)
                .with_context(|| {
                    format!(
                        "current project {} is not a workspace of {}",
                        current_project.display(),
                        root_path.display()
                    )
                })?;
            Ok(vec![to_importer(
                workspace.path.clone(),
                workspace.name.clone(),
                workspace.package_json.clone(),
            )?])
        }
        WorkspaceFilter::All => {
            let workspace_importers = workspaces
                .iter()
                .map(|workspace| {
                    to_importer(
                        workspace.path.clone(),
                        workspace.name.clone(),
                        workspace.package_json.clone(),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let mut importers = Vec::with_capacity(workspace_importers.len() + 1);
            importers.push(root_importer());
            importers.extend(workspace_importers);
            Ok(importers)
        }
        WorkspaceFilter::Selected(filters) => {
            let nodes = workspaces
                .iter()
                .map(|workspace| {
                    let path = workspace
                        .path
                        .strip_prefix(root_path)
                        .with_context(|| {
                            format!(
                                "workspace {} is outside project root",
                                workspace.path.display()
                            )
                        })?
                        .to_path_buf();
                    Ok(WorkspaceNode {
                        name: workspace.name.clone(),
                        path,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let selected = expand_workspace_filters(&nodes, &filters)?;
            workspaces
                .iter()
                .filter(|workspace| selected.contains(&workspace.name))
                .map(|workspace| {
                    to_importer(
                        workspace.path.clone(),
                        workspace.name.clone(),
                        workspace.package_json.clone(),
                    )
                })
                .collect()
        }
    }
}

fn collect_dependencies(
    importer: &Importer,
    patterns: &[String],
    omit: &[OmitType],
    catalogs: &utoo_ruborist::spec::Catalogs,
    workspace_versions: &HashMap<String, String>,
    lock_index: &LockDependencyIndex<'_>,
    override_graph: &DependencyGraph,
) -> Result<Vec<Dependency>> {
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

    let mut result = Vec::with_capacity(dependencies.len());
    for (name, (declared, dependency_type)) in dependencies {
        if is_omitted(dependency_type, omit) {
            continue;
        }
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

        let resolved = lock_index.resolve(&importer.lock_path, &name);
        let (location, current) = resolved.map_or((None, None), |(path, package)| {
            (Some(path.to_string()), package.version.clone())
        });
        if current.is_none() && dependency_type != EdgeType::Prod {
            continue;
        }
        let override_spec =
            override_graph.check_override(override_graph.root_index, &name, current.as_deref());
        let effective_spec = override_spec.as_deref().unwrap_or(&declared);
        let protocol = Protocol::strip_prefix(effective_spec).map(|(protocol, _)| protocol);

        let resolved_spec = if protocol == Some(Protocol::Catalog) {
            resolve_catalog_spec(&name, effective_spec, catalogs)
                .with_context(|| format!("cannot resolve {name} from {effective_spec}"))?
                .to_string()
        } else {
            effective_spec.to_string()
        };

        let (registry_package, version_spec, protocol) =
            match PackageSpec::from(resolved_spec.as_str()) {
                PackageSpec::Registry {
                    name: alias_name,
                    version_spec,
                } if protocol == Some(Protocol::NpmAlias) => {
                    (alias_name, version_spec, Some(OutdatedProtocol::NpmAlias))
                }
                PackageSpec::Registry { .. }
                    if protocol.is_none() || protocol == Some(Protocol::Catalog) =>
                {
                    let protocol =
                        (protocol == Some(Protocol::Catalog)).then_some(OutdatedProtocol::Catalog);
                    (name.clone(), resolved_spec, protocol)
                }
                _ => continue,
            };

        result.push(Dependency {
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
    Ok(result)
}

fn is_omitted(dependency_type: EdgeType, omit: &[OmitType]) -> bool {
    let omitted_type = match dependency_type {
        EdgeType::Prod => return false,
        EdgeType::Dev => OmitType::Dev,
        EdgeType::Peer => OmitType::Peer,
        EdgeType::Optional => OmitType::Optional,
    };
    omit.contains(&omitted_type)
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
    registry: Registry,
) -> Result<HashMap<String, PackageVersions>> {
    let concurrency = get_manifests_concurrency_limit().await.max(1);
    let results: Vec<Option<(String, PackageVersions)>> = stream::iter(package_names)
        .map(|name| {
            let registry = registry.clone();
            async move {
                let result = registry
                    .execute_manifest_job(ManifestJob::Full {
                        name: name.clone(),
                        spec: None,
                    })
                    .await
                    .map_err(anyhow::Error::new)
                    .with_context(|| format!("failed to fetch manifest for {name}"));
                let result = match result {
                    Ok(result) => result,
                    Err(error) if http_status(&error) == Some(404) => return Ok(None),
                    Err(error) => return Err(error),
                };
                let versions = match result {
                    ManifestJobDone::Full { data, .. } => match data {
                        ManifestFullData::Full { manifest, .. } => PackageVersions::Fresh {
                            versions: manifest.versions.clone(),
                            dist_tags: manifest.dist_tags.clone(),
                        },
                        ManifestFullData::Versions(info) => PackageVersions::Cached(info),
                    },
                    ManifestJobDone::Version { .. } => {
                        bail!("full manifest request returned a version response")
                    }
                };
                Ok::<_, anyhow::Error>(Some((name, versions)))
            }
        })
        .buffer_unordered(concurrency)
        .try_collect()
        .await?;

    Ok(results.into_iter().flatten().collect())
}

fn http_status(error: &anyhow::Error) -> Option<u16> {
    error.chain().find_map(|source| {
        let reqwest_status = source
            .downcast_ref::<reqwest::Error>()
            .and_then(reqwest::Error::status)
            .map(|status| status.as_u16());
        reqwest_status.or_else(|| {
            source
                .downcast_ref::<HttpStatusError>()
                .map(HttpStatusError::status)
        })
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use utoo_ruborist::lock::PackageLock;
    use utoo_ruborist::service::{NoopStore, UnifiedRegistry};

    use super::*;

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
        let lock = PackageLock::new("app", "1.0.0", HashMap::new());
        let lock_index = LockDependencyIndex::new(&lock);
        let override_graph =
            DependencyGraph::from_package_json(PathBuf::new(), importer.package_json.clone());

        let mut dependencies = collect_dependencies(
            &importer,
            &[],
            &[],
            &catalogs,
            &HashMap::new(),
            &lock_index,
            &override_graph,
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
        let lock_index = LockDependencyIndex::new(&lock);
        let override_graph = DependencyGraph::from_package_json(PathBuf::new(), package_json);

        let dependencies = collect_dependencies(
            &importer,
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &lock_index,
            &override_graph,
        )
        .unwrap();

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].declared, "^1.0.0");
        assert_eq!(dependencies[0].resolved_spec, "1.0.0");
        assert_eq!(dependencies[0].current.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn expands_catalog_used_by_an_override() {
        let package_json: PackageJson = serde_json::from_str(
            r#"{
              "name":"app",
              "dependencies":{"foo":"^1.0.0"},
              "overrides":{"foo":"catalog:next"}
            }"#,
        )
        .unwrap();
        let importer = Importer {
            name: "app".to_string(),
            lock_path: String::new(),
            package_json: package_json.clone(),
        };
        let catalogs = HashMap::from([(
            "next".to_string(),
            HashMap::from([("foo".to_string(), "^2.0.0".to_string())]),
        )]);
        let lock = PackageLock::new("app", "1.0.0", HashMap::new());
        let lock_index = LockDependencyIndex::new(&lock);
        let override_graph = DependencyGraph::from_package_json(PathBuf::new(), package_json);

        let dependencies = collect_dependencies(
            &importer,
            &[],
            &[],
            &catalogs,
            &HashMap::new(),
            &lock_index,
            &override_graph,
        )
        .unwrap();

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].protocol, Some(OutdatedProtocol::Catalog));
        assert_eq!(dependencies[0].resolved_spec, "^2.0.0");
    }

    #[test]
    fn optional_declaration_wins_and_omit_filters_dependency_types() {
        let package_json: PackageJson = serde_json::from_str(
            r#"{
              "name":"app",
              "dependencies":{"shared":"^1.0.0","prod":"^1.0.0"},
              "devDependencies":{"dev":"^1.0.0"},
              "peerDependencies":{"peer":"^1.0.0"},
              "optionalDependencies":{"shared":"^2.0.0"}
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
              "packages":{
                "node_modules/prod":{"name":"prod","version":"1.0.0"},
                "node_modules/shared":{"name":"shared","version":"2.0.0"}
              }
            }"#,
        )
        .unwrap();
        let lock_index = LockDependencyIndex::new(&lock);
        let override_graph = DependencyGraph::from_package_json(PathBuf::new(), package_json);

        let mut dependencies = collect_dependencies(
            &importer,
            &[],
            &[OmitType::Dev, OmitType::Peer],
            &HashMap::new(),
            &HashMap::new(),
            &lock_index,
            &override_graph,
        )
        .unwrap();
        dependencies.sort_by(|a, b| a.package.cmp(&b.package));

        assert_eq!(dependencies.len(), 2);
        assert_eq!(dependencies[0].package, "prod");
        assert_eq!(dependencies[0].dependency_type, EdgeType::Prod);
        assert_eq!(dependencies[1].package, "shared");
        assert_eq!(dependencies[1].dependency_type, EdgeType::Optional);
        assert_eq!(dependencies[1].declared, "^2.0.0");
    }

    fn test_registry(url: String) -> UnifiedRegistry {
        let supports_semver = false;
        UnifiedRegistry::builder()
            .registry(url)
            .supports_semver(supports_semver)
            .store(Arc::new(NoopStore))
            .build()
    }

    async fn write_single_dependency_project(path: &Path, dependency_spec: &str, current: &str) {
        crate::fs::write(
            path.join("package.json"),
            format!(
                r#"{{"name":"app","version":"1.0.0","dependencies":{{"foo":"{dependency_spec}"}}}}"#
            ),
        )
        .await
        .unwrap();
        crate::fs::write(
            path.join("package-lock.json"),
            format!(
                r#"{{
                  "name":"app",
                  "version":"1.0.0",
                  "lockfileVersion":3,
                  "packages":{{
                    "":{{"name":"app","version":"1.0.0","dependencies":{{"foo":"{dependency_spec}"}}}},
                    "node_modules/foo":{{"name":"foo","version":"{current}"}}
                  }}
                }}"#
            ),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn missing_registry_package_is_skipped_but_auth_failure_is_fatal() {
        let mut missing_server = mockito::Server::new_async().await;
        let missing = missing_server
            .mock("GET", "/missing")
            .with_status(404)
            .with_body("not found")
            .create_async()
            .await;
        let missing_result = fetch_package_versions(
            BTreeSet::from(["missing".to_string()]),
            test_registry(missing_server.url()),
        )
        .await
        .unwrap();
        assert!(missing_result.is_empty());
        missing.assert_async().await;

        let mut auth_server = mockito::Server::new_async().await;
        let unauthorized = auth_server
            .mock("GET", "/private")
            .with_status(401)
            .with_body("unauthorized")
            .create_async()
            .await;
        let error = fetch_package_versions(
            BTreeSet::from(["private".to_string()]),
            test_registry(auth_server.url()),
        )
        .await
        .unwrap_err();
        assert_eq!(http_status(&error), Some(401));
        assert!(error.to_string().contains("private"));
        unauthorized.assert_async().await;
    }

    #[tokio::test]
    async fn current_latest_is_still_reported_when_it_differs_from_wanted() {
        let temp = tempfile::tempdir().unwrap();
        write_single_dependency_project(temp.path(), "^1.0.0", "2.0.0").await;
        let mut server = mockito::Server::new_async().await;
        let manifest = server
            .mock("GET", "/foo")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                  "name":"foo",
                  "dist-tags":{"latest":"2.0.0"},
                  "versions":{
                    "1.1.0":{"name":"foo","version":"1.1.0"},
                    "2.0.0":{"name":"foo","version":"2.0.0"}
                  }
                }"#,
            )
            .create_async()
            .await;

        let result = find_outdated_with_registry(
            temp.path(),
            temp.path(),
            WorkspaceFilter::Current,
            &[],
            &[],
            test_registry(server.url()),
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].current.as_deref(), Some("2.0.0"));
        assert_eq!(result[0].wanted, "1.1.0");
        assert_eq!(result[0].latest, "2.0.0");
        manifest.assert_async().await;
    }

    #[tokio::test]
    async fn no_matching_wanted_version_is_skipped() {
        let temp = tempfile::tempdir().unwrap();
        write_single_dependency_project(temp.path(), "^3.0.0", "1.0.0").await;
        let mut server = mockito::Server::new_async().await;
        let manifest = server
            .mock("GET", "/foo")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                  "name":"foo",
                  "dist-tags":{"latest":"1.0.0"},
                  "versions":{"1.0.0":{"name":"foo","version":"1.0.0"}}
                }"#,
            )
            .create_async()
            .await;

        let result = find_outdated_with_registry(
            temp.path(),
            temp.path(),
            WorkspaceFilter::Current,
            &[],
            &[],
            test_registry(server.url()),
        )
        .await
        .unwrap();

        assert!(result.is_empty());
        manifest.assert_async().await;
    }

    #[tokio::test]
    async fn missing_non_production_dependency_does_not_fetch_a_manifest() {
        let temp = tempfile::tempdir().unwrap();
        crate::fs::write(
            temp.path().join("package.json"),
            r#"{"name":"app","version":"1.0.0","devDependencies":{"foo":"^1.0.0"}}"#,
        )
        .await
        .unwrap();
        crate::fs::write(
            temp.path().join("package-lock.json"),
            r#"{
              "name":"app",
              "version":"1.0.0",
              "lockfileVersion":3,
              "packages":{"":{"name":"app","version":"1.0.0"}}
            }"#,
        )
        .await
        .unwrap();
        let server = mockito::Server::new_async().await;

        let result = find_outdated_with_registry(
            temp.path(),
            temp.path(),
            WorkspaceFilter::Current,
            &[],
            &[],
            test_registry(server.url()),
        )
        .await
        .unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn malformed_catalog_config_is_fatal() {
        let temp = tempfile::tempdir().unwrap();
        write_single_dependency_project(temp.path(), "^1.0.0", "1.0.0").await;
        crate::fs::write(temp.path().join(".utoo.toml"), "[catalog\ninvalid")
            .await
            .unwrap();
        let server = mockito::Server::new_async().await;

        let error = find_outdated_with_registry(
            temp.path(),
            temp.path(),
            WorkspaceFilter::Current,
            &[],
            &[],
            test_registry(server.url()),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("TOML"));
    }

    #[tokio::test]
    async fn finds_outdated_direct_dependency_from_lock_and_registry() {
        let temp = tempfile::tempdir().unwrap();
        crate::fs::write(
            temp.path().join("package.json"),
            r#"{"name":"app","version":"1.0.0","dependencies":{"foo":"^1.0.0"}}"#,
        )
        .await
        .unwrap();
        crate::fs::write(
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
        .await
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
        let registry = test_registry(server.url());

        let result = find_outdated_with_registry(
            temp.path(),
            temp.path(),
            WorkspaceFilter::Current,
            &[],
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
