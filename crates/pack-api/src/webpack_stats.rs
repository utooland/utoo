use std::sync::Arc;

use anyhow::{Error, Result};
use parking_lot::Mutex;
use qstring::QString;
use turbo_rcstr::RcStr;
use turbo_tasks::{FxIndexMap, FxIndexSet, ResolvedVc, TryJoinIterExt, Vc};
use turbopack::css::chunk::CssChunk;
use turbopack_browser::ecmascript::{EcmascriptBrowserChunk, EcmascriptBrowserEvaluateChunk};
use turbopack_core::{
    chunk::{Chunk, ChunkItem, ChunkableModule},
    output::{OutputAsset, OutputAssets},
};

#[turbo_tasks::function]
pub async fn generate_webpack_stats(output_assets: Vc<OutputAssets>) -> Result<Vc<WebpackStats>> {
    let assets: Arc<Mutex<Vec<WebpackStatsAsset>>> = Arc::new(Mutex::new(vec![]));
    let chunks: Arc<Mutex<Vec<WebpackStatsChunk>>> = Arc::new(Mutex::new(vec![]));
    #[allow(clippy::type_complexity)]
    let chunk_items: Arc<Mutex<FxIndexMap<Vc<Box<dyn ChunkItem>>, FxIndexSet<RcStr>>>> =
        Arc::new(Mutex::new(FxIndexMap::default()));
    let modules: Arc<Mutex<Vec<WebpackStatsModule>>> = Arc::new(Mutex::new(vec![]));
    let entrypoints: Arc<Mutex<FxIndexMap<RcStr, WebpackStatsEntrypoint>>> =
        Arc::new(Mutex::new(FxIndexMap::default()));

    let output_assets = &*output_assets.await?;
    output_assets
        .iter()
        .map(|asset| {
            let chunks = chunks.clone();
            let chunk_items = chunk_items.clone();
            let entrypoints = entrypoints.clone();
            let assets = assets.clone();
            async move {
                let asset_len = asset.size_bytes().await?.unwrap_or_default();

                if let Some(chunk) =
                    ResolvedVc::try_downcast_type::<EcmascriptBrowserEvaluateChunk>(*asset)
                {
                    let entry_path = chunk.path().await?.path.clone();
                    {
                        let mut chunks = chunks.lock();
                        chunks.push(WebpackStatsChunk {
                            size: asset_len,
                            files: vec![entry_path.clone()],
                            id: entry_path.clone(),
                            ..Default::default()
                        });
                    }

                    chunk
                        .evaluatable_assets()
                        .await?
                        .iter()
                        .for_each(|evaluatable_asset| {
                            let item = evaluatable_asset
                                .as_chunk_item(chunk.module_graph(), chunk.chunking_context());
                            {
                                let mut chunk_items = chunk_items.lock();
                                chunk_items
                                    .entry(item)
                                    .or_default()
                                    .insert(entry_path.clone());
                            }
                        });

                    let entry_referenced_assets = (*chunk).chunks_data().await?;
                    let mut entry_chunks = entry_referenced_assets
                        .iter()
                        .map(async |asset| Ok(asset.await?.path.as_str().into()))
                        .try_join()
                        .await?;
                    entry_chunks.push(entry_path.clone());

                    let mut entry_assets = entry_referenced_assets
                        .iter()
                        .map(|asset| async move {
                            Ok(WebpackStatsEntrypointAssets {
                                name: asset.await?.path.as_str().into(),
                            })
                        })
                        .try_join()
                        .await?;
                    entry_assets.push(WebpackStatsEntrypointAssets {
                        name: entry_path.clone(),
                    });

                    let entry_name: RcStr = QString::from((*chunk).ident().await?.query.as_str())
                        .get("name")
                        .unwrap_or(remove_extension_from_str(entry_path.as_str()))
                        .into();
                    {
                        let mut entrypoints = entrypoints.lock();
                        entrypoints.insert(
                            entry_name.clone(),
                            WebpackStatsEntrypoint {
                                name: entry_name.clone(),
                                chunks: entry_chunks,
                                assets: entry_assets,
                            },
                        );
                    }
                }

                if let Some(chunk) = ResolvedVc::try_downcast_type::<EcmascriptBrowserChunk>(*asset)
                {
                    let chunk_ident = &chunk.path().await?.path;
                    {
                        let mut chunks = chunks.lock();
                        chunks.push(WebpackStatsChunk {
                            size: asset_len,
                            files: vec![chunk_ident.clone()],
                            id: chunk_ident.clone(),
                            ..Default::default()
                        });
                    }

                    chunk
                        .chunk()
                        .chunk_items()
                        .await?
                        .into_iter()
                        .for_each(|item| {
                            let mut chunk_items = chunk_items.lock();
                            chunk_items
                                .entry(**item)
                                .or_default()
                                .insert(chunk_ident.clone());
                        });
                }

                if let Some(chunk) = ResolvedVc::try_downcast_type::<CssChunk>(*asset) {
                    let chunk_ident = &chunk.path().await?.path;
                    {
                        let mut chunks = chunks.lock();
                        chunks.push(WebpackStatsChunk {
                            size: asset_len,
                            files: vec![chunk_ident.clone()],
                            id: chunk_ident.clone(),
                            ..Default::default()
                        });
                    }
                }

                let path = &asset.path().await?.path;
                {
                    let mut assets = assets.lock();
                    assets.push(WebpackStatsAsset {
                        ty: "asset".into(),
                        name: path.clone(),
                        chunks: vec![path.clone()],
                        size: asset_len,
                        ..Default::default()
                    });
                }
                Ok::<(), Error>(())
            }
        })
        .try_join()
        .await?;

    let chunk_items = Arc::into_inner(chunk_items).unwrap().into_inner();
    chunk_items
        .iter()
        .map(|(chunk_item, chunks)| async {
            let size = match chunk_item.content_ident().await?.path.read().len().await {
                Ok(size_vc) => *size_vc,
                Err(_) => {
                    // For virtual file system or other read errors, use None as size
                    // This prevents the build from failing when dealing with virtual files
                    None
                }
            };
            let path = chunk_item.asset_ident().path().await?.path.clone();
            {
                let mut modules = modules.lock();
                modules.push(WebpackStatsModule {
                    name: path.clone(),
                    id: path.clone(),
                    chunks: chunks.iter().cloned().collect(),
                    size,
                });
            }
            Ok(())
        })
        .try_join()
        .await?;

    Ok(WebpackStats {
        assets: Arc::into_inner(assets).unwrap().into_inner(),
        entrypoints: Arc::into_inner(entrypoints).unwrap().into_inner(),
        chunks: Arc::into_inner(chunks).unwrap().into_inner(),
        modules: Arc::into_inner(modules).unwrap().into_inner(),
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

#[turbo_tasks::value]
#[derive(Default)]
pub struct WebpackStatsAssetInfo {}

#[turbo_tasks::value]
#[derive(Default)]
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
#[derive(Default)]
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
pub struct WebpackStatsModule {
    pub name: RcStr,
    pub id: RcStr,
    pub chunks: Vec<RcStr>,
    pub size: Option<u64>,
}

#[turbo_tasks::value]
pub struct WebpackStatsEntrypointAssets {
    pub name: RcStr,
}

#[turbo_tasks::value]
pub struct WebpackStatsEntrypoint {
    pub name: RcStr,
    pub chunks: Vec<RcStr>,
    pub assets: Vec<WebpackStatsEntrypointAssets>,
}

#[turbo_tasks::value]
pub struct WebpackStats {
    pub assets: Vec<WebpackStatsAsset>,
    pub entrypoints: FxIndexMap<RcStr, WebpackStatsEntrypoint>,
    pub chunks: Vec<WebpackStatsChunk>,
    pub modules: Vec<WebpackStatsModule>,
}
