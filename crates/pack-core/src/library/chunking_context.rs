use anyhow::{Context, Result, bail};
use qstring::QString;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use tracing::Instrument;
use turbo_rcstr::RcStr;
use turbo_tasks::{
    NonLocalValue, ResolvedVc, TaskInput, TryJoinIterExt, ValueToString, Vc, trace::TraceRawVcs,
};
use turbo_tasks_fs::FileSystemPath;
use turbo_tasks_hash::{DeterministicHash, Xxh3Hash64Hasher, encode_hex};
use turbopack_browser::chunking_context::{
    clean_separators, match_content_hash_placeholder, match_name_placeholder,
    replace_content_hash_placeholder, replace_name_placeholder,
};
use turbopack_core::{
    asset::Asset,
    chunk::{
        Chunk, ChunkGroupResult, ChunkItem, ChunkableModule, ChunkingConfig, ChunkingConfigs,
        ChunkingContext, EntryChunkGroupResult, EvaluatableAsset, EvaluatableAssets, MinifyType,
        ModuleId, SourceMapsType,
        availability_info::AvailabilityInfo,
        chunk_group::{MakeChunkGroupResult, make_chunk_group},
        module_id_strategies::{DevModuleIdStrategy, ModuleIdStrategy},
    },
    environment::Environment,
    ident::AssetIdent,
    module::Module,
    module_graph::{
        ModuleGraph,
        chunk_group_info::ChunkGroup,
        export_usage::{ExportUsageInfo, ModuleExportUsageInfo},
    },
    output::{OutputAsset, OutputAssets},
};
use turbopack_ecmascript::chunk::{EcmascriptChunk, EcmascriptChunkType};
use turbopack_ecmascript_runtime::RuntimeType;

use crate::library::ecmascript::chunk::EcmascriptLibraryEvaluateChunk;

#[derive(
    Debug,
    TaskInput,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    TraceRawVcs,
    DeterministicHash,
    NonLocalValue,
)]
pub enum ContentHashing {
    /// Direct content hashing: Embeds the chunk content hash directly into the referencing chunk.
    /// Benefit: No hash manifest needed.
    /// Downside: Causes cascading hash invalidation.
    Direct {
        /// The length of the content hash in hex chars. Anything lower than 8 is not recommended
        /// due to the high risk of collisions.
        length: u8,
    },
}

pub struct LibraryChunkingContextBuilder {
    chunking_context: LibraryChunkingContext,
}

impl LibraryChunkingContextBuilder {
    pub fn name(mut self, name: RcStr) -> Self {
        self.chunking_context.name = Some(name);
        self
    }

    pub fn runtime_root(mut self, runtime_root: Option<RcStr>) -> Self {
        self.chunking_context.runtime_root = runtime_root;
        self
    }

    pub fn runtime_export(mut self, export: Vec<RcStr>) -> Self {
        self.chunking_context.runtime_export = export;
        self
    }

    pub fn runtime_type(mut self, runtime_type: RuntimeType) -> Self {
        self.chunking_context.runtime_type = runtime_type;
        self
    }

    pub fn minify_type(mut self, minify_type: MinifyType) -> Self {
        self.chunking_context.minify_type = minify_type;
        self
    }

    pub fn source_maps(mut self, source_maps: SourceMapsType) -> Self {
        self.chunking_context.source_maps_type = source_maps;
        self
    }

    pub fn use_file_source_map_uris(mut self) -> Self {
        self.chunking_context.should_use_file_source_map_uris = true;
        self
    }

    pub fn module_id_strategy(
        mut self,
        module_id_strategy: ResolvedVc<Box<dyn ModuleIdStrategy>>,
    ) -> Self {
        self.chunking_context.module_id_strategy = module_id_strategy;
        self
    }

    pub fn export_usage(mut self, export_usage: Option<ResolvedVc<ExportUsageInfo>>) -> Self {
        self.chunking_context.export_usage = export_usage;
        self
    }

