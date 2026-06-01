use anyhow::Result;
use tracing::Instrument;
use turbo_rcstr::RcStr;
use turbo_tasks::{ReadRef, ResolvedVc, TryFlatJoinIterExt, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbo_tasks_hash::HashAlgorithm;
use turbopack_core::{
    asset::Asset,
    output::{OutputAsset, OutputAssets},
    reference::all_assets_from_entries,
};

/// A reference to a server file with content hash for change detection
#[turbo_tasks::value]
#[derive(Debug, Clone)]
pub struct ServerPath {
    /// Relative to the root_path
    pub path: String,
    pub content_hash: RcStr,
}

/// A list of server paths
#[turbo_tasks::value(transparent)]
pub struct ServerPaths(Vec<ServerPath>);

/// Return a list of all server paths with filename and hash for all output
/// assets references from the `assets` list. Server paths are identified by
/// being inside `node_root`.
#[turbo_tasks::function]
pub async fn all_server_paths(
    assets: Vc<OutputAssets>,
    node_root: Vc<FileSystemPath>,
) -> Result<Vc<ServerPaths>> {
    let span = tracing::trace_span!("all_server_paths");
    async move {
        let all_assets = all_assets_from_entries(assets).await?;
        let node_root = &node_root.await?;
        Ok(Vc::cell(
            all_assets
                .iter()
                .map(|&asset| async move {
                    Ok(
                        if let Some(path) = node_root.get_path_to(&*asset.path().await?) {
                            let content_hash = ReadRef::into_owned(
                                asset.content().hash(HashAlgorithm::Xxh3Hash64Hex).await?,
                            );
                            Some(ServerPath {
                                path: path.to_string(),
                                content_hash,
                            })
                        } else {
                            None
                        },
                    )
                })
                .try_flat_join()
                .await?,
        ))
    }
    .instrument(span)
    .await
}

/// Return a list of relative paths to `root` for all output assets references
/// from the `assets` list which are located inside the root path.
#[turbo_tasks::function]
pub async fn all_paths_in_root(
    assets: Vc<OutputAssets>,
    root: Vc<FileSystemPath>,
) -> Result<Vc<Vec<RcStr>>> {
    let all_assets = &*all_assets_from_entries(assets).await?;
    let root = &*root.await?;

    Ok(Vc::cell(
        get_paths_from_root(root, all_assets, |_| true).await?,
    ))
}

pub(crate) async fn get_paths_from_root(
    root: &FileSystemPath,
    output_assets: impl IntoIterator<Item = &ResolvedVc<Box<dyn OutputAsset>>>,
    filter: impl FnOnce(&str) -> bool + Copy,
) -> Result<Vec<RcStr>> {
    output_assets
        .into_iter()
        .map(move |&file| async move {
            let path = &*file.path().await?;
            let Some(relative) = root.get_path_to(path) else {
                return Ok(None);
            };

            Ok(if filter(relative) {
                Some(relative.into())
            } else {
                None
            })
        })
        .try_flat_join()
        .await
}
