use anyhow::{Result, bail};
use indoc::writedoc;
use serde::Serialize;
use std::io::Write;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, TryJoinIterExt, ValueToString, Vc};
use turbo_tasks_fs::{File, FileContent, FileSystemPath, rope::RopeBuilder};
use turbopack_core::{
    asset::{Asset, AssetContent},
    chunk::{ChunkingContext, EvaluatableAssets, MinifyType, ModuleChunkItemIdExt, ModuleId},
    code_builder::{Code, CodeBuilder},
    environment::{EdgeWorkerEnvironment, Environment, ExecutionEnvironment, NodeJsVersion},
    ident::AssetIdent,
    output::{OutputAsset, OutputAssetsReference, OutputAssetsWithReferenced},
    source_map::{GenerateSourceMap, SourceMapAsset},
};
use turbopack_ecmascript::{
    chunk::{EcmascriptChunk, EcmascriptChunkData, EcmascriptChunkPlaceable},
    minify::minify,
    utils::StringifyJs,
};
use turbopack_ecmascript_runtime::RuntimeType;

use crate::library::{LibraryChunkingContext, runtime::runtime_code::get_library_runtime_code};

#[turbo_tasks::value(shared)]
pub struct EcmascriptLibraryEvaluateChunk {
    chunking_context: ResolvedVc<LibraryChunkingContext>,
    ident: ResolvedVc<AssetIdent>,
    chunk: ResolvedVc<EcmascriptChunk>,
    pub(crate) evaluatable_assets: ResolvedVc<EvaluatableAssets>,
}

#[turbo_tasks::value_impl]
impl EcmascriptLibraryEvaluateChunk {
    #[turbo_tasks::function]
    pub fn new(
        chunking_context: ResolvedVc<LibraryChunkingContext>,
        ident: ResolvedVc<AssetIdent>,
        chunk: ResolvedVc<EcmascriptChunk>,
        evaluatable_assets: ResolvedVc<EvaluatableAssets>,
    ) -> Vc<Self> {
        EcmascriptLibraryEvaluateChunk {
            chunking_context,
            ident,
            chunk,
            evaluatable_assets,
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
                ((__TURBOPACK__) => {{
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

        writedoc!(
            code,
            r#"
                }})([[{chunk_path}, {{
            "#,
            chunk_path = StringifyJs(chunk_public_path)
        )?;

        let content = this.chunk.chunk_content().await?;
        let chunk_items = content.chunk_item_code_and_ids().await?;
        for item in chunk_items {
            for (id, item_code) in item {
                write!(code, "\n{}: ", StringifyJs(&id))?;
                code.push_code(item_code);
                write!(code, ",")?;
            }
        }

        let params = EcmascriptBrowserChunkRuntimeParams {
            other_chunks: &Vec::<EcmascriptChunkData<'_>>::new(),
            runtime_module_ids,
        };

        write!(code, "\n}},")?;
        write!(code, "\n{},", StringifyJs(&params))?;
        writeln!(code, "\n]]);")?;

        let mut code = code.build();

        if let MinifyType::Minify { mangle } = this.chunking_context.await?.minify_type() {
            code = minify(code, source_maps, mangle)?;
        }

        Ok(code.cell())
    }

    #[turbo_tasks::function]
    async fn ident_for_path(&self) -> Result<Vc<AssetIdent>> {
        let mut ident = self.ident.owned().await?;

        ident.add_modifier(rcstr!("ecmascript library evaluate chunk"));

        Ok(AssetIdent::new(ident))
    }

    #[turbo_tasks::function]
    async fn source_map(self: Vc<Self>) -> Result<Vc<SourceMapAsset>> {
        let this = self.await?;
        Ok(SourceMapAsset::new(
            Vc::upcast(*this.chunking_context),
            self.ident_for_path(),
            Vc::upcast(self),
        ))
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
        let ident = self.ident_for_path();
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
