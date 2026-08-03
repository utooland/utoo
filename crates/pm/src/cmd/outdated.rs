use std::env;

use anyhow::{Context, Result};
use utoo_ruborist::graph::EdgeType;

use crate::helper::ruborist_context::Context as FsContext;
use crate::model::cli_output::{
    DependencyProtocol, DependencyType, OutdatedPackage, OutdatedResult,
};
use crate::service::outdated::{OutdatedProtocol, find_outdated};
use crate::service::workspace::WorkspaceFilter;
use crate::util::cli_enum::OmitType;
use crate::util::format_print::print_outdated;
use crate::util::presenter::emit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutdatedStatus {
    Current,
    Outdated,
}

pub async fn outdated(
    patterns: Vec<String>,
    omit: Vec<OmitType>,
    workspace_filter: WorkspaceFilter,
) -> Result<OutdatedStatus> {
    let cwd = env::current_dir().context("Failed to get current directory")?;
    let discovery = FsContext::discovery();
    let current_project = discovery.find_project_path(&cwd).await?;
    let root_path = discovery.find_root_path(&cwd).await?;
    let items = find_outdated(
        &root_path,
        &current_project,
        workspace_filter,
        &patterns,
        &omit,
    )
    .await?;
    let status = if items.is_empty() {
        OutdatedStatus::Current
    } else {
        OutdatedStatus::Outdated
    };
    let output = OutdatedResult {
        packages: items
            .iter()
            .map(|item| OutdatedPackage {
                package: item.package.clone(),
                registry_package: item.registry_package.clone(),
                protocol: item.protocol.map(|protocol| match protocol {
                    OutdatedProtocol::Catalog => DependencyProtocol::Catalog,
                    OutdatedProtocol::NpmAlias => DependencyProtocol::NpmAlias,
                }),
                dependency_type: match item.dependency_type {
                    EdgeType::Prod => DependencyType::Prod,
                    EdgeType::Dev => DependencyType::Dev,
                    EdgeType::Peer => DependencyType::Peer,
                    EdgeType::Optional => DependencyType::Optional,
                },
                dependent: item.dependent.clone(),
                declared: item.declared.clone(),
                resolved_spec: item.resolved_spec.clone(),
                current: item.current.clone(),
                wanted: item.wanted.clone(),
                latest: item.latest.clone(),
                location: item.location.clone(),
            })
            .collect(),
    };
    emit("outdated", &output, || {
        print_outdated(&items);
        Ok(())
    })?;
    Ok(status)
}
