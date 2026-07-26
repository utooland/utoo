use anyhow::{Context, Result};
use bincode::{Decode, Encode};
use rustc_hash::FxHashSet;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, Vc, fxindexmap, trace::TraceRawVcs};
use turbopack::{ModuleAssetContext, module_options::CustomModuleType};
use turbopack_core::{
    asset::{Asset, AssetContent},
    chunk::{
        AsyncModuleInfo, ChunkItem, ChunkItemOrBatchWithAsyncModuleInfo,
        ChunkItemWithAsyncModuleInfo, ChunkType, ChunkableModule, ChunkingContext,
    },
    context::AssetContext,
    ident::AssetIdent,
    module::{Module, ModuleSideEffects},
    module_graph::ModuleGraph,
    output::{OutputAsset, OutputAssets, OutputAssetsReference, OutputAssetsWithReferenced},
    reference::ModuleReferences,
    reference_type::{CssReferenceSubType, ReferenceType},
    source::{OptionSource, Source},
};
use turbopack_css::{
    CssModule, CssModuleType, LightningCssFeatureFlags,
    chunk::{CssChunk, CssChunkItem, CssChunkType, CssImport, source_map::CssChunkSourceMapAsset},
};
use turbopack_ecmascript::{
    EcmascriptInputTransforms,
    chunk::{
        EcmascriptChunkItemContent, EcmascriptChunkPlaceable, EcmascriptExports,
        ecmascript_chunk_item,
    },
    runtime_functions::TURBOPACK_EXPORT_VALUE,
    utils::StringifyJs,
};

use super::source_asset::{INLINE_CSS_CONTENT, InlineCssFileSource};

#[turbo_tasks::task_input]
#[derive(Eq, PartialEq, Clone, Copy, Debug, PartialOrd, Ord, Hash, TraceRawVcs, Encode, Decode)]
pub enum InjectType {
    Style,
    SingletonStyle,
    Link,
    LazyStyle,
    LazySingletonStyle,
}

impl InjectType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "singletonStyleTag" => Self::SingletonStyle,
            "linkTag" => Self::Link,
            "lazyStyleTag" => Self::LazyStyle,
            "lazySingletonStyleTag" => Self::LazySingletonStyle,
            _ => Self::Style,
        }
    }
}

/// An ECMAScript value module containing CSS produced by Turbopack's standard CSS pipeline.
#[turbo_tasks::value]
struct InlineCssContentModule {
    source: ResolvedVc<Box<dyn Source>>,
    css: ResolvedVc<CssModule>,
}

#[turbo_tasks::value_impl]
impl InlineCssContentModule {
    #[turbo_tasks::function]
    async fn css_chunk(
        self: Vc<Self>,
        module_graph: Vc<ModuleGraph>,
        chunking_context: Vc<Box<dyn ChunkingContext>>,
    ) -> Result<Vc<CssChunk>> {
        let this = self.await?;
        let root_item = this
            .css
            .as_chunk_item(module_graph, chunking_context)
            .to_resolved()
            .await?;
        let root_item = ResolvedVc::try_downcast::<Box<dyn CssChunkItem>>(root_item)
            .context("inline CSS root did not produce a CSS chunk item")?;

        // Match the standard CSS chunk order: imported styles first, then the importer. The
        // CssChunk below remains responsible for import contexts, URL rewriting and printing.
        let mut seen = FxHashSet::default();
        let mut stack = vec![(root_item, false)];
        let mut chunk_items = Vec::new();
        while let Some((item, expanded)) = stack.pop() {
            if expanded {
                chunk_items.push(item);
                continue;
            }
            if !seen.insert(item) {
                continue;
            }

            stack.push((item, true));
            for import in item.content().await?.imports.iter().rev() {
                match import {
                    CssImport::Internal(_, imported) | CssImport::Composes(imported) => {
                        stack.push((*imported, false));
                    }
                    CssImport::External(_) => {}
                }
            }
        }

        let mut items_with_info = Vec::with_capacity(chunk_items.len());
        for item in chunk_items {
            let chunk_item: ResolvedVc<Box<dyn ChunkItem>> = ResolvedVc::upcast(item);
            let chunk_type = chunk_item
                .into_trait_ref()
                .await?
                .ty()
                .to_resolved()
                .await?;
            items_with_info.push(ChunkItemOrBatchWithAsyncModuleInfo::ChunkItem(
                ChunkItemWithAsyncModuleInfo {
                    chunk_item,
                    chunk_type,
                    module: None,
                    async_info: None,
                },
            ));
        }

        let chunk = Vc::<CssChunkType>::default()
            .chunk(chunking_context, items_with_info, Vec::new())
            .to_resolved()
            .await?;
        let css_chunk = ResolvedVc::try_downcast_type::<CssChunk>(chunk)
            .context("CSS chunk type did not produce a CSS chunk")?;
        Ok(*css_chunk)
    }
}

