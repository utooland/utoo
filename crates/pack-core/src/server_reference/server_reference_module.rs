use anyhow::Result;
use turbo_rcstr::rcstr;
use turbo_tasks::{ResolvedVc, Vc};
use turbopack_core::{
    chunk::{AsyncModuleInfo, ChunkItem, ChunkType, ChunkableModule, ChunkingContext},
    ident::AssetIdent,
    module::{Module, ModuleSideEffects},
    module_graph::ModuleGraph,
    output::OutputAssetsReference,
    reference::ModuleReferences,
    source::OptionSource,
};
use turbopack_ecmascript::chunk::{
    EcmascriptChunkItem, EcmascriptChunkItemContent, EcmascriptChunkPlaceable, EcmascriptChunkType,
    EcmascriptExports,
};

/// A **marker module** in the client module graph for `"use server"` modules.
///
/// This module is created by `ServerReferenceTransition` and placed into the
/// client module graph via the proxy's ghost transition import. It produces
/// **empty content** in client chunks — its sole purpose is to be discoverable
/// via `ResolvedVc::try_downcast_type::<ServerReferenceModule>()` during graph
/// traversal so the build can collect server modules and build them as Node.js.
///
/// The actual server code lives in `self.server_module`, which is built
/// separately by the `AppEndpoint` using the Node.js chunking context.
#[turbo_tasks::value]
pub struct ServerReferenceModule {
    pub server_ident: ResolvedVc<AssetIdent>,
    pub server_module: ResolvedVc<Box<dyn EcmascriptChunkPlaceable>>,
}

#[turbo_tasks::value_impl]
impl ServerReferenceModule {
    #[turbo_tasks::function]
    pub fn new(
        server_ident: ResolvedVc<AssetIdent>,
        server_module: ResolvedVc<Box<dyn EcmascriptChunkPlaceable>>,
    ) -> Vc<Self> {
        ServerReferenceModule {
            server_ident,
            server_module,
        }
        .cell()
    }
}

#[turbo_tasks::value_impl]
impl Module for ServerReferenceModule {
    #[turbo_tasks::function]
    async fn ident(&self) -> Result<Vc<AssetIdent>> {
        let ident = self
            .server_ident
            .owned()
            .await?
            .with_modifier(rcstr!("server reference"));

        Ok(ident.into_vc())
    }

    #[turbo_tasks::function]
    fn source(&self) -> Vc<OptionSource> {
        Vc::cell(None)
    }

    #[turbo_tasks::function]
    async fn references(&self) -> Result<Vc<ModuleReferences>> {
        // No references — the server module is NOT part of the client chunk graph.
        // It will be discovered via graph traversal and built separately
        // using project.server_fn_module_graph().
        Ok(Vc::cell(vec![]))
    }

    #[turbo_tasks::function]
    fn side_effects(self: Vc<Self>) -> Vc<ModuleSideEffects> {
        // Side-effectful to ensure it's not tree-shaken from the graph
        ModuleSideEffects::SideEffectful.cell()
    }
}

#[turbo_tasks::value_impl]
impl ChunkableModule for ServerReferenceModule {
    #[turbo_tasks::function]
    async fn as_chunk_item(
        self: ResolvedVc<Self>,
        _module_graph: Vc<ModuleGraph>,
        chunking_context: ResolvedVc<Box<dyn ChunkingContext>>,
    ) -> Result<Vc<Box<dyn ChunkItem>>> {
        Ok(Vc::upcast(
            ServerReferenceChunkItem {
                inner_module: self,
                chunking_context,
            }
            .cell(),
        ))
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkPlaceable for ServerReferenceModule {
    #[turbo_tasks::function]
    fn get_exports(&self) -> Vc<EcmascriptExports> {
        // Empty exports — the client proxy uses callServer stubs, not these exports
        EcmascriptExports::None.cell()
    }

    #[turbo_tasks::function]
    fn chunk_item_content(
        self: Vc<Self>,
        _chunking_context: Vc<Box<dyn ChunkingContext>>,
        _module_graph: Vc<ModuleGraph>,
        _async_module_info: Option<Vc<AsyncModuleInfo>>,
        _estimated: bool,
    ) -> Result<Vc<EcmascriptChunkItemContent>> {
        // Empty — this module is a graph marker, not real code
        Ok(EcmascriptChunkItemContent::default().cell())
    }
}

/// Chunk item that produces empty content for the client bundle.
#[turbo_tasks::value]
struct ServerReferenceChunkItem {
    inner_module: ResolvedVc<ServerReferenceModule>,
    chunking_context: ResolvedVc<Box<dyn ChunkingContext>>,
}

#[turbo_tasks::value_impl]
impl OutputAssetsReference for ServerReferenceChunkItem {}

#[turbo_tasks::value_impl]
impl ChunkItem for ServerReferenceChunkItem {
    #[turbo_tasks::function]
    fn asset_ident(&self) -> Vc<AssetIdent> {
        self.inner_module.ident()
    }

    #[turbo_tasks::function]
    fn chunking_context(&self) -> Vc<Box<dyn ChunkingContext>> {
        *self.chunking_context
    }

    #[turbo_tasks::function]
    fn ty(&self) -> Vc<Box<dyn ChunkType>> {
        Vc::upcast(Vc::<EcmascriptChunkType>::default())
    }

    #[turbo_tasks::function]
    fn module(&self) -> Vc<Box<dyn Module>> {
        Vc::upcast(*self.inner_module)
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItem for ServerReferenceChunkItem {
    #[turbo_tasks::function]
    fn content(&self) -> Vc<EcmascriptChunkItemContent> {
        // Empty content — server code is built separately as Node.js
        EcmascriptChunkItemContent::default().cell()
    }

    #[turbo_tasks::function]
    fn content_with_async_module_info(
        &self,
        _async_module_info: Option<Vc<AsyncModuleInfo>>,
        _estimated: bool,
    ) -> Vc<EcmascriptChunkItemContent> {
        EcmascriptChunkItemContent::default().cell()
    }
}