    pub fn filename(mut self, filename: RcStr) -> Self {
        self.chunking_context.filename = Some(filename);
        self
    }

    pub fn build(self) -> Vc<LibraryChunkingContext> {
        LibraryChunkingContext::cell(self.chunking_context)
    }
}

/// A chunking context for development mode.
///
/// It uses readable filenames and module ids to improve development.
/// It also uses a chunking heuristic that is incremental and cacheable.
/// It splits "node_modules" separately as these are less likely to change
/// during development
#[turbo_tasks::value]
#[derive(Debug, Clone, Hash, TaskInput)]
pub struct LibraryChunkingContext {
    name: Option<RcStr>,
    /// The library root name
    runtime_root: Option<RcStr>,
    /// The library export subpaths
    runtime_export: Vec<RcStr>,
    /// The root path of the project
    root_path: FileSystemPath,
    /// Whether to write file sources as file:// paths in source maps
    should_use_file_source_map_uris: bool,
    /// This path is used to compute the url to request chunks from
    output_root: FileSystemPath,
    /// The relative path from the output_root to the root_path.
    output_root_to_root_path: RcStr,
    /// The environment chunks will be evaluated in.
    environment: ResolvedVc<Environment>,
    /// The kind of runtime to include in the output.
    runtime_type: RuntimeType,
    /// Whether to minify resulting chunks
    minify_type: MinifyType,
    /// Whether to generate source maps
    source_maps_type: SourceMapsType,
    /// The module id strategy to use
    module_id_strategy: ResolvedVc<Box<dyn ModuleIdStrategy>>,
    /// The module export usage info, if available.
    export_usage: Option<ResolvedVc<ExportUsageInfo>>,
    /// Evaluate chunk filename template
    filename: Option<RcStr>,
}

impl LibraryChunkingContext {
    pub fn builder(
        root_path: FileSystemPath,
        output_root: FileSystemPath,
        output_root_to_root_path: RcStr,
        environment: ResolvedVc<Environment>,
        runtime_type: RuntimeType,
        runtime_root: Option<RcStr>,
        runtime_export: Vec<RcStr>,
    ) -> LibraryChunkingContextBuilder {
        LibraryChunkingContextBuilder {
            chunking_context: LibraryChunkingContext {
                name: None,
                root_path,
                output_root,
                output_root_to_root_path,
                should_use_file_source_map_uris: false,
                environment,
                runtime_type,
                minify_type: MinifyType::NoMinify,
                source_maps_type: SourceMapsType::Full,
                module_id_strategy: ResolvedVc::upcast(DevModuleIdStrategy::new_resolved()),
                export_usage: None,
                filename: Default::default(),
                runtime_root,
                runtime_export,
            },
        }
    }
}

impl LibraryChunkingContext {
    /// Returns the kind of runtime to include in output chunks.
    ///
    /// This is defined directly on `LibraryChunkingContext` so it is zero-cost
    /// when `RuntimeType` has a single variant.
    pub fn runtime_type(&self) -> RuntimeType {
        self.runtime_type
    }

    /// Returns the minify type.
    pub fn source_maps_type(&self) -> SourceMapsType {
        self.source_maps_type
    }

    /// Returns the minify type.
    pub fn minify_type(&self) -> MinifyType {
        self.minify_type
    }
}

#[turbo_tasks::value_impl]
impl LibraryChunkingContext {
    #[turbo_tasks::function]
    async fn generate_chunk(
        self: Vc<Self>,
        ident: Vc<AssetIdent>,
        chunk: Vc<Box<dyn Chunk>>,
        evaluatable_assets: Vc<EvaluatableAssets>,
    ) -> Result<Vc<Box<dyn OutputAsset>>> {
        Ok(
            if let Some(ecmascript_chunk) =
                Vc::try_resolve_downcast_type::<EcmascriptChunk>(chunk).await?
            {
                let ident =
                    self.ecmascript_chunk_ident_with_filename_template(ident, ecmascript_chunk);
                Vc::upcast(EcmascriptLibraryEvaluateChunk::new(
                    self,
                    ident,
                    ecmascript_chunk,
                    evaluatable_assets,
                ))
            } else if let Some(output_asset) =
                Vc::try_resolve_sidecast::<Box<dyn OutputAsset>>(chunk).await?
            {
                output_asset
            } else {
                bail!("Unable to generate output asset for chunk");
            },
        )
    }

