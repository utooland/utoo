use anyhow::{Context, Result};
use bincode::{Decode, Encode};
use pack_core::library::ecmascript::{EcmascriptLibraryChunk, EcmascriptLibraryEvaluateChunk};
use qstring::QString;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use turbo_rcstr::RcStr;
use turbo_tasks::{FxIndexMap, NonLocalValue, ResolvedVc, TryJoinIterExt, Vc, trace::TraceRawVcs};
use turbo_tasks_fs::FileSystemPath;
use turbopack_browser::ecmascript::{
    EcmascriptBrowserChunk, EcmascriptBrowserEvaluateChunk, EcmascriptDevChunkList,
};
use turbopack_core::{
    asset::Asset,
    chunk::{ChunkItem, ChunkItemExt, ModuleId},
    output::{OutputAsset, OutputAssets},
    reference::all_assets_from_entries,
};
use turbopack_css::chunk::CssChunk;
use turbopack_ecmascript::chunk::EcmascriptChunk;
use turbopack_nodejs::{EcmascriptBuildNodeChunk, EcmascriptBuildNodeEntryChunk};

#[turbo_tasks::value]
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AssetIntermediateInfo {
    pub asset: WebpackStatsAsset,
    pub chunks: Vec<WebpackStatsChunk>,
    pub entrypoints: Vec<(RcStr, WebpackStatsEntrypoint)>,
    pub modules: Vec<WebpackStatsModule>,
    pub dev_chunk_list: Option<RcStr>,
}

#[turbo_tasks::value(transparent)]
pub struct OutputAssetGroups(pub Vec<ResolvedVc<OutputAssets>>);

fn normalize_stats_path(path: RcStr) -> RcStr {
    path.strip_prefix("./").map(Into::into).unwrap_or(path)
}