#[turbo_tasks::value_impl]
impl Module for InlineCssContentModule {
    #[turbo_tasks::function]
    async fn ident(&self) -> Result<Vc<AssetIdent>> {
        Ok(self
            .css
            .ident()
            .owned()
            .await?
            .with_modifier(rcstr!("inline css content"))
            .into_vc())
    }

    #[turbo_tasks::function]
    fn source(&self) -> Vc<OptionSource> {
        Vc::cell(Some(self.source))
    }

    #[turbo_tasks::function]
    fn references(&self) -> Vc<ModuleReferences> {
        ModuleReferences::empty()
    }

    #[turbo_tasks::function]
    fn side_effects(self: Vc<Self>) -> Vc<ModuleSideEffects> {
        ModuleSideEffects::SideEffectFree.cell()
    }
}

#[turbo_tasks::value_impl]
impl ChunkableModule for InlineCssContentModule {
    #[turbo_tasks::function]
    fn as_chunk_item(
        self: ResolvedVc<Self>,
        module_graph: ResolvedVc<ModuleGraph>,
        chunking_context: ResolvedVc<Box<dyn ChunkingContext>>,
    ) -> Vc<Box<dyn turbopack_core::chunk::ChunkItem>> {
        ecmascript_chunk_item(ResolvedVc::upcast(self), module_graph, chunking_context)
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkPlaceable for InlineCssContentModule {
    #[turbo_tasks::function]
    fn get_exports(&self) -> Vc<EcmascriptExports> {
        EcmascriptExports::Value.cell()
    }

    #[turbo_tasks::function]
    async fn chunk_item_content(
        self: Vc<Self>,
        chunking_context: Vc<Box<dyn ChunkingContext>>,
        module_graph: Vc<ModuleGraph>,
        _async_module_info: Option<Vc<AsyncModuleInfo>>,
        _estimated: bool,
    ) -> Result<Vc<EcmascriptChunkItemContent>> {
        let css_chunk = self.css_chunk(module_graph, chunking_context);
        let css_asset: Vc<Box<dyn Asset>> = Vc::upcast(css_chunk);
        let content = css_asset.content().await?;
        let AssetContent::File(content) = &*content else {
            anyhow::bail!("inline CSS chunk did not produce file content");
        };
        let content = content.await?;
        let Some(file) = content.as_content() else {
            anyhow::bail!("inline CSS chunk content was not found");
        };
        let css = file.content().to_str()?;
        let source_map_path = CssChunkSourceMapAsset::new(*css_chunk.to_resolved().await?)
            .path()
            .await?;
        let source_map_reference = format!(
            "/*# sourceMappingURL={}*/",
            urlencoding::encode(source_map_path.file_name())
        );
        // The synthetic CSS chunk is consumed as an ECMAScript string rather than emitted as a
        // standalone stylesheet. Its external source map would have no valid CSS asset to point
        // at and can collide when multiple inline styles share an output filename.
        let css = css.strip_suffix(&source_map_reference).unwrap_or(&css);
        let code = format!("{TURBOPACK_EXPORT_VALUE}({});\n", StringifyJs(&css));

        Ok(EcmascriptChunkItemContent {
            inner_code: code.into(),
            ..Default::default()
        }
        .cell())
    }

    #[turbo_tasks::function]
    async fn chunk_item_output_assets(
        self: Vc<Self>,
        chunking_context: Vc<Box<dyn ChunkingContext>>,
        module_graph: Vc<ModuleGraph>,
    ) -> Result<Vc<OutputAssetsWithReferenced>> {
        let css_chunk = self.css_chunk(module_graph, chunking_context).await?;
        let mut references = OutputAssetsWithReferenced::from_assets(OutputAssets::empty());
        for item in &css_chunk.content.await?.chunk_items {
            references = references.concatenate(item.references());
        }
        Ok(references)
    }
}

/// Custom module type that transforms CSS files into JavaScript modules that inject styles into
/// the DOM at runtime.
#[turbo_tasks::value]
pub struct InlineCssModuleType {
    pub insert: RcStr,
    pub inject_type: InjectType,
    pub css_modules_pattern: Option<RcStr>,
}

#[turbo_tasks::value_impl]
impl InlineCssModuleType {
    #[turbo_tasks::function]
    pub fn new(
        insert: RcStr,
        inject_type: InjectType,
        css_modules_pattern: Option<RcStr>,
    ) -> Vc<Self> {
        InlineCssModuleType {
            insert,
            inject_type,
            css_modules_pattern,
        }
        .cell()
    }

    #[turbo_tasks::function]
    pub(crate) async fn create_module(
        source: ResolvedVc<Box<dyn Source>>,
        module_asset_context: ResolvedVc<ModuleAssetContext>,
        reference_type: ReferenceType,
        insert: RcStr,
        inject_type: InjectType,
        css_modules_pattern: Option<RcStr>,
    ) -> Result<Vc<Box<dyn Module>>> {
        let asset_context: ResolvedVc<Box<dyn AssetContext>> =
            ResolvedVc::upcast(module_asset_context);
        let environment = asset_context
            .compile_time_info()
            .environment()
            .to_resolved()
            .await?;
        let is_at_import = matches!(
            &reference_type,
            ReferenceType::Css(CssReferenceSubType::AtImport(_))
        );
        let (css_type, import_context) = match reference_type {
            ReferenceType::Css(CssReferenceSubType::Inner) => (CssModuleType::Module, None),
            ReferenceType::Css(CssReferenceSubType::AtImport(import_context)) => {
                (CssModuleType::Default, import_context)
            }
            _ => (CssModuleType::Default, None),
        };
        let css = CssModule::new(
            *source,
            *asset_context,
            css_type,
            import_context.map(|context| *context),
            Some(*environment),
            LightningCssFeatureFlags::default(),
            css_modules_pattern,
        )
        .to_resolved()
        .await?;

        // Imported styles stay as standard CssModules. This is the same module type used by
        // Turbopack's normal CSS chunk pipeline and preserves media/layer/supports context.
        if is_at_import {
            return Ok(Vc::upcast(*css));
        }
        let content_module = InlineCssContentModule { source, css }.resolved_cell();

        Ok(module_asset_context
            .process(
                Vc::upcast(
                    InlineCssFileSource {
                        css: source,
                        insert,
                        inject_type,
                    }
                    .cell(),
                ),
                ReferenceType::Internal(ResolvedVc::cell(fxindexmap!(
                    rcstr!(INLINE_CSS_CONTENT) => ResolvedVc::upcast(content_module)
                ))),
            )
            .module())
    }
}

#[turbo_tasks::value_impl]
impl CustomModuleType for InlineCssModuleType {
    #[turbo_tasks::function]
    fn create_module(
        &self,
        source: Vc<Box<dyn Source>>,
        module_asset_context: Vc<ModuleAssetContext>,
        reference_type: ReferenceType,
    ) -> Vc<Box<dyn Module>> {
        InlineCssModuleType::create_module(
            source,
            module_asset_context,
            reference_type,
            self.insert.clone(),
            self.inject_type,
            self.css_modules_pattern.clone(),
        )
    }

    #[turbo_tasks::function]
    fn extend_ecmascript_transforms(
        self: Vc<Self>,
        _preprocess: Vc<EcmascriptInputTransforms>,
        _main: Vc<EcmascriptInputTransforms>,
        _postprocess: Vc<EcmascriptInputTransforms>,
    ) -> Result<Vc<Box<dyn CustomModuleType>>> {
        Ok(Vc::upcast(self))
    }
}