    #[turbo_tasks::function]
    pub(crate) async fn ecmascript_chunk_ident_with_filename_template(
        self: Vc<Self>,
        ident: Vc<AssetIdent>,
        ecmascript_chunk: Vc<EcmascriptChunk>,
    ) -> Result<Vc<AssetIdent>> {
        let query = QString::from(ident.await?.query.as_str());
        let Some(name) = query.get("name") else {
            bail!("Failed to get name for entry")
        };
        let this = self.await?;
        let root = &this.root_path;
        if let Some(filename) = self.await?.filename.as_ref() {
            let mut filename = filename.to_string();
            if match_name_placeholder(&filename) {
                filename = replace_name_placeholder(&filename, name);
            }
            if match_content_hash_placeholder(&filename) {
                let content_hash = self.ecmascript_chunk_content_hash(ecmascript_chunk).await?;
                filename = replace_content_hash_placeholder(&filename, &content_hash);
            };
            Ok(AssetIdent::from_path(root.join(&filename)?))
        } else {
            Ok(AssetIdent::from_path(root.join(name)?))
        }
    }

    #[turbo_tasks::function]
    pub(crate) fn runtime_root(&self) -> Vc<Option<RcStr>> {
        Vc::cell(self.runtime_root.clone())
    }

    #[turbo_tasks::function]
    pub(crate) fn runtime_export(&self) -> Vc<Vec<RcStr>> {
        Vc::cell(self.runtime_export.clone())
    }

    #[turbo_tasks::function]
    pub(crate) async fn ecmascript_chunk_content_hash(
        self: Vc<Self>,
        ecmascript_chunk: Vc<EcmascriptChunk>,
    ) -> Result<Vc<RcStr>> {
        let minify_type = self.minify_type().await?;
        let chunk_items = ecmascript_chunk
            .chunk_content()
            .await?
            .chunk_item_code_and_ids()
            .await?;

        let mut hasher = Xxh3Hash64Hasher::new();
        hasher.write_ref(&minify_type);
        hasher.write_value(chunk_items.len());

        for item in &chunk_items {
            for (module_id, code) in item {
                hasher.write_value((module_id, code.source_code()));
            }
        }

        let hash = hasher.finish();
        let hex_hash = encode_hex(hash);

        Ok(Vc::cell(hex_hash.into()))
    }
}

#[turbo_tasks::value_impl]
impl ChunkingContext for LibraryChunkingContext {
    #[turbo_tasks::function]
    fn name(&self) -> Vc<RcStr> {
        if let Some(name) = &self.name {
            Vc::cell(name.clone())
        } else {
            Vc::cell("unknown".into())
        }
    }

    #[turbo_tasks::function]
    fn root_path(&self) -> Vc<FileSystemPath> {
        self.root_path.clone().cell()
    }

    #[turbo_tasks::function]
    fn output_root(&self) -> Vc<FileSystemPath> {
        self.output_root.clone().cell()
    }

    #[turbo_tasks::function]
    fn output_root_to_root_path(&self) -> Vc<RcStr> {
        Vc::cell(self.output_root_to_root_path.clone())
    }

    #[turbo_tasks::function]
    fn environment(&self) -> Vc<Environment> {
        *self.environment
    }

    #[turbo_tasks::function]
    async fn chunk_root_path(&self) -> Vc<FileSystemPath> {
        self.output_root.clone().cell()
    }