async fn get_chunk_modules(
    chunk: ResolvedVc<EcmascriptChunk>,
    chunk_id: RcStr,
) -> Result<Vec<WebpackStatsModule>> {
    let content = chunk.chunk_content();
    let module_names = content
        .included_chunk_items()
        .await?
        .iter()
        .map(|chunk_item| {
            let chunk_item = *chunk_item;
            async move {
                let id = chunk_item.id().await?;
                let asset_ident = chunk_item.asset_ident().await?;
                let name = normalize_stats_path(asset_ident.path.path.clone());
                Ok::<_, anyhow::Error>((id, name))
            }
        })
        .try_join()
        .await?
        .into_iter()
        .collect::<FxHashMap<_, _>>();
    let content = content.await?;
    let chunk_items = content.chunk_item_code_module_ids_and_paths().await?;
    let mut modules = Vec::new();

    for item in chunk_items {
        for (id, code, _) in &*item {
            modules.push(WebpackStatsModule {
                name: module_names
                    .get(id)
                    .cloned()
                    .with_context(|| format!("missing source path for module {id}"))?,
                id: id.into(),
                chunks: vec![chunk_id.clone()],
                size: code.source_code().len() as u64,
            });
        }
    }

    Ok(modules)
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
    let path = normalize_stats_path(path);

    let mut local_chunks = vec![];
    let mut local_entrypoints = vec![];
    let mut local_modules = vec![];
    let mut local_dev_chunk_list = None;

    if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptBrowserEvaluateChunk>(asset) {
        let entry_path_full = chunk.path().await?;
        let entry_path = dist_root
            .await?
            .get_relative_path_to(&entry_path_full)
            .unwrap_or_else(|| entry_path_full.path.clone());
        let entry_path = normalize_stats_path(entry_path);
        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![entry_path.clone()],
            id: entry_path.clone(),
            ..Default::default()
        });

        let entry_referenced_assets = chunk.chunks_data().await?;
        let futures: Vec<_> = entry_referenced_assets
            .iter()
            .map(|asset| {
                let asset = *asset;
                async move {
                    let chunk_data = asset.await?;
                    let name = normalize_stats_path(chunk_data.path.as_str().into());
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
        let chunk_ident = normalize_stats_path(chunk_ident);
        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![chunk_ident.clone()],
            id: chunk_ident.clone(),
            ..Default::default()
        });

        let chunk = chunk.chunk().to_resolved().await?;
        if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptChunk>(chunk) {
            local_modules.extend(get_chunk_modules(chunk, chunk_ident).await?);
        }
    }

    if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptBuildNodeChunk>(asset) {
        let chunk_path_full = chunk.path().await?;
        let chunk_ident = dist_root
            .await?
            .get_relative_path_to(&chunk_path_full)
            .unwrap_or_else(|| chunk_path_full.path.clone());
        let chunk_ident = normalize_stats_path(chunk_ident);

        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![chunk_ident.clone()],
            id: chunk_ident.clone(),
            ..Default::default()
        });

        let chunk = chunk.chunk().to_resolved().await?;
        if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptChunk>(chunk) {
            local_modules.extend(get_chunk_modules(chunk, chunk_ident).await?);
        }
    }

    if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptLibraryChunk>(asset) {
        let chunk_path_full = chunk.path().await?;
        let chunk_ident = dist_root
            .await?
            .get_relative_path_to(&chunk_path_full)
            .unwrap_or_else(|| chunk_path_full.path.clone());
        let chunk_ident = normalize_stats_path(chunk_ident);

        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![chunk_ident.clone()],
            id: chunk_ident.clone(),
            ..Default::default()
        });

        local_modules
            .extend(get_chunk_modules(chunk.chunk().to_resolved().await?, chunk_ident).await?);
    }

    if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptBuildNodeEntryChunk>(asset) {
        let entry_path_full = chunk.path().await?;
        let entry_path = dist_root
            .await?
            .get_relative_path_to(&entry_path_full)
            .unwrap_or_else(|| entry_path_full.path.clone());
        let entry_path = normalize_stats_path(entry_path);

        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![entry_path.clone()],
            id: entry_path.clone(),
            ..Default::default()
        });

        let entry_referenced_assets = chunk.chunks_data().await?;
        let futures: Vec<_> = entry_referenced_assets
            .iter()
            .map(|asset| {
                let asset = *asset;
                async move {
                    let chunk_data = asset.await?;
                    let name = normalize_stats_path(chunk_data.path.as_str().into());
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
        let chunk_list_ident = normalize_stats_path(chunk_list_ident);
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
        let chunk_ident = normalize_stats_path(chunk_ident);
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
        let entry_path = normalize_stats_path(entry_path);
        local_chunks.push(WebpackStatsChunk {
            size: asset_len,
            files: vec![entry_path.clone()],
            id: entry_path.clone(),
            ..Default::default()
        });

        local_modules.extend(
            get_chunk_modules(chunk.chunk().to_resolved().await?, entry_path.clone()).await?,
        );

        let entry_referenced_assets = chunk.chunks_data().await?;
        let futures: Vec<_> = entry_referenced_assets
            .iter()
            .map(|asset| {
                let asset = *asset;
                async move {
                    let chunk_data = asset.await?;
                    let name = normalize_stats_path(chunk_data.path.as_str().into());
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
        modules: local_modules,
        dev_chunk_list: local_dev_chunk_list,
    }
    .cell())
}

#[turbo_tasks::function]
pub async fn generate_webpack_stats(
    entry_assets: Vc<OutputAssets>,
    entry_asset_groups: Vc<OutputAssetGroups>,
    dist_root: Vc<FileSystemPath>,
) -> Result<Vc<WebpackStats>> {
    let mut assets = vec![];
    let mut seen_asset_paths = FxHashSet::default();
    let mut chunks = vec![];
    let mut modules: FxIndexMap<WebpackStatsModuleId, WebpackStatsModule> = FxIndexMap::default();
    let mut entrypoints: FxIndexMap<RcStr, WebpackStatsEntrypoint> = FxIndexMap::default();

    // Reuse Turbopack's graph traversal so shared async chunk graphs are expanded once.
    let all_assets = all_assets_from_entries(entry_assets).await?;

    // Iterate over all collected assets in parallel using cached sub-tasks
    let asset_results = all_assets
        .iter()
        .copied()
        .map(|asset| async move {
            let info = get_asset_intermediate_info(*asset, dist_root).await?;
            Ok::<_, anyhow::Error>(info)
        })
        .try_join()
        .await?;
    let asset_info_by_asset: FxHashMap<_, _> = all_assets
        .iter()
        .copied()
        .zip(asset_results.iter())
        .collect();

    for info in &asset_results {
        if seen_asset_paths.insert(info.asset.name.clone()) {
            assets.push(info.asset.clone());
        }
        chunks.extend(info.chunks.iter().cloned());
        for (name, ep) in info.entrypoints.iter() {
            entrypoints.insert(name.clone(), ep.clone());
        }
        for module in &info.modules {
            if let Some(existing) = modules.get_mut(&module.id) {
                for chunk in &module.chunks {
                    if !existing.chunks.contains(chunk) {
                        existing.chunks.push(chunk.clone());
                    }
                }
            } else {
                modules.insert(module.id.clone(), module.clone());
            }
        }
    }

    // Endpoint output groups preserve which evaluate entry owns each development chunk list.
    // Associating these lists after flattening all output assets made every entrypoint include
    // every other page's HMR bootstrap in multi-page builds.
    for group in entry_asset_groups.await?.iter().copied() {
        let group = group.await?;
        let group_entrypoints: FxIndexMap<_, _> = group
            .iter()
            .filter_map(|asset| asset_info_by_asset.get(asset))
            .flat_map(|info| info.entrypoints.iter())
            .map(|(name, _)| (name.clone(), ()))
            .collect();
        let group_chunk_lists: FxIndexMap<_, _> = group
            .iter()
            .filter_map(|asset| asset_info_by_asset.get(asset))
            .filter_map(|info| info.dev_chunk_list.as_ref())
            .map(|name| (name.clone(), ()))
            .collect();

        for entrypoint_name in group_entrypoints.keys() {
            let Some(entrypoint) = entrypoints.get_mut(entrypoint_name) else {
                continue;
            };
            for dev_chunk_list in group_chunk_lists.keys() {
                if entrypoint.chunks.contains(dev_chunk_list) {
                    continue;
                }
                entrypoint.chunks.push(dev_chunk_list.clone());
                entrypoint.assets.push(WebpackStatsEntrypointAssets {
                    name: dev_chunk_list.clone(),
                });
            }
        }
    }

    let modules = modules.into_values().collect::<Vec<_>>();

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

#[derive(
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    TraceRawVcs,
    NonLocalValue,
    Encode,
    Decode,
)]
#[serde(untagged)]
pub enum WebpackStatsModuleId {
    Number(u64),
    String(RcStr),
}

impl From<&ModuleId> for WebpackStatsModuleId {
    fn from(id: &ModuleId) -> Self {
        match id {
            ModuleId::Number(id) => Self::Number(*id),
            ModuleId::String(id) => Self::String(id.clone()),
        }
    }
}

#[turbo_tasks::value]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WebpackStatsModule {
    pub name: RcStr,
    pub id: WebpackStatsModuleId,
    pub chunks: Vec<RcStr>,
    pub size: u64,
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
