use anyhow::Result;
use pack_core::library::ecmascript::EcmascriptLibraryEvaluateChunk;
use qstring::QString;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use turbo_rcstr::RcStr;
use turbo_tasks::{FxIndexMap, FxIndexSet, ResolvedVc, TryJoinIterExt, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbopack_browser::ecmascript::{
    EcmascriptBrowserChunk, EcmascriptBrowserEvaluateChunk, EcmascriptDevChunkList,
};
use turbopack_core::{
    asset::Asset,
    chunk::{Chunk, ChunkItem, ChunkableModule},
    output::{OutputAsset, OutputAssets, OutputAssetsReference},
};
use turbopack_css::chunk::CssChunk;
use turbopack_nodejs::{EcmascriptBuildNodeChunk, EcmascriptBuildNodeEntryChunk};

#[turbo_tasks::value]
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AssetIntermediateInfo {
    pub asset: WebpackStatsAsset,
    pub chunks: Vec<WebpackStatsChunk>,
    pub entrypoints: Vec<(RcStr, WebpackStatsEntrypoint)>,
    pub chunk_items: Vec<(ResolvedVc<Box<dyn ChunkItem>>, RcStr)>,
    pub dev_chunk_list: Option<RcStr>,
}

#[turbo_tasks::function]
pub async fn get_asset_intermediate_info(
    asset: ResolvedVc<Box<dyn OutputAsset>>,
    dist_root: Vc<FileSystemPath>,
) -> Result<Vc<AssetIntermediateInfo>> {
    let asset_len = asset.content().len().await?.unwrap_or_default();
    let asset_path_full = asset.path().await?;
    let path = dist_root
        .await?
        .get_relative_path_to(&asset_path_full)
        .unwrap_or_else(|| asset_path_full.path.clone());
    // Remove leading "./" prefix if present
    let path: RcStr = path.strip_prefix("./").map(|s| s.into()).unwrap_or(path);

    let mut local_chunks = vec![];
    let mut local_entrypoints = vec![];
    let mut local_chunk_items = vec![];
    let mut local_dev_chunk_list = None;

    if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptBrowserEvaluateChunk>(asset) {
        let entry_path_full = chunk.path().await?;
        let entry_path = dist_root
            .await?
            .get_relative_path_to(&entry_path_full)
            .unwrap_or_else(|| entry_path_full.path.clone());
        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![entry_path.clone()],
            id: entry_path.clone(),
            ..Default::default()
        });

        let evaluatable_assets = chunk.evaluatable_assets().await?;
        let items: Vec<_> = evaluatable_assets
            .iter()
            .map(|&evaluatable_asset| {
                let entry_path = entry_path.clone();
                async move {
                    let item = evaluatable_asset
                        .as_chunk_item(chunk.module_graph(), chunk.chunking_context());
                    Ok::<_, anyhow::Error>((item.to_resolved().await?, entry_path))
                }
            })
            .try_join()
            .await?;
        local_chunk_items.extend(items);

        let entry_referenced_assets = chunk.chunks_data().await?;
        let futures: Vec<_> = entry_referenced_assets
            .iter()
            .map(|asset| {
                let asset = *asset;
                async move {
                    let chunk_data = asset.await?;
                    let name: RcStr = chunk_data.path.as_str().into();
                    Ok::<_, anyhow::Error>((name.clone(), WebpackStatsEntrypointAssets { name }))
                }
            })
            .collect();

        let results = futures::future::try_join_all(futures).await?;
        let mut entry_chunks = Vec::with_capacity(results.len() + 1);
        let mut entry_assets_list = Vec::with_capacity(results.len() + 1);

        for (chunk_name, asset_info) in results {
            entry_chunks.push(chunk_name);
            entry_assets_list.push(asset_info);
        }

        let mut entry_chunks = entry_chunks;
        entry_chunks.push(entry_path.clone());
        entry_assets_list.push(WebpackStatsEntrypointAssets {
            name: entry_path.clone(),
        });

        let entry_name: RcStr = QString::from(chunk.ident().await?.query.as_str())
            .get("name")
            .unwrap_or(remove_extension_from_str(entry_path.as_str()))
            .into();

        local_entrypoints.push((
            entry_name.clone(),
            WebpackStatsEntrypoint {
                name: entry_name,
                chunks: entry_chunks,
                assets: entry_assets_list,
            },
        ));
    }

    if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptBrowserChunk>(asset) {
        let chunk_path_full = chunk.path().await?;
        let chunk_ident = dist_root
            .await?
            .get_relative_path_to(&chunk_path_full)
            .unwrap_or_else(|| chunk_path_full.path.clone());
        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![chunk_ident.clone()],
            id: chunk_ident.clone(),
            ..Default::default()
        });

        let chunk_items = chunk.chunk().chunk_items().await?;
        for item in chunk_items.iter() {
            local_chunk_items.push((*item, chunk_ident.clone()));
        }
    }

    if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptBuildNodeChunk>(asset) {
        let chunk_path_full = chunk.path().await?;
        let chunk_ident = dist_root
            .await?
            .get_relative_path_to(&chunk_path_full)
            .unwrap_or_else(|| chunk_path_full.path.clone());

        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![chunk_ident.clone()],
            id: chunk_ident.clone(),
            ..Default::default()
        });

        let chunk_items = chunk.chunk().chunk_items().await?;
        for item in chunk_items.iter() {
            local_chunk_items.push((*item, chunk_ident.clone()));
        }
    }

    if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptBuildNodeEntryChunk>(asset) {
        let entry_path_full = chunk.path().await?;
        let entry_path = dist_root
            .await?
            .get_relative_path_to(&entry_path_full)
            .unwrap_or_else(|| entry_path_full.path.clone());

        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![entry_path.clone()],
            id: entry_path.clone(),
            ..Default::default()
        });

        let evaluatable_assets = chunk.evaluatable_assets().await?;
        let items: Vec<_> = evaluatable_assets
            .iter()
            .map(|&evaluatable_asset| {
                let entry_path = entry_path.clone();
                async move {
                    let item = evaluatable_asset
                        .as_chunk_item(chunk.module_graph(), chunk.chunking_context());
                    Ok::<_, anyhow::Error>((item.to_resolved().await?, entry_path))
                }
            })
            .try_join()
            .await?;
        local_chunk_items.extend(items);

        let entry_referenced_assets = chunk.chunks_data().await?;
        let futures: Vec<_> = entry_referenced_assets
            .iter()
            .map(|asset| {
                let asset = *asset;
                async move {
                    let chunk_data = asset.await?;
                    let name: RcStr = chunk_data.path.as_str().into();
                    Ok::<_, anyhow::Error>((name.clone(), WebpackStatsEntrypointAssets { name }))
                }
            })
            .collect();

        let results = futures::future::try_join_all(futures).await?;
        let mut entry_chunks = Vec::with_capacity(results.len() + 1);
        let mut entry_assets_list = Vec::with_capacity(results.len() + 1);

        for (chunk_name, asset_info) in results {
            entry_chunks.push(chunk_name);
            entry_assets_list.push(asset_info);
        }

        entry_chunks.push(entry_path.clone());
        entry_assets_list.push(WebpackStatsEntrypointAssets {
            name: entry_path.clone(),
        });

        let entry_name: RcStr = remove_extension_from_str(entry_path.as_str()).into();

        local_entrypoints.push((
            entry_name.clone(),
            WebpackStatsEntrypoint {
                name: entry_name,
                chunks: entry_chunks,
                assets: entry_assets_list,
            },
        ));
    }

    if let Some(chunk_list) = ResolvedVc::try_downcast_type::<EcmascriptDevChunkList>(asset) {
        let chunk_list_path_full = chunk_list.path().await?;
        let chunk_list_ident = dist_root
            .await?
            .get_relative_path_to(&chunk_list_path_full)
            .unwrap_or_else(|| chunk_list_path_full.path.clone());
        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![chunk_list_ident.clone()],
            id: chunk_list_ident.clone(),
            ..Default::default()
        });

        local_dev_chunk_list = Some(chunk_list_ident);
    }

    if let Some(chunk) = ResolvedVc::try_downcast_type::<CssChunk>(asset) {
        let chunk_path_full = chunk.path().await?;
        let chunk_ident = dist_root
            .await?
            .get_relative_path_to(&chunk_path_full)
            .unwrap_or_else(|| chunk_path_full.path.clone());
        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![chunk_ident.clone()],
            id: chunk_ident.clone(),
            ..Default::default()
        });
    }

    if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptLibraryEvaluateChunk>(asset) {
        let entry_path_full = chunk.path().await?;
        let entry_path = dist_root
            .await?
            .get_relative_path_to(&entry_path_full)
            .unwrap_or_else(|| entry_path_full.path.clone());
        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![entry_path.clone()],
            id: entry_path.clone(),
            ..Default::default()
        });

        let evaluatable_assets = chunk.evaluatable_assets().await?;
        let items: Vec<_> = evaluatable_assets
            .iter()
            .map(|&evaluatable_asset| {
                let entry_path = entry_path.clone();
                async move {
                    let item = evaluatable_asset
                        .as_chunk_item(chunk.module_graph(), chunk.chunking_context());
                    Ok::<_, anyhow::Error>((item.to_resolved().await?, entry_path))
                }
            })
            .try_join()
            .await?;
        local_chunk_items.extend(items);

        let entry_referenced_assets = chunk.chunks_data().await?;
        let futures: Vec<_> = entry_referenced_assets
            .iter()
            .map(|asset| {
                let asset = *asset;
                async move {
                    let chunk_data = asset.await?;
                    let name: RcStr = chunk_data.path.as_str().into();
                    Ok::<_, anyhow::Error>((name.clone(), WebpackStatsEntrypointAssets { name }))
                }
            })
            .collect();

        let results = futures::future::try_join_all(futures).await?;
        let mut entry_chunks = Vec::with_capacity(results.len() + 1);
        let mut entry_assets_list = Vec::with_capacity(results.len() + 1);

        for (chunk_name, asset_info) in results {
            entry_chunks.push(chunk_name);
            entry_assets_list.push(asset_info);
        }

        let mut entry_chunks = entry_chunks;
        entry_chunks.push(entry_path.clone());
        entry_assets_list.push(WebpackStatsEntrypointAssets {
            name: entry_path.clone(),
        });

        let entry_name: RcStr = QString::from(chunk.ident().await?.query.as_str())
            .get("name")
            .unwrap_or(remove_extension_from_str(entry_path.as_str()))
            .into();

        local_entrypoints.push((
            entry_name.clone(),
            WebpackStatsEntrypoint {
                name: entry_name,
                chunks: entry_chunks,
                assets: entry_assets_list,
            },
        ));
    }

    let local_asset = WebpackStatsAsset {
        ty: "asset".into(),
        name: path.clone(),
        chunks: vec![path],
        size: asset_len,
        ..Default::default()
    };

    Ok(AssetIntermediateInfo {
        asset: local_asset,
        chunks: local_chunks,
        entrypoints: local_entrypoints,
        chunk_items: local_chunk_items,
        dev_chunk_list: local_dev_chunk_list,
    }
    .cell())
}

