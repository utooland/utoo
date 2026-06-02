use anyhow::{Result, bail};
use indoc::writedoc;
use serde::Serialize;
use std::io::Write;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, TryJoinIterExt, ValueToString, Vc};
use turbo_tasks_fs::{File, FileContent, FileSystemPath, rope::RopeBuilder};
use turbopack_core::{
    asset::{Asset, AssetContent},
    chunk::{
        ChunkData, ChunkingContext, ChunksData, EvaluatableAssets, MinifyType,
        ModuleChunkItemIdExt, ModuleId,
    },
    code_builder::{Code, CodeBuilder},
    environment::{EdgeWorkerEnvironment, Environment, ExecutionEnvironment, NodeJsVersion},
    ident::AssetIdent,
    module_graph::ModuleGraph,
    output::{OutputAsset, OutputAssets, OutputAssetsReference, OutputAssetsWithReferenced},
    source_map::{GenerateSourceMap, SourceMapAsset},
};
use turbopack_ecmascript::{
    chunk::{EcmascriptChunk, EcmascriptChunkData, EcmascriptChunkPlaceable},
    minify::{get_compress_options, minify},
    utils::StringifyJs,
};
use turbopack_ecmascript_runtime::RuntimeType;

use crate::library::{LibraryChunkingContext, runtime::runtime_code::get_library_runtime_code};

#[turbo_tasks::value(shared)]
pub struct EcmascriptLibraryEvaluateChunk {
    chunking_context: ResolvedVc<LibraryChunkingContext>,
    ident: ResolvedVc<AssetIdent>,
    chunk: ResolvedVc<EcmascriptChunk>,
    other_chunks: ResolvedVc<OutputAssets>,
    module_graph: ResolvedVc<ModuleGraph>,
    pub(crate) evaluatable_assets: ResolvedVc<EvaluatableAssets>,
}

#[turbo_tasks::value_impl]
impl EcmascriptLibraryEvaluateChunk {
    #[turbo_tasks::function]
    pub fn new(
        chunking_context: ResolvedVc<LibraryChunkingContext>,
        ident: ResolvedVc<AssetIdent>,
        chunk: ResolvedVc<EcmascriptChunk>,
        other_chunks: ResolvedVc<OutputAssets>,
        evaluatable_assets: ResolvedVc<EvaluatableAssets>,
        module_graph: ResolvedVc<ModuleGraph>,
    ) -> Vc<Self> {
        EcmascriptLibraryEvaluateChunk {
            chunking_context,
            ident,
            chunk,
            other_chunks,
            evaluatable_assets,
            module_graph,
        }
        .cell()
    }