    #[turbo_tasks::function]
    async fn chunk_path(
        &self,
        _asset: Option<Vc<Box<dyn Asset>>>,
        ident: Vc<AssetIdent>,
        extension: RcStr,
    ) -> Result<Vc<FileSystemPath>> {
        let evaluate = ident
            .await?
            .modifiers
            .iter()
            .any(|m| m.contains("evaluate"));

        if !evaluate {
            bail!(
                "library should only generate a single evaluate chunk, please enable inline features of styles.inlineCss and images.inlineLimit in config"
            )
        }

        let root_path = &self.output_root;
        let mut name = ident_to_output_filename(ident, self.root_path.clone(), extension.clone())
            .owned()
            .await?;

        // Check if the name already ends with the extension
        if !name.ends_with(&*extension) {
            // If doesn't end with extension, add the provided extension
            name = format!("{name}{extension}").into();
        }

        root_path.join(&name).map(|p| p.cell())
    }

    #[turbo_tasks::function]
    pub fn minify_type(&self) -> Vc<MinifyType> {
        self.minify_type.cell()
    }

    #[turbo_tasks::function]
    async fn asset_url(&self, ident: FileSystemPath) -> Result<Vc<RcStr>> {
        let asset_path = ident.to_string();

        Ok(Vc::cell(asset_path.into()))
    }

    #[turbo_tasks::function]
    fn reference_chunk_source_maps(&self, _chunk: Vc<Box<dyn OutputAsset>>) -> Vc<bool> {
        Vc::cell(match self.source_maps_type {
            SourceMapsType::Full => true,
            SourceMapsType::None => false,
        })
    }

    #[turbo_tasks::function]
    fn reference_module_source_maps(&self, _module: Vc<Box<dyn Module>>) -> Vc<bool> {
        Vc::cell(match self.source_maps_type {
            SourceMapsType::Full => true,
            SourceMapsType::None => false,
        })
    }

    #[turbo_tasks::function]
    async fn asset_path(
        &self,
        content_hash: RcStr,
        original_asset_ident: Vc<AssetIdent>,
    ) -> Result<Vc<FileSystemPath>> {
        let source_path = original_asset_ident.path().await?;
        let basename = source_path.file_name();
        let asset_path = match source_path.extension_ref() {
            Some(ext) => format!(
                "{basename}.{content_hash}.{ext}",
                basename = &basename[..basename.len() - ext.len() - 1],
                content_hash = &content_hash[..8]
            ),
            None => format!(
                "{basename}.{content_hash}",
                content_hash = &content_hash[..8]
            ),
        };
        self.output_root.join(&asset_path).map(|p| p.cell())
    }

    #[turbo_tasks::function]
    async fn chunking_configs(&self) -> Result<Vc<ChunkingConfigs>> {
        let mut map = FxHashMap::default();
        map.insert(
            ResolvedVc::upcast(Vc::<EcmascriptChunkType>::default().to_resolved().await?),
            ChunkingConfig {
                min_chunk_size: usize::MAX,
                max_chunk_count_per_group: 1,
                max_merge_chunk_size: usize::MAX,
                ..Default::default()
            },
        );
        Ok(Vc::cell(map))
    }

    #[turbo_tasks::function]
    fn should_use_file_source_map_uris(&self) -> Vc<bool> {
        Vc::cell(self.should_use_file_source_map_uris)
    }

    #[turbo_tasks::function]
    async fn chunk_group(
        self: ResolvedVc<Self>,
        _ident: Vc<AssetIdent>,
        _chunk_group: ChunkGroup,
        _module_graph: Vc<ModuleGraph>,
        _availability_info: AvailabilityInfo,
    ) -> Result<Vc<ChunkGroupResult>> {
        bail!("Library chunking context does not support chunk groups")
    }