#[turbo_tasks::function]
pub async fn generate_webpack_stats(
    entry_assets: Vc<OutputAssets>,
    dist_root: Vc<FileSystemPath>,
) -> Result<Vc<WebpackStats>> {
    let mut assets = vec![];
    let mut seen_asset_paths = FxHashSet::default();
    let mut chunks = vec![];
    let mut chunk_items: FxIndexMap<Vc<Box<dyn ChunkItem>>, FxIndexSet<RcStr>> =
        FxIndexMap::default();
    let mut entrypoints: FxIndexMap<RcStr, WebpackStatsEntrypoint> = FxIndexMap::default();

    let entry_assets_read = entry_assets.await?;
    let entry_assets_list = entry_assets_read.iter().copied().collect::<Vec<_>>();

    // Collect all assets including referenced assets (async chunks) in parallel
    let asset_children = {
        let mut asset_children =
            FxIndexMap::with_capacity_and_hasher(entry_assets_list.len(), Default::default());
        let mut visited =
            FxHashSet::with_capacity_and_hasher(entry_assets_list.len(), Default::default());
        let mut queue = entry_assets_list.clone();
        while !queue.is_empty() {
            let current_batch = std::mem::take(&mut queue);
            let next_batches = current_batch
                .into_iter()
                .filter(|&asset| visited.insert(asset))
                .map(|asset| async move {
                    let references = asset.references().all_assets();
                    let references_read = references.await?;
                    Ok::<_, anyhow::Error>((asset, references, references_read))
                })
                .collect::<Vec<_>>();

            let results = futures::future::try_join_all(next_batches).await?;
            for (asset, references, references_read) in results {
                asset_children.insert(*asset, references);
                queue.extend(references_read.iter().copied());
            }
        }
        asset_children
    };

    // Iterate over all collected assets in parallel using cached sub-tasks
    let asset_results = asset_children
        .keys()
        .copied()
        .map(|asset: Vc<Box<dyn OutputAsset>>| async move {
            let asset_resolved = asset.to_resolved().await?;
            let info = get_asset_intermediate_info(*asset_resolved, dist_root).await?;
            Ok::<_, anyhow::Error>(info)
        })
        .try_join()
        .await?;

    let mut dev_chunk_lists: Vec<RcStr> = vec![];
    for info in asset_results {
        let info = info;
        if seen_asset_paths.insert(info.asset.name.clone()) {
            assets.push(info.asset.clone());
        }
        chunks.extend(info.chunks.iter().cloned());
        for (name, ep) in info.entrypoints.iter() {
            entrypoints.insert(name.clone(), ep.clone());
        }
        for (item, chunk_id) in info.chunk_items.iter() {
            chunk_items
                .entry(**item)
                .or_default()
                .insert(chunk_id.clone());
        }
        if let Some(dev_chunk_list) = &info.dev_chunk_list {
            dev_chunk_lists.push(dev_chunk_list.clone());
        }
    }

    for dev_chunk_list in dev_chunk_lists {
        for entrypoint in entrypoints.values_mut() {
            entrypoint.chunks.push(dev_chunk_list.clone());
            entrypoint.assets.push(WebpackStatsEntrypointAssets {
                name: dev_chunk_list.clone(),
            });
        }
    }

    let modules = chunk_items
        .into_iter()
        .map(|(chunk_item, chunk_ids)| async move {
            let content_ident = chunk_item.content_ident().await?;
            let asset_path = chunk_item.asset_ident().await?.path.clone();
            let size = content_ident
                .path
                .read()
                .await
                .ok()
                .and_then(|file_content| {
                    file_content.as_content().map(|f| f.content().len() as u64)
                });
            let path = asset_path.path.clone();
            Ok::<_, anyhow::Error>(WebpackStatsModule {
                name: path.clone(),
                id: path.clone(),
                chunks: chunk_ids.into_iter().collect(),
                size,
            })
        })
        .try_join()
        .await?;

    #[cfg(feature = "test")]
    let modules = {
        let mut modules = modules;
        sort_stats_for_tests(&mut assets, &mut chunks, &mut modules, &mut entrypoints);
        modules
    };

    Ok(WebpackStats {
        assets,
        entrypoints,
        chunks,
        modules,
    }
    .cell())
}