    #[turbo_tasks::function]
    async fn code(self: Vc<Self>) -> Result<Vc<Code>> {
        let this = self.await?;

        let output_root = this.chunking_context.output_root().await?;
        let output_root_to_root_path = this.chunking_context.output_root_to_root_path();
        let source_maps = *this
            .chunking_context
            .reference_chunk_source_maps(*ResolvedVc::upcast(self.to_resolved().await?))
            .await?;
        let chunk_path_vc = self.path();
        let chunk_path = chunk_path_vc.await?;
        let chunk_public_path = if let Some(path) = output_root.get_path_to(&chunk_path) {
            path
        } else {
            bail!(
                "chunk path {} is not in output root {}",
                chunk_path,
                output_root
            );
        };

        let other_chunks_data = self.chunks_data().await?;
        let other_chunks_data = other_chunks_data.iter().try_join().await?;
        let other_chunks_data: Vec<_> = other_chunks_data
            .iter()
            .map(|chunk_data| EcmascriptChunkData::new(chunk_data))
            .collect();

        let runtime_module_ids: Vec<turbopack_core::chunk::ModuleId> = this
            .evaluatable_assets
            .await?
            .iter()
            .map({
                let chunking_context = this.chunking_context;
                move |entry| async move {
                    if let Some(placeable) =
                        ResolvedVc::try_sidecast::<Box<dyn EcmascriptChunkPlaceable>>(*entry)
                    {
                        Ok(Some(
                            placeable
                                .chunk_item_id(Vc::upcast(*chunking_context))
                                .await?,
                        ))
                    } else {
                        Ok(None)
                    }
                }
            })
            .try_join()
            .await?
            .into_iter()
            .flatten()
            .collect();

        let mut code = CodeBuilder::default();

        writedoc!(
            code,
            r#"
                ((__UTOOPACK__) => {{
            "#,
        )?;

        let runtime_type = this.chunking_context.await?.runtime_type();

        // Get runtime code based on runtime type
        match runtime_type {
            RuntimeType::Development | RuntimeType::Production => {
                let runtime_code = get_library_runtime_code(
                    Environment::new(ExecutionEnvironment::EdgeWorker(
                        EdgeWorkerEnvironment {
                            node_version: NodeJsVersion::default().resolved_cell(),
                        }
                        .resolved_cell(),
                    )),
                    Vc::cell(None),
                    Vc::cell(None),
                    runtime_type,
                    output_root_to_root_path,
                    source_maps,
                    this.chunking_context.runtime_root(),
                    this.chunking_context.runtime_export(),
                    Vc::cell(
                        runtime_module_ids
                            .iter()
                            .map(|id| id.to_string().into())
                            .collect(),
                    ),
                    this.chunking_context.await?.is_node_platform(),
                )
                .await?;

                code.push_code(&runtime_code);
            }
            #[cfg(feature = "test")]
            RuntimeType::Dummy => {
                let runtime_code = turbopack_ecmascript_runtime::get_dummy_runtime_code();
                code.push_code(&runtime_code);
            }
        }
        // Emit two registrations matching upstream Turbopack's ChunkRegistration format:
        // 1. Module factories: [chunkPath, id, factory, ...] (length > 2)
        // 2. Bootstrap: [chunkPath, {otherChunks, runtimeModuleIds}] (length === 2)
        writedoc!(
            code,
            r#"
                }})([
                [{chunk_path},
            "#,
            chunk_path = StringifyJs(chunk_public_path)
        )?;

        let content = this.chunk.chunk_content().await?;
        let chunk_items = content.chunk_item_code_and_ids().await?;
        for item in &chunk_items {
            for (id, item_code) in &**item {
                write!(code, "\n{}, ", StringifyJs(id))?;
                code.push_code(item_code);
                write!(code, ",")?;
            }
        }

        let params = EcmascriptBrowserChunkRuntimeParams {
            other_chunks: &other_chunks_data,
            runtime_module_ids,
        };

        write!(code, "\n],")?;
        writeln!(
            code,
            "\n[{}, {}],",
            StringifyJs(chunk_public_path),
            StringifyJs(&params)
        )?;
        writeln!(code, "]);")?;

        let mut code = code.build();

        if let MinifyType::Minify { mangle, compress } = this.chunking_context.await?.minify_type()
        {
            code = minify(
                code,
                source_maps,
                mangle,
                get_compress_options(compress, mangle),
            )?;
        }

        Ok(code.cell())
    }

    #[turbo_tasks::function]
    pub async fn ident(&self) -> Result<Vc<AssetIdent>> {
        let ident = self
            .ident
            .owned()
            .await?
            .with_modifier(rcstr!("ecmascript library evaluate chunk"));

        Ok(ident.into_vc())
    }

    #[turbo_tasks::function]
    async fn source_map(self: Vc<Self>) -> Result<Vc<SourceMapAsset>> {
        let this = self.await?;
        Ok(SourceMapAsset::new(
            Vc::upcast(*this.chunking_context),
            self.ident(),
            Vc::upcast(self),
        ))
    }

    #[turbo_tasks::function]
    pub async fn chunks_data(&self) -> Result<Vc<ChunksData>> {
        Ok(ChunkData::from_assets(
            self.chunking_context.output_root().owned().await?,
            *self.other_chunks,
        ))
    }

    #[turbo_tasks::function]
    pub fn evaluatable_assets(&self) -> Vc<EvaluatableAssets> {
        *self.evaluatable_assets
    }

    #[turbo_tasks::function]
    pub fn module_graph(&self) -> Vc<ModuleGraph> {
        *self.module_graph
    }

    #[turbo_tasks::function]
    pub fn chunking_context(&self) -> Vc<Box<dyn ChunkingContext>> {
        Vc::upcast(*self.chunking_context)
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for EcmascriptLibraryEvaluateChunk {
    #[turbo_tasks::function]
    fn to_string(&self) -> Vc<RcStr> {
        Vc::cell("Ecmascript Library Chunk".into())
    }
}

#[turbo_tasks::value_impl]
impl OutputAsset for EcmascriptLibraryEvaluateChunk {
    #[turbo_tasks::function]
    async fn path(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let ident = self.ident();
        Ok(this
            .chunking_context
            .chunk_path(Some(Vc::upcast(self)), ident, None, ".js".into()))
    }
}

#[turbo_tasks::value_impl]
impl OutputAssetsReference for EcmascriptLibraryEvaluateChunk {
    #[turbo_tasks::function]
    async fn references(self: Vc<Self>) -> Result<Vc<OutputAssetsWithReferenced>> {
        let this = self.await?;
        let chunk_references = this.chunk.references().await?;
        let include_source_map = *this
            .chunking_context
            .reference_chunk_source_maps(Vc::upcast(self))
            .await?;
        let ref_assets = chunk_references.assets.await?;
        let mut assets =
            Vec::with_capacity(ref_assets.len() + if include_source_map { 1 } else { 0 });

        assets.extend(ref_assets.iter().copied());

        if include_source_map {
            assets.push(ResolvedVc::upcast(self.source_map().to_resolved().await?));
        }

        Ok(OutputAssetsWithReferenced {
            assets: ResolvedVc::cell(assets),
            referenced_assets: chunk_references.referenced_assets,
            references: chunk_references.references,
        }
        .cell())
    }
}

#[turbo_tasks::value_impl]
impl Asset for EcmascriptLibraryEvaluateChunk {
    #[turbo_tasks::function]
    async fn content(self: Vc<Self>) -> Result<Vc<AssetContent>> {
        let code = self.code().await?;

        let rope = if code.has_source_map() {
            let mut rope_builder = RopeBuilder::default();
            rope_builder.concat(code.source_code());
            let source_map_path = self.source_map().path().await?;
            write!(
                rope_builder,
                "\n\n//# sourceMappingURL={}",
                urlencoding::encode(source_map_path.file_name())
            )?;
            rope_builder.build()
        } else {
            code.source_code().clone()
        };

        Ok(AssetContent::file(
            FileContent::Content(File::from(rope)).cell(),
        ))
    }
}

#[turbo_tasks::value_impl]
impl GenerateSourceMap for EcmascriptLibraryEvaluateChunk {
    #[turbo_tasks::function]
    fn generate_source_map(self: Vc<Self>) -> Vc<FileContent> {
        self.code().generate_source_map()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EcmascriptBrowserChunkRuntimeParams<'a, T> {
    /// Other chunks in the chunk group this chunk belongs to, if any. Does not
    /// include the chunk itself.
    ///
    /// These chunks must be loaed before the runtime modules can be
    /// instantiated.
    other_chunks: &'a [T],
    /// List of module IDs that this chunk should instantiate when executed.
    runtime_module_ids: Vec<ModuleId>,
}

#[turbo_tasks::function]
fn modifier() -> Vc<RcStr> {
    Vc::cell("ecmascript library evaluate chunk".into())
}