    #[turbo_tasks::function]
    async fn evaluated_chunk_group(
        self: ResolvedVc<Self>,
        ident: Vc<AssetIdent>,
        chunk_group: ChunkGroup,
        module_graph: Vc<ModuleGraph>,
        availability_info: AvailabilityInfo,
    ) -> Result<Vc<ChunkGroupResult>> {
        let span = {
            let ident = ident.to_string().await?.to_string();
            tracing::info_span!("chunking", chunking_type = "evaluated", ident = ident)
        };
        async move {
            let entries = chunk_group.entries();

            let MakeChunkGroupResult {
                chunks,
                availability_info,
            } = make_chunk_group(
                entries,
                module_graph,
                ResolvedVc::upcast(self),
                availability_info,
            )
            .await?;

            let evaluatable_assets = Vc::cell(
                chunk_group
                    .entries()
                    .map(|m| {
                        ResolvedVc::try_downcast::<Box<dyn EvaluatableAsset>>(m)
                            .context("evaluated_chunk_group entries must be evaluatable assets")
                    })
                    .collect::<Result<Vec<_>>>()?,
            );

            let assets: Vec<ResolvedVc<Box<dyn OutputAsset>>> = chunks
                .iter()
                .map(|chunk| {
                    self.generate_chunk(ident, **chunk, evaluatable_assets)
                        .to_resolved()
                })
                .try_join()
                .await?;

            Ok(ChunkGroupResult {
                assets: ResolvedVc::cell(assets),
                availability_info,
            }
            .cell())
        }
        .instrument(span)
        .await
    }

    #[turbo_tasks::function]
    fn entry_chunk_group(
        self: Vc<Self>,
        _path: FileSystemPath,
        _evaluatable_assets: Vc<EvaluatableAssets>,
        _module_graph: Vc<ModuleGraph>,
        _extra_chunks: Vc<OutputAssets>,
        _availability_info: AvailabilityInfo,
    ) -> Result<Vc<EntryChunkGroupResult>> {
        bail!("Library chunking context does not support entry chunk groups")
    }

    #[turbo_tasks::function]
    fn chunk_item_id_from_ident(&self, ident: Vc<AssetIdent>) -> Vc<ModuleId> {
        self.module_id_strategy.get_module_id(ident)
    }

    #[turbo_tasks::function]
    async fn async_loader_chunk_item(
        self: Vc<Self>,
        _module: Vc<Box<dyn ChunkableModule>>,
        _module_graph: Vc<ModuleGraph>,
        _availability_info: AvailabilityInfo,
    ) -> Result<Vc<Box<dyn ChunkItem>>> {
        bail!("Library chunking context does not support async loader chunk item")
    }

    #[turbo_tasks::function]
    async fn async_loader_chunk_item_id(
        self: Vc<Self>,
        _module: Vc<Box<dyn ChunkableModule>>,
    ) -> Result<Vc<ModuleId>> {
        bail!("Library chunking context does not support async loader chunk item id")
    }

    #[turbo_tasks::function]
    async fn module_export_usage(
        self: Vc<Self>,
        module: ResolvedVc<Box<dyn Module>>,
    ) -> Result<Vc<ModuleExportUsageInfo>> {
        if let Some(export_usage) = self.await?.export_usage {
            Ok(export_usage.await?.used_exports(module))
        } else {
            Ok(ModuleExportUsageInfo::all())
        }
    }
}

#[turbo_tasks::function]
async fn ident_to_output_filename(
    ident: Vc<AssetIdent>,
    context_path: FileSystemPath,
    expected_extension: RcStr,
) -> Result<Vc<RcStr>> {
    let ident = &*ident.await?;
    let mut name = if let Some(inner) = context_path.get_path_to(&ident.path) {
        clean_separators(inner)
    } else {
        clean_separators(&ident.path.to_string())
    };
    let removed_extension = name.ends_with(&*expected_extension);
    if removed_extension {
        name.truncate(name.len() - expected_extension.len());
    }
    Ok(Vc::cell(name.into()))
}