fn remove_extension_from_str(filename: &str) -> &str {
    if let Some(dot_index) = filename.rfind('.')
        && dot_index > 0
    {
        return &filename[..dot_index];
    }
    filename
}

#[cfg(feature = "test")]
fn sort_stats_for_tests(
    assets: &mut [WebpackStatsAsset],
    chunks: &mut [WebpackStatsChunk],
    modules: &mut [WebpackStatsModule],
    entrypoints: &mut FxIndexMap<RcStr, WebpackStatsEntrypoint>,
) {
    assets.sort_by(|a, b| a.name.cmp(&b.name));
    chunks.sort_by(|a, b| a.id.cmp(&b.id));
    modules.sort_by(|a, b| a.id.cmp(&b.id));

    let mut entrypoint_pairs = std::mem::take(entrypoints).into_iter().collect::<Vec<_>>();
    entrypoint_pairs.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, mut entrypoint) in entrypoint_pairs {
        entrypoint.chunks.sort();
        entrypoint.assets.sort_by(|a, b| a.name.cmp(&b.name));
        entrypoints.insert(name, entrypoint);
    }
}

#[turbo_tasks::value]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WebpackStatsAssetInfo {}

#[turbo_tasks::value]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WebpackStatsAsset {
    #[serde(rename = "type")]
    pub ty: RcStr,
    pub name: RcStr,
    pub info: WebpackStatsAssetInfo,
    pub size: u64,
    pub emitted: bool,
    pub compared_for_emit: bool,
    pub cached: bool,
    pub chunks: Vec<RcStr>,
}

#[turbo_tasks::value]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WebpackStatsChunk {
    pub rendered: bool,
    pub initial: bool,
    pub entry: bool,
    pub recorded: bool,
    pub id: RcStr,
    pub size: u64,
    pub hash: RcStr,
    pub files: Vec<RcStr>,
}

#[turbo_tasks::value]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WebpackStatsModule {
    pub name: RcStr,
    pub id: RcStr,
    pub chunks: Vec<RcStr>,
    pub size: Option<u64>,
}

#[turbo_tasks::value]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WebpackStatsEntrypointAssets {
    pub name: RcStr,
}

#[turbo_tasks::value]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WebpackStatsEntrypoint {
    pub name: RcStr,
    pub chunks: Vec<RcStr>,
    pub assets: Vec<WebpackStatsEntrypointAssets>,
}

#[turbo_tasks::value(serialization = "skip")]
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WebpackStats {
    pub entrypoints: FxIndexMap<RcStr, WebpackStatsEntrypoint>,
    pub chunks: Vec<WebpackStatsChunk>,
    pub assets: Vec<WebpackStatsAsset>,
    pub modules: Vec<WebpackStatsModule>,
}
